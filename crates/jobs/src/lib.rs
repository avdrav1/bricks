//! Long jobs off the UI thread (ENG-8, ADR 0005): index, save, copy, and later sort, filter,
//! search, and inference. Every job runs on one shared Rayon pool and gets a [`CancelToken`]
//! to check between chunks of work. Its result comes back over an `async-channel`: the UI
//! awaits [`Job::finished`] on its own loop (`glib::spawn_future_local`), and workers and
//! tests can block on [`Job::wait`].
//!
//! Progress stays with each job (bytes indexed, bytes written, rows copied) as a latest
//! value the UI reads on its own clock, so a stalled UI never builds a backlog of progress
//! messages.
//!
//! Cancelling is cooperative: a running job sees the token at its next check and returns
//! early. A job cancelled while it is still queued behind others never runs and reports
//! [`Stopped::Cancelled`] at once, so a busy pool can't hold up a cancel.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock};

/// Shared cancel flag of one job. Jobs check it at least every few milliseconds of work.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// The flag itself, for work that takes one (`SparseRowIndex::build`, `save_csv`).
    pub fn flag(&self) -> &AtomicBool {
        &self.0
    }
}

/// Why a job has no result of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// Cancelled before it started, so it never ran. A job cancelled while running
    /// returns whatever its work returns on cancel.
    Cancelled,
    /// The work panicked; the pool carries on.
    Panicked,
}

const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const SKIPPED: u8 = 2;

struct Shared<T> {
    state: AtomicU8,
    /// Capacity one; exactly one result is sent, by the worker or by a cancel while queued.
    tx: async_channel::Sender<Result<T, Stopped>>,
}

/// Handle to a job on the pool. Clones refer to the same job; only one of them receives
/// its result.
pub struct Job<T> {
    token: CancelToken,
    shared: Arc<Shared<T>>,
    rx: async_channel::Receiver<Result<T, Stopped>>,
}

impl<T> Clone for Job<T> {
    fn clone(&self) -> Self {
        Self {
            token: self.token.clone(),
            shared: self.shared.clone(),
            rx: self.rx.clone(),
        }
    }
}

/// Run `work` on the shared pool. It gets the job's cancel token and should return early,
/// with whatever its type says about an unfinished job, once the token is set.
pub fn spawn<T, F>(work: F) -> Job<T>
where
    T: Send + 'static,
    F: FnOnce(&CancelToken) -> T + Send + 'static,
{
    let (tx, rx) = async_channel::bounded(1);
    let token = CancelToken::default();
    let shared = Arc::new(Shared {
        state: AtomicU8::new(QUEUED),
        tx,
    });
    POOL.spawn({
        let (token, shared) = (token.clone(), shared.clone());
        move || {
            let start =
                shared
                    .state
                    .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire);
            if start.is_err() {
                return; // cancelled while queued; the result is already sent
            }
            let out =
                catch_unwind(AssertUnwindSafe(|| work(&token))).map_err(|_| Stopped::Panicked);
            let _ = shared.tx.try_send(out);
        }
    });
    Job { token, shared, rx }
}

impl<T> Job<T> {
    /// Ask the job to stop. A queued job is dropped and reports [`Stopped::Cancelled`] now.
    pub fn cancel(&self) {
        self.token.cancel();
        let skipped = self.shared.state.compare_exchange(
            QUEUED,
            SKIPPED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if skipped.is_ok() {
            let _ = self.shared.tx.try_send(Err(Stopped::Cancelled));
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Whether `other` is a handle to this same job.
    pub fn is(&self, other: &Job<T>) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// The job's result, once it has one.
    pub async fn finished(&self) -> Result<T, Stopped> {
        self.rx
            .recv()
            .await
            .expect("a job's sender lives as long as its handle")
    }

    /// Block until the job has a result. Not for the UI thread.
    pub fn wait(&self) -> Result<T, Stopped> {
        self.rx
            .recv_blocking()
            .expect("a job's sender lives as long as its handle")
    }
}

/// The pool every job shares: one thread per CPU but one, so the UI thread keeps a core
/// (ADR 0005). Data-parallel work inside a job (`par_iter`, `par_sort`) runs on it too.
static POOL: LazyLock<rayon::ThreadPool> = LazyLock::new(|| {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    rayon::ThreadPoolBuilder::new()
        .num_threads(cpus.saturating_sub(1).max(1))
        .thread_name(|i| format!("job-{i}"))
        .build()
        .expect("start the job pool")
});

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// A job that works in 1 ms steps until cancelled, as long jobs do in chunks. Returns
    /// once it is running (tests share the pool, so it may have queued).
    fn busy() -> Job<u32> {
        let (started_tx, started) = mpsc::channel();
        let job = spawn(move |cancel| {
            started_tx.send(()).unwrap();
            let mut steps = 0;
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
                steps += 1;
            }
            steps
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        job
    }

    #[test]
    fn a_running_job_returns_its_own_result_soon_after_cancel() {
        let job = busy();
        std::thread::sleep(Duration::from_millis(50));
        let t = Instant::now();
        job.cancel();
        let steps = job.wait().expect("it ran");
        assert!(steps > 0);
        assert!(
            t.elapsed() < Duration::from_millis(200),
            "{:?}",
            t.elapsed()
        );
    }

    #[test]
    fn a_queued_job_cancelled_behind_a_full_pool_reports_at_once_and_never_runs() {
        // Fill every pool thread with a job that waits for its release.
        let (started_tx, started) = mpsc::channel();
        let (releases, blockers): (Vec<mpsc::Sender<()>>, Vec<Job<()>>) = (0..POOL
            .current_num_threads())
            .map(|_| {
                let (release, gate) = mpsc::channel::<()>();
                let started_tx = started_tx.clone();
                let job = spawn(move |_| {
                    started_tx.send(()).unwrap();
                    gate.recv().unwrap();
                });
                (release, job)
            })
            .unzip();
        for _ in &blockers {
            started.recv_timeout(Duration::from_secs(5)).unwrap();
        }

        let ran = Arc::new(AtomicBool::new(false));
        let queued = spawn({
            let ran = ran.clone();
            move |_| ran.store(true, Ordering::Relaxed)
        });
        let t = Instant::now();
        queued.cancel();
        assert_eq!(queued.wait(), Err(Stopped::Cancelled));
        assert!(
            t.elapsed() < Duration::from_millis(200),
            "{:?}",
            t.elapsed()
        );

        for release in &releases {
            release.send(()).unwrap();
        }
        for b in &blockers {
            b.wait().unwrap();
        }
        // Once the pool is free, the dropped job still doesn't run.
        spawn(|_| ()).wait().unwrap();
        assert!(!ran.load(Ordering::Relaxed));
    }

    #[test]
    fn a_panicking_job_reports_it_and_the_pool_keeps_working() {
        let job = spawn(|_| -> u32 { panic!("boom") });
        assert_eq!(job.wait(), Err(Stopped::Panicked));
        assert_eq!(spawn(|_| 7).wait(), Ok(7));
    }
}
