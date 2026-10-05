// DEC-5 option A: Rayon pool + async-channel to the glib loop + AtomicBool cancel token.
#[path = "../../harness.rs"]
mod harness;

use harness::*;
use memmap2::Mmap;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

struct RayonEngine {
    pool: rayon::ThreadPool,
}

struct CancelToken(Arc<AtomicBool>);

impl Job for CancelToken {
    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct Rx(async_channel::Receiver<Msg>);

impl Inbox for Rx {
    async fn recv(&mut self) -> Option<Msg> {
        self.0.recv().await.ok()
    }
}

impl Engine for RayonEngine {
    type J = CancelToken;
    type R = Rx;

    fn start(&self, data: Arc<Mmap>, needle: Arc<[u8]>) -> (CancelToken, Rx) {
        let (tx, rx) = async_channel::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        self.pool.spawn(move || {
            let r = (0..chunk_count(data.len()))
                .into_par_iter()
                .try_for_each_init(Vec::new, |buf, i| {
                    if flag.load(Ordering::Relaxed) {
                        return Err(());
                    }
                    let hits = search_chunk(&data, i, &needle, buf);
                    let _ = tx.send_blocking(Msg::Progress {
                        hits,
                        sent: Instant::now(),
                    });
                    Ok(())
                });
            let _ = tx.send_blocking(Msg::Done {
                cancelled: r.is_err(),
                sent: Instant::now(),
            });
        });
        (CancelToken(cancel), Rx(rx))
    }
}

fn main() {
    let args = Args::parse();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()
        .unwrap();
    run("rayon", RayonEngine { pool }, args);
}
