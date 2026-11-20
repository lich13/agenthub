use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct Replies(Mutex<Vec<(u64, bool, String)>>);

unsafe extern "C" fn reply(
    id: u64,
    ok: bool,
    ptr: *const c_char,
    len: usize,
    ctx: *mut std::ffi::c_void,
) {
    // SAFETY: the test retains Replies and the sink lends its UTF-8 payload during the callback.
    let replies = unsafe { &*ctx.cast::<Replies>() };
    let bytes = if len == 0 {
        &[]
    } else {
        // SAFETY: nonempty responses borrow exactly len bytes for this synchronous callback.
        unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) }
    };
    replies
        .0
        .lock()
        .unwrap()
        .push((id, ok, String::from_utf8(bytes.to_vec()).unwrap()));
}

#[tokio::test]
async fn rpc_completes_once_for_success_error_abort_and_unwind() {
    let replies = Replies::default();
    let sink = |id| {
        RpcSink::new(
            id,
            Some(reply),
            std::ptr::from_ref(&replies).cast_mut().cast(),
        )
    };
    sink(1).respond(Ok("{}".into()));
    sink(2).respond(Err("invalid request".into()));
    let cancelled = sink(3);
    let task = tokio::spawn(async move {
        std::future::pending::<()>().await;
        cancelled.respond(Ok("{}".into()));
    });
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let interrupted = sink(4);
    assert!(std::panic::catch_unwind(move || {
        let _owner = interrupted;
        panic!("fixture unwind");
    })
    .is_err());
    let result = replies.0.lock().unwrap();
    assert_eq!(
        result
            .iter()
            .map(|(id, ok, _)| (*id, *ok))
            .collect::<Vec<_>>(),
        [(1, true), (2, false), (3, false), (4, false)]
    );
    assert_eq!(result[0].2, "{}");
    assert_eq!(result[1].2, "invalid request");
    assert!(result[2..]
        .iter()
        .all(|reply| reply.2.contains("结果未确认")));
}

#[test]
fn bridge_cleanup_and_blocking_workers_share_two_seconds() {
    let (release, held) = std::sync::mpsc::channel::<()>();
    let (done, result) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.spawn_blocking(move || {
            let _ = held.recv();
        });
        stop_runtime(runtime, std::future::pending());
        done.send(()).ok();
    });
    let stopped = result.recv_timeout(Duration::from_millis(3500));
    drop(release);
    worker.join().unwrap();
    stopped.expect("runtime must use the time remaining after bridge cleanup");
}
