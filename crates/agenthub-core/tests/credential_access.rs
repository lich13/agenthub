use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, TryLockError};
use std::time::Duration;

use agenthub_core::services::remote::credentials::{CredentialAccess, CredentialAccessError};
use tokio::sync::{oneshot, Barrier};

const WATCHDOG: Duration = Duration::from_secs(10);
const QUEUE_TIMEOUT: Duration = Duration::from_millis(40);
const READ_TIMEOUT: Duration = Duration::from_millis(200);

// Unwinding a failed assertion must also release any blocking fixture worker.
struct ReleaseWork(Option<mpsc::Sender<()>>);

impl Drop for ReleaseWork {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

fn held_work() -> (ReleaseWork, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    (ReleaseWork(Some(sender)), receiver)
}

fn wait_for_release(receiver: mpsc::Receiver<()>) {
    receiver
        .recv_timeout(WATCHDOG)
        .expect("fixture worker was not released");
}

async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(WATCHDOG, future)
        .await
        .expect("fixture watchdog elapsed")
}

#[derive(Default)]
struct WorkCounts {
    active: AtomicUsize,
    maximum: AtomicUsize,
}

impl WorkCounts {
    fn enter(self: &Arc<Self>) -> ActiveWork {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        ActiveWork(Arc::clone(self))
    }
}

struct ActiveWork(Arc<WorkCounts>);

impl Drop for ActiveWork {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_reads_and_transactions_share_one_active_worker() {
    const CALLERS: usize = 8;
    let access = Arc::new(CredentialAccess::default());
    let counts = Arc::new(WorkCounts::default());
    let barrier = Arc::new(Barrier::new(CALLERS + 1));
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let mut releases = Vec::new();
    let mut callers = Vec::new();

    for index in 0..CALLERS {
        let access = Arc::clone(&access);
        let counts = Arc::clone(&counts);
        let barrier = Arc::clone(&barrier);
        let started = started.clone();
        let (release, released) = held_work();
        releases.push(release);
        callers.push(tokio::spawn(async move {
            barrier.wait().await;
            let work = move || {
                let _active = counts.enter();
                started.send(index).expect("start observer disappeared");
                wait_for_release(released);
                index
            };
            if index % 2 == 0 {
                access.read(WATCHDOG, work).await
            } else {
                access.transaction(WATCHDOG, work).await
            }
        }));
    }

    bounded(barrier.wait()).await;
    bounded(starts.recv()).await.expect("no worker started");
    assert!(
        tokio::time::timeout(QUEUE_TIMEOUT, starts.recv())
            .await
            .is_err(),
        "another credential operation started while the first was blocked"
    );
    drop(releases);
    for (index, caller) in callers.into_iter().enumerate() {
        assert_eq!(
            bounded(caller)
                .await
                .expect("caller panicked")
                .expect("credential work failed"),
            index
        );
    }
    assert_eq!(counts.maximum.load(Ordering::SeqCst), 1);
    assert_eq!(counts.active.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn blocking_credentials_leave_the_single_async_thread_responsive() {
    let access = Arc::new(CredentialAccess::default());
    let async_thread = std::thread::current().id();

    for transaction in [false, true] {
        let access = Arc::clone(&access);
        let (release, released) = held_work();
        let (started, start) = oneshot::channel();
        let caller = tokio::spawn(async move {
            let work = move || {
                started
                    .send(std::thread::current().id())
                    .expect("start observer disappeared");
                wait_for_release(released);
                17
            };
            if transaction {
                access.transaction(WATCHDOG, work).await
            } else {
                access.read(WATCHDOG, work).await
            }
        });

        assert_ne!(
            bounded(start).await.expect("worker did not start"),
            async_thread
        );
        let heartbeat = tokio::spawn(async {
            tokio::task::yield_now().await;
            23
        });
        assert_eq!(bounded(heartbeat).await.expect("heartbeat panicked"), 23);
        assert!(!caller.is_finished());
        drop(release);
        assert_eq!(
            bounded(caller)
                .await
                .expect("caller panicked")
                .expect("credential work failed"),
            17
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn timed_out_read_holds_the_permit_and_expired_queue_work_never_runs() {
    let access = Arc::new(CredentialAccess::default());
    let counts = Arc::new(WorkCounts::default());
    let queued_calls = Arc::new(AtomicUsize::new(0));
    let (release, released) = held_work();
    let (started, start) = oneshot::channel();
    let (finished, finish) = oneshot::channel();
    let worker_access = Arc::clone(&access);
    let worker_counts = Arc::clone(&counts);
    let caller = tokio::spawn(async move {
        worker_access
            .read(READ_TIMEOUT, move || {
                {
                    let _active = worker_counts.enter();
                    started.send(()).expect("start observer disappeared");
                    wait_for_release(released);
                }
                let _ = finished.send(());
                31
            })
            .await
    });

    bounded(start).await.expect("worker did not start");
    assert!(matches!(
        bounded(caller).await.expect("caller panicked"),
        Err(CredentialAccessError::TimedOut)
    ));
    assert_eq!(counts.active.load(Ordering::SeqCst), 1);

    for transaction in [false, true] {
        let queued_calls = Arc::clone(&queued_calls);
        let work = move || queued_calls.fetch_add(1, Ordering::SeqCst);
        let result = if transaction {
            bounded(access.transaction(QUEUE_TIMEOUT, work)).await
        } else {
            bounded(access.read(QUEUE_TIMEOUT, work)).await
        };
        assert!(matches!(result, Err(CredentialAccessError::TimedOut)));
    }
    assert_eq!(queued_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.active.load(Ordering::SeqCst), 1);

    drop(release);
    bounded(finish).await.expect("worker did not finish");
    assert_eq!(
        bounded(access.read(WATCHDOG, || "fixture-ready"))
            .await
            .expect("read did not recover"),
        "fixture-ready"
    );
    bounded(access.transaction(WATCHDOG, || ()))
        .await
        .expect("transaction did not recover");
    assert_eq!(queued_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.active.load(Ordering::SeqCst), 0);
    assert_eq!(counts.maximum.load(Ordering::SeqCst), 1);
}

async fn cancellation_keeps_running_work_serialized(transaction: bool) {
    let access = Arc::new(CredentialAccess::default());
    let transaction_guard = Arc::new(Mutex::new(()));
    let counts = Arc::new(WorkCounts::default());
    let queued_calls = Arc::new(AtomicUsize::new(0));
    let (release, released) = held_work();
    let (started, start) = oneshot::channel();
    let (finished, finish) = oneshot::channel();
    let worker_access = Arc::clone(&access);
    let worker_guard = Arc::clone(&transaction_guard);
    let worker_counts = Arc::clone(&counts);
    let caller = tokio::spawn(async move {
        let work = move || {
            {
                let _active = worker_counts.enter();
                let _guard = worker_guard.lock().expect("fixture guard poisoned");
                started.send(()).expect("start observer disappeared");
                wait_for_release(released);
            }
            let _ = finished.send(());
            41
        };
        if transaction {
            worker_access.transaction(QUEUE_TIMEOUT, work).await
        } else {
            worker_access.read(WATCHDOG, work).await
        }
    });

    bounded(start).await.expect("worker did not start");
    caller.abort();
    assert!(bounded(caller)
        .await
        .expect_err("caller was not cancelled")
        .is_cancelled());
    assert!(matches!(
        transaction_guard.try_lock(),
        Err(TryLockError::WouldBlock)
    ));
    assert_eq!(counts.active.load(Ordering::SeqCst), 1);

    for queued_transaction in [false, true] {
        let queued_calls = Arc::clone(&queued_calls);
        let work = move || queued_calls.fetch_add(1, Ordering::SeqCst);
        let result = if queued_transaction {
            bounded(access.transaction(QUEUE_TIMEOUT, work)).await
        } else {
            bounded(access.read(QUEUE_TIMEOUT, work)).await
        };
        assert!(matches!(result, Err(CredentialAccessError::TimedOut)));
    }
    assert_eq!(queued_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        transaction_guard.try_lock(),
        Err(TryLockError::WouldBlock)
    ));

    drop(release);
    bounded(finish).await.expect("worker did not finish");
    bounded(access.transaction(WATCHDOG, || ()))
        .await
        .expect("transaction did not recover");
    assert!(transaction_guard.try_lock().is_ok());
    assert_eq!(queued_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.active.load(Ordering::SeqCst), 0);
    assert_eq!(counts.maximum.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_a_read_keeps_its_running_work_serialized() {
    cancellation_keeps_running_work_serialized(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_a_transaction_keeps_its_permit_and_transaction_guard() {
    cancellation_keeps_running_work_serialized(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn started_transaction_waits_past_queue_timeout_for_its_final_result() {
    let access = Arc::new(CredentialAccess::default());
    let (release, released) = held_work();
    let (started, start) = oneshot::channel();
    let worker_access = Arc::clone(&access);
    let caller = tokio::spawn(async move {
        worker_access
            .transaction(QUEUE_TIMEOUT, move || {
                started.send(()).expect("start observer disappeared");
                wait_for_release(released);
                Err::<(), _>("fixture-write-rejected")
            })
            .await
    });

    bounded(start).await.expect("worker did not start");
    // This queued read establishes that twice the transaction's queue budget
    // has elapsed while the started transaction is still waiting to finish.
    assert!(matches!(
        bounded(access.read(QUEUE_TIMEOUT * 2, || ())).await,
        Err(CredentialAccessError::TimedOut)
    ));
    assert!(
        !caller.is_finished(),
        "a started transaction returned before its work had a result"
    );
    drop(release);
    assert_eq!(
        bounded(caller)
            .await
            .expect("caller panicked")
            .expect("started transaction reported a timeout"),
        Err("fixture-write-rejected")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn worker_panics_release_the_permit_without_exposing_the_panic_message() {
    const PANIC_MARKER: &str = "fixture-worker-panic-marker";
    let access = CredentialAccess::default();

    for transaction in [false, true] {
        let work = || -> () { panic!("{PANIC_MARKER}") };
        let result = if transaction {
            bounded(access.transaction(WATCHDOG, work)).await
        } else {
            bounded(access.read(WATCHDOG, work)).await
        };
        let error = result.expect_err("worker panic was not reported");
        assert!(matches!(&error, CredentialAccessError::WorkerUnavailable));
        assert!(!format!("{error:?}").contains(PANIC_MARKER));
        assert!(!error.to_string().contains(PANIC_MARKER));

        assert!(bounded(access.read(WATCHDOG, || true))
            .await
            .expect("read did not recover after panic"));
        assert!(bounded(access.transaction(WATCHDOG, || true))
            .await
            .expect("transaction did not recover after panic"));
    }
}

#[test]
fn timed_out_queued_reads_do_not_fill_the_blocking_thread_pool() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(2)
        .build()
        .expect("fixture runtime failed");

    runtime.block_on(async {
        let access = Arc::new(CredentialAccess::default());
        let queued_calls = Arc::new(AtomicUsize::new(0));
        let (release, released) = held_work();
        let (started, start) = oneshot::channel();
        let worker_access = Arc::clone(&access);
        let caller = tokio::spawn(async move {
            worker_access
                .read(WATCHDOG, move || {
                    started.send(()).expect("start observer disappeared");
                    wait_for_release(released);
                })
                .await
        });
        bounded(start).await.expect("worker did not start");

        let mut queued = Vec::new();
        for _ in 0..8 {
            let access = Arc::clone(&access);
            let queued_calls = Arc::clone(&queued_calls);
            queued.push(tokio::spawn(async move {
                access
                    .read(QUEUE_TIMEOUT, move || {
                        queued_calls.fetch_add(1, Ordering::SeqCst);
                    })
                    .await
            }));
        }
        for queued in queued {
            assert!(matches!(
                bounded(queued).await.expect("queued caller panicked"),
                Err(CredentialAccessError::TimedOut)
            ));
        }

        let unrelated = tokio::task::spawn_blocking(|| 59);
        assert_eq!(
            tokio::time::timeout(READ_TIMEOUT, unrelated)
                .await
                .expect("queued credentials occupied the remaining blocking thread")
                .expect("unrelated blocking work panicked"),
            59
        );
        assert_eq!(queued_calls.load(Ordering::SeqCst), 0);
        drop(release);
        bounded(caller)
            .await
            .expect("caller panicked")
            .expect("active credential read failed");
        bounded(access.read(WATCHDOG, || ()))
            .await
            .expect("read did not recover");
        assert_eq!(queued_calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn cancelling_work_queued_in_the_blocking_pool_prevents_it_from_running() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .expect("fixture runtime failed");

    runtime.block_on(async {
        let access = CredentialAccess::default();
        for transaction in [false, true] {
            let queued_calls = Arc::new(AtomicUsize::new(0));
            let (release, released) = held_work();
            let (started, start) = oneshot::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                started.send(()).expect("start observer disappeared");
                wait_for_release(released);
            });
            bounded(start)
                .await
                .expect("blocking pool was not occupied");

            let worker_calls = Arc::clone(&queued_calls);
            let mut caller = Box::pin(async {
                let work = move || worker_calls.fetch_add(1, Ordering::SeqCst);
                if transaction {
                    access.transaction(WATCHDOG, work).await
                } else {
                    access.read(WATCHDOG, work).await
                }
            });
            // Poll once with the credential permit free and the only blocking
            // thread occupied, then cancel before the queued work can start.
            std::future::poll_fn(|context| match caller.as_mut().poll(context) {
                std::task::Poll::Pending => std::task::Poll::Ready(()),
                std::task::Poll::Ready(_) => panic!("queued caller completed early"),
            })
            .await;
            drop(caller);
            drop(release);
            bounded(blocker).await.expect("blocking fixture panicked");
            bounded(tokio::task::spawn_blocking(|| ()))
                .await
                .expect("blocking pool did not drain");
            assert_eq!(queued_calls.load(Ordering::SeqCst), 0);
            bounded(access.transaction(WATCHDOG, || ()))
                .await
                .expect("cancelled queued work did not release its permit");
        }
    });
}
