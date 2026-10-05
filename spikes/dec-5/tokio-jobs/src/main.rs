// DEC-5 option B: tokio multi-thread runtime + tokio mpsc to the glib loop + CancellationToken.
// CPU work runs in async tasks that yield between 4 MiB chunks.
#[path = "../../harness.rs"]
mod harness;

use harness::*;
use memmap2::Mmap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct TokioEngine {
    rt: tokio::runtime::Runtime,
    tasks: usize,
}

struct Token(CancellationToken);

impl Job for Token {
    fn cancel(&self) {
        self.0.cancel();
    }
}

struct Rx(mpsc::UnboundedReceiver<Msg>);

impl Inbox for Rx {
    async fn recv(&mut self) -> Option<Msg> {
        self.0.recv().await
    }
}

impl Engine for TokioEngine {
    type J = Token;
    type R = Rx;

    fn start(&self, data: Arc<Mmap>, needle: Arc<[u8]>) -> (Token, Rx) {
        let (tx, rx) = mpsc::unbounded_channel();
        let token = CancellationToken::new();
        let (tok, tasks) = (token.clone(), self.tasks);
        self.rt.spawn(async move {
            let n = chunk_count(data.len());
            let next = Arc::new(AtomicUsize::new(0));
            let mut set = tokio::task::JoinSet::new();
            for _ in 0..tasks {
                let (data, needle, next, tok, tx) = (
                    data.clone(),
                    needle.clone(),
                    next.clone(),
                    tok.clone(),
                    tx.clone(),
                );
                set.spawn(async move {
                    let mut buf = Vec::new();
                    loop {
                        if tok.is_cancelled() {
                            return true;
                        }
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            return false;
                        }
                        let hits = search_chunk(&data, i, &needle, &mut buf);
                        let _ = tx.send(Msg::Progress {
                            hits,
                            sent: Instant::now(),
                        });
                        tokio::task::yield_now().await;
                    }
                });
            }
            let mut cancelled = false;
            while let Some(r) = set.join_next().await {
                cancelled |= r.unwrap();
            }
            let _ = tx.send(Msg::Done {
                cancelled,
                sent: Instant::now(),
            });
        });
        (Token(token), Rx(rx))
    }
}

fn main() {
    let args = Args::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.threads)
        .build()
        .unwrap();
    let tasks = args.threads;
    run("tokio", TokioEngine { rt, tasks }, args);
}
