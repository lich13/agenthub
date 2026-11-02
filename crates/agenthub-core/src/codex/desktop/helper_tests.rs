use super::{catalog_request, read_bounded_line, CatalogQuery, DesktopHelper};
use crate::{codex::desktop::DesktopError, resource_limits::CODEX_DESKTOP_FRAME_BYTES};
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    pin::Pin,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, BufReader, ReadBuf};

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);
// These tests share the production helper semaphore. Keep each multi-step
// cancellation assertion isolated from other fixture clients taking its slot.
static HELPER_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct HelperFixture {
    directory: PathBuf,
    executable: PathBuf,
    trace: PathBuf,
}

#[derive(Clone, Copy, Debug)]
enum CreateReply {
    RpcError,
    Disconnected,
    Malformed,
    MissingThreadId,
    InvalidThreadId,
    ValidThreadId,
}

impl HelperFixture {
    fn new(hang_on_apps: bool) -> Self {
        Self::build(hang_on_apps, None)
    }

    fn for_create(reply: CreateReply) -> Self {
        Self::build(false, Some(reply))
    }

    fn build(hang_on_apps: bool, create_reply: Option<CreateReply>) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "agenthub-helper-test-{}-{}",
            std::process::id(),
            FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).expect("create isolated helper fixture directory");
        let executable = directory.join("fake-codex.sh");
        let trace = directory.join("fake-codex.sh.requests");
        let hang = if hang_on_apps { "true" } else { "false" };
        let create_case = match create_reply {
            Some(CreateReply::RpcError) => {
                r#"    *'"method":"thread/start"'*) printf '{"id":%s,"error":{"code":-32000,"message":"synthetic failure"}}\n' "$request_id" ;;"#
            }
            Some(CreateReply::Disconnected) => r#"    *'"method":"thread/start"'*) exit 0 ;;"#,
            Some(CreateReply::Malformed) => {
                r#"    *'"method":"thread/start"'*) printf 'not-json\n' ;;"#
            }
            Some(CreateReply::MissingThreadId) => {
                r#"    *'"method":"thread/start"'*) printf '{"id":%s,"result":{"thread":{}}}\n' "$request_id" ;;"#
            }
            Some(CreateReply::InvalidThreadId) => {
                r#"    *'"method":"thread/start"'*) printf '{"id":%s,"result":{"thread":{"id":"not-a-uuid"}}}\n' "$request_id" ;;"#
            }
            Some(CreateReply::ValidThreadId) => {
                r#"    *'"method":"thread/start"'*) printf '{"id":%s,"result":{"thread":{"id":"00112233-4455-6677-8899-aabbccddeeff"}}}\n' "$request_id" ;;"#
            }
            None => "",
        };
        let script = format!(
            r#"#!/bin/sh
page=0
while IFS= read -r line; do
  printf '%s\t%s\n' "$$" "$line" >> "$0.requests"
  request_id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"id":%s,"result":{{}}}}\n' "$request_id" ;;
    *'"method":"app/list"'*)
      if [ {hang} = true ]; then IFS= read -r held; exit 0; fi
      page=$((page + 1))
      if [ $page -eq 1 ]; then
        printf '{{"id":%s,"result":{{"data":[],"nextCursor":"fixture-next"}}}}\n' "$request_id"
      else
        printf '{{"id":%s,"result":{{"data":[],"nextCursor":null}}}}\n' "$request_id"
      fi ;;
{create_case}
  esac
done
"#,
            create_case = create_case,
        );
        fs::write(&executable, script).expect("write fake helper executable");
        let mut permissions = fs::metadata(&executable)
            .expect("read fake helper metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&executable, permissions).expect("make fake helper executable");
        fs::create_dir(directory.join("codex-home")).expect("create fixture Codex home");
        Self {
            directory,
            executable,
            trace,
        }
    }

    fn helper(&self) -> DesktopHelper {
        DesktopHelper {
            executable: self.executable.clone(),
            codex_home: self.directory.join("codex-home"),
        }
    }

    fn records(&self) -> Vec<(u32, serde_json::Value)> {
        fs::read_to_string(&self.trace)
            .expect("fake helper should record protocol requests")
            .lines()
            .map(|line| {
                let (pid, message) = line.split_once('\t').expect("trace contains PID and JSON");
                (
                    pid.parse().expect("helper PID is numeric"),
                    serde_json::from_str(message).expect("trace contains protocol JSON"),
                )
            })
            .collect()
    }
}

impl Drop for HelperFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

const GENERATED_CHUNK_BYTES: usize = 8 * 1024;

struct GeneratedReader {
    prefix: &'static [u8],
    prefix_offset: usize,
    fill_byte: u8,
    fill_remaining: usize,
    suffix: &'static [u8],
    suffix_offset: usize,
}

impl GeneratedReader {
    fn new(prefix: &'static [u8], fill_byte: u8, fill_len: usize, suffix: &'static [u8]) -> Self {
        Self {
            prefix,
            prefix_offset: 0,
            fill_byte,
            fill_remaining: fill_len,
            suffix,
            suffix_offset: 0,
        }
    }
}

impl AsyncRead for GeneratedReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        if this.prefix_offset < this.prefix.len() {
            let end = (this.prefix_offset + output.remaining()).min(this.prefix.len());
            output.put_slice(&this.prefix[this.prefix_offset..end]);
            this.prefix_offset = end;
        } else if this.fill_remaining > 0 {
            let length = output
                .remaining()
                .min(this.fill_remaining)
                .min(GENERATED_CHUNK_BYTES);
            let chunk = [this.fill_byte; GENERATED_CHUNK_BYTES];
            output.put_slice(&chunk[..length]);
            this.fill_remaining -= length;
        } else if this.suffix_offset < this.suffix.len() {
            let end = (this.suffix_offset + output.remaining()).min(this.suffix.len());
            output.put_slice(&this.suffix[this.suffix_offset..end]);
            this.suffix_offset = end;
        }

        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn read_bounded_line_rejects_invalid_json() {
    let mut reader = BufReader::new(std::io::Cursor::new(&b"{not json}\n"[..]));
    assert!(matches!(
        read_bounded_line(&mut reader).await,
        Err(DesktopError::InvalidFrame)
    ));
}

#[tokio::test]
async fn read_bounded_line_requires_a_complete_newline_terminated_frame() {
    let mut reader = BufReader::new(std::io::Cursor::new(&b"{\"id\":1}"[..]));
    assert!(matches!(
        read_bounded_line(&mut reader).await,
        Err(DesktopError::Disconnected)
    ));
}

#[tokio::test]
async fn read_bounded_line_accepts_exactly_the_64_mib_limit() {
    let fill_len = CODEX_DESKTOP_FRAME_BYTES - 3;
    let generated = GeneratedReader::new(b"{", b' ', fill_len, b"}\n");
    let mut reader = BufReader::new(generated);

    assert_eq!(
        read_bounded_line(&mut reader)
            .await
            .expect("exactly bounded JSON line is accepted"),
        serde_json::json!({})
    );
}

#[tokio::test]
async fn read_bounded_line_rejects_one_byte_over_the_64_mib_limit() {
    let generated = GeneratedReader::new(b"{", b' ', CODEX_DESKTOP_FRAME_BYTES, b"}\n");
    let mut reader = BufReader::new(generated);

    assert!(matches!(
        read_bounded_line(&mut reader).await,
        Err(DesktopError::FrameTooLarge)
    ));
}

#[test]
fn app_list_is_unbound_and_force_flags_are_method_specific() {
    let (method, params) = catalog_request(CatalogQuery::Apps { cursor: None }, true).unwrap();
    assert_eq!(method, "app/list");
    assert_eq!(params["cursor"], serde_json::Value::Null);
    assert_eq!(params["forceRefetch"], true);
    assert!(params["threadId"].is_null());
    assert!(params.get("thread_id").is_none());

    for query in [CatalogQuery::Models { cursor: None }, CatalogQuery::Modes] {
        let (_, params) = catalog_request(query, true).unwrap();
        assert!(params.get("forceRefetch").is_none());
        assert!(params.get("forceReload").is_none());
    }

    let (_, skills) = catalog_request(
        CatalogQuery::Skills {
            cwd: PathBuf::from("/fixture/workspace"),
        },
        true,
    )
    .unwrap();
    assert_eq!(skills["forceReload"], true);
    assert!(skills.get("forceRefetch").is_none());

    let (_, plugins) = catalog_request(
        CatalogQuery::Plugins {
            cwd: PathBuf::from("/fixture/workspace"),
        },
        true,
    )
    .unwrap();
    assert_eq!(plugins["forceRefetch"], true);
    assert!(plugins.get("forceReload").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn app_catalog_pages_use_one_helper_and_one_process() {
    let _test = HELPER_TEST.lock().await;
    let fixture = HelperFixture::new(false);
    let helper = fixture.helper();
    let mut reader = helper.catalog_reader(true).await.unwrap();
    let first = reader
        .catalog(CatalogQuery::Apps { cursor: None }, true)
        .await
        .unwrap();
    assert_eq!(first["nextCursor"], "fixture-next");
    let second = reader
        .catalog(
            CatalogQuery::Apps {
                cursor: first["nextCursor"].as_str().map(str::to_owned),
            },
            false,
        )
        .await
        .unwrap();
    assert!(second["nextCursor"].is_null());
    reader.finish().await;

    let records = fixture.records();
    let pages: Vec<_> = records
        .iter()
        .filter(|(_, message)| message["method"] == "app/list")
        .collect();
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages[0].0, pages[1].0,
        "all pages use the same helper process"
    );
    assert_eq!(pages[0].1["id"], 2);
    assert_eq!(pages[1].1["id"], 3);
    assert_eq!(pages[0].1["params"]["cursor"], serde_json::Value::Null);
    assert_eq!(pages[0].1["params"]["forceRefetch"], true);
    assert_eq!(pages[1].1["params"]["cursor"], "fixture-next");
    assert_eq!(pages[1].1["params"]["forceRefetch"], false);
}

#[tokio::test(flavor = "current_thread")]
async fn apps_budget_allows_a_page_after_simulated_thirty_two_seconds() {
    let _test = HELPER_TEST.lock().await;
    let apps_fixture = HelperFixture::new(false);
    let mut apps_reader = apps_fixture.helper().catalog_reader(true).await.unwrap();
    apps_reader.deadline -= Duration::from_secs(32);
    let page = apps_reader
        .catalog(CatalogQuery::Apps { cursor: None }, false)
        .await
        .unwrap();
    assert_eq!(page["nextCursor"], "fixture-next");
    apps_reader.finish().await;

    let regular_fixture = HelperFixture::new(false);
    let mut regular_reader = regular_fixture
        .helper()
        .catalog_reader(false)
        .await
        .unwrap();
    regular_reader.deadline -= Duration::from_secs(32);
    assert!(matches!(
        regular_reader
            .catalog(CatalogQuery::Apps { cursor: None }, false)
            .await,
        Err(DesktopError::Timeout)
    ));
    regular_reader.finish().await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_catalog_read_kills_its_helper_and_releases_the_permit() {
    let _test = HELPER_TEST.lock().await;
    let blocked = HelperFixture::new(true);
    let blocked_helper = blocked.helper();
    let read = tokio::spawn(async move {
        let mut reader = blocked_helper.catalog_reader(true).await?;
        reader
            .catalog(CatalogQuery::Apps { cursor: None }, false)
            .await
    });

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if fs::read_to_string(&blocked.trace)
                .is_ok_and(|trace| trace.contains("\"method\":\"app/list\""))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fake helper should reach the blocked page request");
    let helper_pid = blocked.records()[1].0;

    read.abort();
    let _ = read.await;

    let next = HelperFixture::new(false);
    let mut reader =
        tokio::time::timeout(Duration::from_secs(2), next.helper().catalog_reader(false))
            .await
            .expect("cancellation should promptly release the helper permit")
            .unwrap();
    reader
        .catalog(CatalogQuery::Apps { cursor: None }, false)
        .await
        .unwrap();
    reader.finish().await;

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = Command::new("/bin/ps")
                .args(["-o", "stat=", "-p", &helper_pid.to_string()])
                .output()
                .expect("ps is available for checking the fixture child");
            let status = String::from_utf8_lossy(&status.stdout);
            if status.trim().is_empty() || status.trim_start().starts_with('Z') {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancelled helper process should be stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn create_empty_reports_unknown_after_any_committed_error_or_bad_thread_id() {
    let _test = HELPER_TEST.lock().await;
    for reply in [
        CreateReply::RpcError,
        CreateReply::Disconnected,
        CreateReply::Malformed,
        CreateReply::MissingThreadId,
        CreateReply::InvalidThreadId,
    ] {
        let fixture = HelperFixture::for_create(reply);
        let result = fixture
            .helper()
            .create_empty(Path::new("/fixture/workspace"))
            .await;

        assert!(
            matches!(result, Err(DesktopError::OutcomeUnknown)),
            "{reply:?} after thread/start must not look retryable"
        );
        let thread_starts = fixture
            .records()
            .iter()
            .filter(|(_, message)| message["method"] == "thread/start")
            .count();
        assert_eq!(thread_starts, 1, "one create attempt is submitted");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn create_empty_returns_only_a_verified_thread_uuid() {
    let _test = HELPER_TEST.lock().await;
    let fixture = HelperFixture::for_create(CreateReply::ValidThreadId);

    let result = fixture
        .helper()
        .create_empty(Path::new("/fixture/workspace"))
        .await
        .expect("valid synthetic thread id is returned");

    assert_eq!(result, "00112233-4455-6677-8899-aabbccddeeff");
    assert_eq!(
        fixture
            .records()
            .iter()
            .filter(|(_, message)| message["method"] == "thread/start")
            .count(),
        1
    );
}
