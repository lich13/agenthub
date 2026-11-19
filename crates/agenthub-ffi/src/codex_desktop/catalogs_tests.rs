use super::{
    hash_token, limit, next, plugin_item, CatalogCache, Catalogs, Item, Snapshot, CACHE_AGE,
    MAX_BYTES, MAX_ENTRIES, MAX_ITEMS, PAGE_SIZE,
};
use agenthub_core::services::codex_desktop::{DesktopCatalogRequest, DesktopMode};
use serde_json::json;
use std::{
    collections::HashSet,
    future::{poll_fn, Future},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::Poll,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Notify};

fn model_item(index: usize, name: &str, description: &str) -> Item {
    Item {
        id: format!("model-{index}"),
        kind: "model".into(),
        name: name.into(),
        description: description.into(),
        efforts: vec!["low".into(), "high".into()],
        default_effort: Some("low".into()),
        grant: None,
    }
}

fn snapshot(items: Vec<Item>, modes: &[&str]) -> Snapshot {
    let bytes = items.iter().map(Item::bytes).sum();
    Snapshot {
        key: "fixture-key".into(),
        revision: 9,
        thread: "fixture-thread".into(),
        kind: "models".into(),
        nonce: "fixture-nonce".into(),
        created: Instant::now(),
        items,
        modes: modes.iter().map(|mode| (*mode).into()).collect(),
        bytes,
    }
}

fn request(query: &str, cursor: Option<String>) -> DesktopCatalogRequest {
    DesktopCatalogRequest {
        force_refetch: None,
        revision: 9,
        session_id: None,
        project_handle: None,
        kind: "models".into(),
        query: query.into(),
        cursor,
    }
}

fn cursor(snapshot: &Snapshot, query: &str, offset: usize) -> String {
    let query = query.trim().to_lowercase();
    let stamp = hash_token(&format!("{}\n{query}", snapshot.nonce));
    format!("{stamp}.{offset}")
}

async fn poll_once<F: Future>(future: F) -> Poll<F::Output> {
    let mut future = Box::pin(future);
    poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
}

async fn observe_first_poll<F: Future>(future: F, observed: oneshot::Sender<bool>) -> F::Output {
    let mut future = Box::pin(future);
    let mut observed = Some(observed);
    poll_fn(move |cx| {
        let result = future.as_mut().poll(cx);
        if let Some(observed) = observed.take() {
            let _ = observed.send(result.is_pending());
        }
        result
    })
    .await
}

#[test]
fn page_cursor_is_query_bound_and_query_normalization_is_stable() {
    let items = (0..205)
        .map(|index| model_item(index, &format!("fixture item {index}"), ""))
        .collect();
    let snapshot = snapshot(items, &[]);
    let first = snapshot.page(&request("fixture", None)).unwrap();
    assert_eq!(first.items.len(), PAGE_SIZE);
    let next_cursor = first.next_cursor.unwrap();

    let second = snapshot
        .page(&request(" Fixture ", Some(next_cursor.clone())))
        .unwrap();
    assert_eq!(second.items.len(), PAGE_SIZE);
    assert_eq!(second.items[0].name, "fixture item 100");
    assert!(snapshot.page(&request("other", Some(next_cursor))).is_err());
}

#[test]
fn page_cursor_fails_after_snapshot_replacement() {
    let items: Vec<_> = (0..101)
        .map(|index| model_item(index, &format!("fixture item {index}"), ""))
        .collect();
    let original = snapshot(items.clone(), &[]);
    let stale_cursor = original
        .page(&request("fixture", None))
        .unwrap()
        .next_cursor
        .unwrap();
    let mut replacement = snapshot(items, &[]);
    replacement.nonce = "replacement-nonce".into();

    assert!(replacement
        .page(&request("fixture", Some(stale_cursor)))
        .is_err());
}

#[test]
fn page_rejects_malformed_misaligned_and_filtered_out_offsets() {
    let mut items: Vec<_> = (0..150)
        .map(|index| model_item(index, &format!("match item {index}"), ""))
        .collect();
    items.extend((150..205).map(|index| model_item(index, &format!("other item {index}"), "")));
    let snapshot = snapshot(items, &[]);
    let valid_stamp = cursor(&snapshot, "match", 0)
        .strip_suffix(".0")
        .unwrap()
        .to_owned();

    for invalid in [
        "malformed".to_owned(),
        format!("{valid_stamp}.not-a-number"),
        format!("{valid_stamp}.1"),
        format!("{valid_stamp}.300"),
        format!("{valid_stamp}.200"),
    ] {
        assert!(snapshot.page(&request("match", Some(invalid))).is_err());
    }
}

#[test]
fn model_settings_require_current_catalog_identity_and_allowlisted_choices() {
    let mut item = model_item(1, "Fixture model", "");
    item.efforts = vec![
        "low".into(),
        "high".into(),
        "max".into(),
        "ultra".into(),
        "fixture-custom".into(),
    ];
    let mut cache = CatalogCache::default();
    cache.entries.push(snapshot(vec![item], &["plan"]));
    let mut modes = snapshot(vec![], &["plan"]);
    modes.kind = "modes".into();
    cache.entries.push(modes);

    assert!(cache
        .validate_settings(
            9,
            "fixture-thread",
            "model-1",
            Some("high"),
            DesktopMode::Plan
        )
        .is_ok());
    assert!(cache
        .validate_settings(
            10,
            "fixture-thread",
            "model-1",
            Some("high"),
            DesktopMode::Plan,
        )
        .is_err());
    assert!(cache
        .validate_settings(9, "other-thread", "model-1", None, DesktopMode::Plan)
        .is_err());
    assert!(cache
        .validate_settings(
            9,
            "fixture-thread",
            "unlisted-model",
            None,
            DesktopMode::Plan
        )
        .is_err());
    assert!(cache
        .validate_settings(
            9,
            "fixture-thread",
            "model-1",
            Some("unlisted-effort"),
            DesktopMode::Plan,
        )
        .is_err());
    for effort in ["max", "ultra", "fixture-custom"] {
        assert!(cache
            .validate_settings(
                9,
                "fixture-thread",
                "model-1",
                Some(effort),
                DesktopMode::Plan,
            )
            .is_ok());
    }
    assert!(cache
        .validate_settings(9, "fixture-thread", "model-1", None, DesktopMode::Default)
        .is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn force_refetch_bypasses_cache_and_rejects_a_cursor() {
    let catalogs = Catalogs::default();
    let cached_request = request("", None);
    catalogs
        .cache
        .lock()
        .await
        .insert(
            "fixture-thread",
            &cached_request,
            vec![model_item(1, "cached model", "")],
            vec![],
        )
        .unwrap();

    let mut forced_request = request("", None);
    forced_request.force_refetch = Some(true);
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let forced_count = Arc::clone(&fetch_count);
    let page = catalogs
        .read_with("fixture-thread", &forced_request, || Ok(()), async move {
            forced_count.fetch_add(1, Ordering::SeqCst);
            Ok((vec![model_item(2, "refetched model", "")], vec![]))
        })
        .await
        .unwrap();
    assert_eq!(page.items[0].id, "model-2");
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);

    let mut cursor_request = forced_request;
    cursor_request.cursor = Some("fixture-cursor".into());
    let rejected = catalogs
        .read_with("fixture-thread", &cursor_request, || Ok(()), async {
            panic!("forced refresh with a cursor must not fetch")
        })
        .await;
    assert!(matches!(rejected, Err(error) if error.contains("首页")));
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_modes_read_keeps_the_model_catalog_cached() {
    let catalogs = Catalogs::default();
    let models_request = request("", None);
    catalogs
        .read_with("fixture-thread", &models_request, || Ok(()), async {
            Ok((vec![model_item(1, "fixture model", "")], vec![]))
        })
        .await
        .unwrap();

    let mut modes_request = request("", None);
    modes_request.kind = "modes".into();
    let failed = catalogs
        .read_with("fixture-thread", &modes_request, || Ok(()), async {
            Err("fixture modes failure".into())
        })
        .await;
    assert!(matches!(failed, Err(error) if error == "fixture modes failure"));

    let mut cache = catalogs.cache.lock().await;
    let models = cache
        .cached("fixture-thread", &models_request)
        .unwrap()
        .expect("model snapshot survives a separate modes failure");
    assert_eq!(models.items[0].id, "model-1");
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.entries[0].kind, "models");
}

#[test]
fn plugin_catalog_uses_native_name_and_keeps_friendly_title_as_description() {
    let fixture = json!({
        "id": "fixture-plugin-id",
        "name": "canonical-plugin",
        "interface": {
            "displayName": "Friendly Plugin",
            "shortDescription": "fixture summary"
        }
    });

    let item = plugin_item(&fixture, "fixture-marketplace").unwrap();
    assert_eq!(item.id, "fixture-plugin-id");
    assert_eq!(item.name, "canonical-plugin");
    assert_eq!(item.description, "Friendly Plugin · fixture summary");

    let reference = match item.grant.as_ref() {
        Some(super::GrantValue::Reference(reference)) => reference,
        _ => panic!("plugin catalog item should retain a native reference"),
    };
    let projected = serde_json::to_value(reference).unwrap();
    assert_eq!(projected["kind"], "plugin");
    assert_eq!(projected["name"], "canonical-plugin");
    assert_eq!(
        projected["path"],
        "plugin://canonical-plugin@fixture-marketplace"
    );
}

#[test]
fn expired_model_catalog_cannot_authorize_settings() {
    let mut cache = CatalogCache::default();
    let mut entry = snapshot(vec![model_item(1, "Fixture model", "")], &["default"]);
    entry.created = Instant::now() - CACHE_AGE - Duration::from_secs(1);
    cache.entries.push(entry);

    assert!(cache
        .validate_settings(9, "fixture-thread", "model-1", None, DesktopMode::Default)
        .is_err());
}

#[test]
fn expired_snapshots_are_removed_on_lookup() {
    let mut cache = CatalogCache::default();
    let mut expired = snapshot(vec![model_item(1, "fixture model", "")], &[]);
    expired.created = Instant::now() - CACHE_AGE - Duration::from_secs(1);
    cache.entries.push(expired);

    assert!(cache
        .cached("fixture-thread", &request("", None))
        .unwrap()
        .is_none());
    assert!(cache.entries.is_empty());
}

#[test]
fn snapshot_count_eviction_keeps_only_the_newest_entry_limit() {
    let mut cache = CatalogCache::default();
    let request = request("", None);

    for index in 0..MAX_ENTRIES {
        cache
            .insert(
                &format!("fixture-thread-{index}"),
                &request,
                vec![model_item(index, "fixture", "")],
                vec![],
            )
            .unwrap();
    }
    cache
        .insert(
            "fixture-thread-newest",
            &request,
            vec![model_item(MAX_ENTRIES, "fixture", "")],
            vec![],
        )
        .unwrap();

    assert_eq!(cache.entries.len(), MAX_ENTRIES);
    assert!(cache
        .cached("fixture-thread-0", &request)
        .unwrap()
        .is_none());
    assert!(cache
        .cached("fixture-thread-1", &request)
        .unwrap()
        .is_some());
    assert!(cache
        .cached("fixture-thread-newest", &request)
        .unwrap()
        .is_some());
}

#[test]
fn byte_budget_eviction_removes_oldest_snapshot_before_inserting() {
    let mut cache = CatalogCache::default();
    let request = request("", None);
    let make_large_item = |index: usize, value: &str| {
        let mut item = model_item(index, "fixture", "");
        item.description = value.repeat(MAX_BYTES / 2);
        item
    };
    let first = make_large_item(1, "a");
    let second = make_large_item(2, "b");
    assert!(first.bytes() < MAX_BYTES);
    assert!(second.bytes() < MAX_BYTES);
    assert!(first.bytes() + second.bytes() > MAX_BYTES);

    cache
        .insert("fixture-thread-old", &request, vec![first], vec![])
        .unwrap();
    cache
        .insert("fixture-thread-new", &request, vec![second], vec![])
        .unwrap();

    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.entries[0].thread, "fixture-thread-new");
    assert!(cache
        .cached("fixture-thread-old", &request)
        .unwrap()
        .is_none());
    assert!(cache
        .cached("fixture-thread-new", &request)
        .unwrap()
        .is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn cache_hit_for_one_key_completes_while_another_key_fetch_is_blocked() {
    let catalogs = Arc::new(Catalogs::default());
    let cached_request = request("", None);
    catalogs
        .cache
        .lock()
        .await
        .insert(
            "warm-thread",
            &cached_request,
            vec![model_item(1, "warm fixture", "")],
            vec![],
        )
        .unwrap();

    let fetch_started = Arc::new(Notify::new());
    let release_fetch = Arc::new(Notify::new());
    let slow_catalogs = Arc::clone(&catalogs);
    let slow_started = Arc::clone(&fetch_started);
    let slow_release = Arc::clone(&release_fetch);
    let slow_request = request("", None);
    let slow_read = tokio::spawn(async move {
        slow_catalogs
            .read_with("slow-thread", &slow_request, || Ok(()), async move {
                slow_started.notify_one();
                slow_release.notified().await;
                Ok((vec![model_item(2, "slow fixture", "")], vec![]))
            })
            .await
    });
    fetch_started.notified().await;

    let hit = poll_once(
        catalogs.read_with("warm-thread", &cached_request, || Ok(()), async {
            Err("unexpected cache miss".into())
        }),
    )
    .await;
    release_fetch.notify_one();
    let slow_result = tokio::time::timeout(Duration::from_secs(2), slow_read)
        .await
        .expect("slow fixture fetch should finish after release")
        .expect("slow reader task should not panic");
    assert!(slow_result.is_ok());

    match hit {
        Poll::Ready(Ok(page)) => assert_eq!(page.items[0].id, "model-1"),
        Poll::Ready(Err(error)) => panic!("warm cache read failed: {error}"),
        Poll::Pending => panic!("warm cache hit waited for a different key's fetch"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn same_key_reads_share_one_successful_fetch() {
    let catalogs = Arc::new(Catalogs::default());
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let fetch_started = Arc::new(Notify::new());
    let release_fetch = Arc::new(Notify::new());

    let first_catalogs = Arc::clone(&catalogs);
    let first_count = Arc::clone(&fetch_count);
    let first_started = Arc::clone(&fetch_started);
    let first_release = Arc::clone(&release_fetch);
    let first = tokio::spawn(async move {
        first_catalogs
            .read_with(
                "fixture-thread",
                &request("", None),
                || Ok(()),
                async move {
                    first_count.fetch_add(1, Ordering::SeqCst);
                    first_started.notify_one();
                    first_release.notified().await;
                    Ok((vec![model_item(1, "shared fixture", "")], vec![]))
                },
            )
            .await
    });
    fetch_started.notified().await;

    let second_catalogs = Arc::clone(&catalogs);
    let second_count = Arc::clone(&fetch_count);
    let (observed_tx, observed_rx) = oneshot::channel();
    let second = tokio::spawn(async move {
        let request = request("", None);
        observe_first_poll(
            second_catalogs.read_with("fixture-thread", &request, || Ok(()), async move {
                second_count.fetch_add(1, Ordering::SeqCst);
                Ok((vec![model_item(2, "unexpected fixture", "")], vec![]))
            }),
            observed_tx,
        )
        .await
    });
    assert!(
        observed_rx.await.expect("second reader should be polled"),
        "same-key reader should wait for the active fetch"
    );
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);

    release_fetch.notify_one();
    let first = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .expect("first same-key read should finish after release")
        .expect("first reader task should not panic")
        .expect("shared catalog fetch should succeed");
    let second = tokio::time::timeout(Duration::from_secs(2), second)
        .await
        .expect("second same-key read should finish after release")
        .expect("second reader task should not panic")
        .expect("shared catalog fetch should succeed");

    assert_eq!(first.items[0].id, "model-1");
    assert_eq!(second.items[0].id, "model-1");
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn same_key_readers_share_failure_but_a_later_read_retries() {
    let catalogs = Arc::new(Catalogs::default());
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let fetch_started = Arc::new(Notify::new());
    let release_fetch = Arc::new(Notify::new());

    let first_catalogs = Arc::clone(&catalogs);
    let first_count = Arc::clone(&fetch_count);
    let first_started = Arc::clone(&fetch_started);
    let first_release = Arc::clone(&release_fetch);
    let first = tokio::spawn(async move {
        first_catalogs
            .read_with(
                "fixture-thread",
                &request("", None),
                || Ok(()),
                async move {
                    first_count.fetch_add(1, Ordering::SeqCst);
                    first_started.notify_one();
                    first_release.notified().await;
                    Err("fixture fetch failed".into())
                },
            )
            .await
    });
    fetch_started.notified().await;

    let second_catalogs = Arc::clone(&catalogs);
    let second_count = Arc::clone(&fetch_count);
    let (observed_tx, observed_rx) = oneshot::channel();
    let second = tokio::spawn(async move {
        let request = request("", None);
        observe_first_poll(
            second_catalogs.read_with("fixture-thread", &request, || Ok(()), async move {
                second_count.fetch_add(1, Ordering::SeqCst);
                Err("unexpected second fetch".into())
            }),
            observed_tx,
        )
        .await
    });
    assert!(
        observed_rx.await.expect("second reader should be polled"),
        "same-key reader should wait for the active failing fetch"
    );
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);

    release_fetch.notify_one();
    let first = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .expect("first failing read should finish after release")
        .expect("first reader task should not panic");
    let second = tokio::time::timeout(Duration::from_secs(2), second)
        .await
        .expect("second failing read should finish after release")
        .expect("second reader task should not panic");
    assert!(matches!(first, Err(error) if error == "fixture fetch failed"));
    assert!(matches!(second, Err(error) if error == "fixture fetch failed"));

    let retry_count = Arc::clone(&fetch_count);
    let retry = catalogs
        .read_with(
            "fixture-thread",
            &request("", None),
            || Ok(()),
            async move {
                retry_count.fetch_add(1, Ordering::SeqCst);
                Ok((vec![model_item(3, "retry fixture", "")], vec![]))
            },
        )
        .await;
    assert!(retry.is_ok());
    assert_eq!(fetch_count.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn waiting_reader_can_initialize_after_the_first_call_is_cancelled() {
    let catalogs = Arc::new(Catalogs::default());
    let first_started = Arc::new(Notify::new());
    let first_release = Arc::new(Notify::new());

    let first_catalogs = Arc::clone(&catalogs);
    let first_fetch_started = Arc::clone(&first_started);
    let first_fetch_release = Arc::clone(&first_release);
    let first = tokio::spawn(async move {
        first_catalogs
            .read_with(
                "fixture-thread",
                &request("", None),
                || Ok(()),
                async move {
                    first_fetch_started.notify_one();
                    first_fetch_release.notified().await;
                    Ok((vec![model_item(1, "cancelled fixture", "")], vec![]))
                },
            )
            .await
    });
    first_started.notified().await;

    let second_catalogs = Arc::clone(&catalogs);
    let (observed_tx, observed_rx) = oneshot::channel();
    let second = tokio::spawn(async move {
        let request = request("", None);
        observe_first_poll(
            second_catalogs.read_with("fixture-thread", &request, || Ok(()), async {
                Ok((vec![model_item(2, "waiting fixture", "")], vec![]))
            }),
            observed_tx,
        )
        .await
    });
    assert!(
        observed_rx.await.expect("waiting reader should be polled"),
        "second reader should be waiting on the first initialization"
    );

    first.abort();
    let cancellation = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .expect("cancelled first reader should stop");
    let cancellation = match cancellation {
        Err(error) => error,
        Ok(_) => panic!("first reader should be cancelled"),
    };
    assert!(cancellation.is_cancelled());

    let second = tokio::time::timeout(Duration::from_secs(2), second)
        .await
        .expect("waiting reader should initialize after cancellation")
        .expect("waiting reader task should not panic")
        .expect("waiting reader's fetch should succeed");
    assert_eq!(second.items[0].id, "model-2");
}

#[tokio::test(flavor = "current_thread")]
async fn late_fetch_is_not_cached_after_current_identity_changes() {
    let catalogs = Arc::new(Catalogs::default());
    let current = Arc::new(AtomicBool::new(true));
    let fetch_started = Arc::new(Notify::new());
    let release_fetch = Arc::new(Notify::new());
    let read_catalogs = Arc::clone(&catalogs);
    let checked_current = Arc::clone(&current);
    let started = Arc::clone(&fetch_started);
    let release = Arc::clone(&release_fetch);
    let read = tokio::spawn(async move {
        read_catalogs
            .read_with(
                "fixture-thread",
                &request("", None),
                move || {
                    if checked_current.load(Ordering::SeqCst) {
                        Ok(())
                    } else {
                        Err("fixture identity changed".into())
                    }
                },
                async move {
                    started.notify_one();
                    release.notified().await;
                    Ok((vec![model_item(1, "late fixture", "")], vec![]))
                },
            )
            .await
    });
    fetch_started.notified().await;
    current.store(false, Ordering::SeqCst);
    release_fetch.notify_one();

    let result = tokio::time::timeout(Duration::from_secs(2), read)
        .await
        .expect("late read should finish after fetch release")
        .expect("reader task should not panic");
    assert!(matches!(result, Err(error) if error == "fixture identity changed"));
    assert!(catalogs.cache.lock().await.entries.is_empty());
    current.store(true, Ordering::SeqCst);
    let retry_count = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&retry_count);
    let retry = catalogs
        .read_with(
            "fixture-thread",
            &request("", None),
            || Ok(()),
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok((vec![model_item(2, "fresh fixture", "")], vec![]))
            },
        )
        .await;
    assert!(retry.is_ok());
    assert_eq!(retry_count.load(Ordering::SeqCst), 1);
}

#[test]
fn catalog_item_and_byte_budgets_are_enforced() {
    let too_many: Vec<_> = (0..=MAX_ITEMS)
        .map(|index| model_item(index, "fixture", ""))
        .collect();
    assert!(limit(&too_many).is_err());

    let too_large = model_item(1, "fixture", &"x".repeat(MAX_BYTES));
    assert!(limit(&[too_large]).is_err());
}

#[test]
fn helper_pagination_rejects_duplicate_invalid_and_over_budget_cursors() {
    let mut seen = HashSet::new();
    assert_eq!(next(&json!({}), &mut seen).unwrap(), None);
    assert_eq!(
        next(&json!({"nextCursor":"fixture-cursor"}), &mut seen).unwrap(),
        Some("fixture-cursor".into())
    );
    assert!(next(&json!({"nextCursor":"fixture-cursor"}), &mut seen).is_err());
    assert!(next(&json!({"nextCursor":""}), &mut seen).is_err());
    assert!(next(&json!({"nextCursor":42}), &mut seen).is_err());
    assert!(next(&json!({"nextCursor": "x".repeat(4097)}), &mut seen).is_err());

    let mut bounded = HashSet::new();
    for index in 0..200 {
        let value = format!("cursor-{index}");
        assert_eq!(
            next(&json!({"nextCursor":value}), &mut bounded).unwrap(),
            Some(value)
        );
    }
    assert!(next(&json!({"nextCursor":"cursor-over-budget"}), &mut bounded).is_err());
}

#[tokio::test(start_paused = true)]
async fn invalidated_selection_drops_pending_fetch_and_never_caches_it() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let catalogs = Catalogs::default();
    let current = Arc::new(AtomicBool::new(true));
    let dropped = Arc::new(AtomicBool::new(false));
    let changed = current.clone();
    let guard = Dropped(dropped.clone());
    let started = tokio::time::Instant::now();
    let result = catalogs
        .read_with(
            "fixture-thread",
            &request("", None),
            || {
                if current.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err("selection changed fixture".into())
                }
            },
            async move {
                let _guard = guard;
                changed.store(false, Ordering::SeqCst);
                std::future::pending::<Result<(Vec<Item>, Vec<String>), String>>().await
            },
        )
        .await;
    assert!(matches!(result, Err(error) if error == "selection changed fixture"));
    assert!(dropped.load(Ordering::SeqCst));
    assert!(tokio::time::Instant::now() - started <= Duration::from_millis(100));
    assert!(catalogs.cache.lock().await.entries.is_empty());
}

#[tokio::test]
async fn forced_refresh_still_rejects_invalid_context_before_fetch() {
    let catalogs = Catalogs::default();
    let mut request = request("", None);
    request.force_refetch = Some(true);
    request.project_handle = Some("unselected-fixture".into());
    let fetched = AtomicBool::new(false);
    let result = catalogs
        .read_with("fixture-thread", &request, || Ok(()), async {
            fetched.store(true, Ordering::SeqCst);
            Ok((vec![], vec![]))
        })
        .await;
    assert!(result.is_err());
    assert!(!fetched.load(Ordering::SeqCst));
}
