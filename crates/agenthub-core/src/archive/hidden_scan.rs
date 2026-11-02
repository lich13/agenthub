//! Ephemeral, bounded cleanup scans. No native files or persistent jobs are written.
use super::{HiddenThreadCandidate, HiddenThreadDeletePlan};
use crate::{codex::CodexPaths, scan_work::ScanWork};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex,
    },
    time::Instant,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HiddenScanMode {
    Start,
    Poll,
    Cancel,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HiddenScanRequest {
    pub mode: Option<HiddenScanMode>,
    pub scan_id: Option<String>,
    pub cursor: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HiddenScanPhase {
    Preparing,
    Scanning,
    Complete,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenScanProgress {
    pub id: String,
    pub phase: HiddenScanPhase,
    pub processed: usize,
    pub total: u64,
    pub cursor: usize,
    pub next_cursor: usize,
    pub has_more: bool,
    pub error: Option<String>,
}

struct JobState {
    plan: HiddenThreadDeletePlan,
    items: Vec<HiddenThreadCandidate>,
    phase: HiddenScanPhase,
    error: Option<String>,
}

struct ScanJob {
    id: String,
    source: PathBuf,
    owner: String,
    created: Instant,
    page_size: usize,
    _slot: OwnedSemaphorePermit,
    work: ScanWork,
    finished: AtomicBool,
    state: Mutex<JobState>,
}

static JOBS: LazyLock<Mutex<HashMap<String, Arc<ScanJob>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SLOTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));
static ACTIVE_SOURCES: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

struct SourceLease(PathBuf);
impl SourceLease {
    fn acquire(source: PathBuf) -> Result<Self> {
        ensure!(
            ACTIVE_SOURCES
                .lock()
                .map_err(|_| anyhow::anyhow!("扫描服务不可用"))?
                .insert(source.clone()),
            "该来源已有扫描正在进行或停止，请稍后重试"
        );
        Ok(Self(source))
    }
}
impl Drop for SourceLease {
    fn drop(&mut self) {
        if let Ok(mut sources) = ACTIVE_SOURCES.lock() {
            sources.remove(&self.0);
        }
    }
}

fn empty_plan() -> HiddenThreadDeletePlan {
    HiddenThreadDeletePlan {
        scan: None,
        total_threads: 0,
        visible_threads: 0,
        hidden_threads: 0,
        archived_threads: 0,
        session_index_lines: 0,
        rollout_files: 0,
        hidden_ids: Vec::new(),
        candidates: Vec::new(),
        source_counts: Default::default(),
        integrity: "pending".into(),
    }
}

fn metadata(plan: &HiddenThreadDeletePlan) -> HiddenThreadDeletePlan {
    HiddenThreadDeletePlan {
        scan: None,
        total_threads: plan.total_threads,
        visible_threads: plan.visible_threads,
        hidden_threads: plan.hidden_threads,
        archived_threads: plan.archived_threads,
        session_index_lines: plan.session_index_lines,
        rollout_files: plan.rollout_files,
        hidden_ids: Vec::new(),
        candidates: Vec::new(),
        source_counts: plan.source_counts.clone(),
        integrity: plan.integrity.clone(),
    }
}

impl ScanJob {
    fn response(&self, cursor: usize, limit: usize) -> Result<HiddenThreadDeletePlan> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("扫描状态不可用"))?;
        ensure!(cursor <= state.items.len(), "扫描游标无效，请重新扫描");
        let terminal = matches!(
            state.phase,
            HiddenScanPhase::Complete | HiddenScanPhase::Stopped | HiddenScanPhase::Failed
        );
        let available = state.items.len() - cursor;
        // Publish full pages while scanning; once published their contents never change.
        let count = if terminal || available >= limit {
            available.min(limit)
        } else {
            0
        };
        let end = cursor + count;
        let mut plan = metadata(&state.plan);
        plan.candidates = state.items[cursor..end].to_vec();
        plan.hidden_ids = plan.candidates.iter().map(|item| item.id.clone()).collect();
        plan.scan = Some(HiddenScanProgress {
            id: self.id.clone(),
            phase: state.phase,
            processed: state.items.len(),
            total: state.plan.hidden_threads,
            cursor,
            next_cursor: end,
            has_more: state.items.len() - end >= limit || (terminal && end < state.items.len()),
            error: state.error.clone(),
        });
        Ok(plan)
    }
}

pub async fn request(
    paths: CodexPaths,
    owner: String,
    request: HiddenScanRequest,
) -> Result<HiddenThreadDeletePlan> {
    let source = paths.home.canonicalize().context("原生会话目录不可用")?;
    let Some(mode) = request.mode else {
        ensure!(
            request.scan_id.is_none() && request.cursor.is_none() && request.limit.is_none(),
            "缺少扫描模式"
        );
        let slot = SLOTS
            .clone()
            .try_acquire_owned()
            .context("扫描任务已达上限，请稍后重试")?;
        let work = ScanWork::new(false);
        let active = SourceLease::acquire(source)?;
        return crate::read_tasks::spawn_blocking(move || {
            let _slot = slot;
            let _active = active;
            super::hidden_delete_plan_with_work(&paths, &work)
        })
        .await?;
    };
    let limit = request.limit.unwrap_or(100);
    ensure!((1..=100).contains(&limit), "扫描每页必须为 1 至 100 项");
    let cursor = request.cursor.unwrap_or(0);
    match mode {
        HiddenScanMode::Start => {
            ensure!(
                request.scan_id.is_none() && cursor == 0,
                "新扫描不能携带旧游标"
            );
            let active = SourceLease::acquire(source.clone())?;
            // Retire the oldest completed context before reserving a slot.
            {
                let mut jobs = JOBS.lock().map_err(|_| anyhow::anyhow!("扫描服务不可用"))?;
                if SLOTS.available_permits() == 0 {
                    let oldest = jobs
                        .values()
                        .filter(|previous| previous.finished.load(Ordering::Acquire))
                        .min_by_key(|previous| previous.created)
                        .map(|previous| previous.id.clone());
                    if let Some(id) = oldest {
                        jobs.remove(&id);
                    }
                }
            }
            let slot = SLOTS
                .clone()
                .try_acquire_owned()
                .context("扫描任务已达上限，请稍后重试")?;
            let work = ScanWork::new(true);
            let job = Arc::new(ScanJob {
                id: uuid::Uuid::new_v4().to_string(),
                source,
                owner,
                created: Instant::now(),
                page_size: limit,
                _slot: slot,
                finished: AtomicBool::new(false),
                state: Mutex::new(JobState {
                    plan: empty_plan(),
                    items: Vec::new(),
                    phase: HiddenScanPhase::Preparing,
                    error: None,
                }),
                work,
            });
            {
                let mut jobs = JOBS.lock().map_err(|_| anyhow::anyhow!("扫描服务不可用"))?;
                jobs.retain(|_, previous| {
                    if previous.work.expired() {
                        previous.work.cancel();
                    }
                    !(previous.work.expired() && previous.finished.load(Ordering::Acquire))
                });
                ensure!(
                    !jobs.values().any(|previous| previous.source == job.source
                        && !previous.finished.load(Ordering::Acquire)),
                    "该来源已有扫描正在进行或停止，请稍后重试"
                );
                if jobs.len() >= 2 {
                    let oldest = jobs
                        .values()
                        .filter(|previous| previous.finished.load(Ordering::Acquire))
                        .min_by_key(|previous| previous.created)
                        .map(|previous| previous.id.clone());
                    if let Some(id) = oldest {
                        jobs.remove(&id);
                    }
                }
                ensure!(jobs.len() < 2, "扫描任务已达上限，请稍后重试");
                jobs.insert(job.id.clone(), job.clone());
            }
            let worker = job.clone();
            tokio::spawn(async move {
                let task = worker.clone();
                let outcome = crate::read_tasks::spawn_blocking(move || {
                    let _active = active;
                    super::scan_hidden(&paths, &task.work, |plan, candidate| {
                        task.work.check()?;
                        let mut state = task
                            .state
                            .lock()
                            .map_err(|_| anyhow::anyhow!("扫描状态不可用"))?;
                        if let Some(candidate) = candidate {
                            state.items.push(candidate);
                            state.phase = HiddenScanPhase::Scanning;
                        } else {
                            state.plan = metadata(plan);
                        }
                        Ok(())
                    })
                    .map(|_| ())
                })
                .await;
                if let Ok(mut state) = worker.state.lock() {
                    match outcome {
                        _ if worker.work.cancelled() || worker.work.expired() => {
                            state.phase = HiddenScanPhase::Stopped;
                        }
                        Ok(Ok(())) => {
                            state.phase = HiddenScanPhase::Complete;
                        }
                        Ok(Err(error)) => {
                            state.phase = HiddenScanPhase::Failed;
                            state.error = Some(error.to_string());
                        }
                        Err(_) => {
                            state.phase = HiddenScanPhase::Failed;
                            state.error = Some("扫描工作未能完成，请重试".into());
                        }
                    }
                }
                worker.finished.store(true, Ordering::Release);
            });
            let weak = Arc::downgrade(&job);
            tokio::spawn(async move {
                loop {
                    let remaining = match weak.upgrade() {
                        Some(job) => job.work.idle_remaining(),
                        None => break,
                    };
                    tokio::time::sleep(remaining).await;
                    let Some(job) = weak.upgrade() else {
                        break;
                    };
                    if job.work.expired() {
                        job.work.cancel();
                        if let Ok(mut jobs) = JOBS.lock() {
                            jobs.remove(&job.id);
                        }
                        break;
                    }
                }
            });
            job.response(0, limit)
        }
        HiddenScanMode::Poll | HiddenScanMode::Cancel => {
            let id = request.scan_id.context("缺少扫描 ID")?;
            let job = JOBS
                .lock()
                .map_err(|_| anyhow::anyhow!("扫描服务不可用"))?
                .get(&id)
                .cloned()
                .context("扫描已过期，请重新扫描")?;
            ensure!(
                job.owner == owner && job.source == source,
                "扫描不属于当前来源"
            );
            ensure!(job.page_size == limit, "扫描分页大小不能改变");
            ensure!(!job.work.expired(), "扫描已过期，请重新扫描");
            job.work.touch();
            if matches!(mode, HiddenScanMode::Cancel) {
                job.work.cancel();
                let mut state = job
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("扫描状态不可用"))?;
                state.phase = if job.finished.load(Ordering::Acquire) {
                    HiddenScanPhase::Stopped
                } else {
                    HiddenScanPhase::Stopping
                };
            }
            job.response(cursor, limit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(total: usize, processed: usize) -> ScanJob {
        let work = ScanWork::default();
        let mut plan = empty_plan();
        plan.hidden_threads = total as u64;
        ScanJob {
            id: "fixture-scan".into(),
            source: PathBuf::new(),
            owner: "fixture-owner".into(),
            created: Instant::now(),
            page_size: 100,
            _slot: Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap(),
            finished: AtomicBool::new(false),
            state: Mutex::new(JobState {
                plan,
                items: (0..processed)
                    .map(|index| HiddenThreadCandidate {
                        id: index.to_string(),
                        title: "fixture".into(),
                        fingerprint: Some("fixture-fingerprint".into()),
                        allowed: true,
                        reason: None,
                    })
                    .collect(),
                phase: HiddenScanPhase::Scanning,
                error: None,
            }),
            work,
        }
    }

    #[test]
    fn incomplete_pages_wait_without_advertising_immediate_results() {
        let job = job(250, 150);
        let first = job.response(0, 100).unwrap();
        assert_eq!(first.candidates.len(), 100);
        assert!(!first.scan.as_ref().unwrap().has_more);
        let waiting = job.response(100, 100).unwrap();
        assert!(waiting.candidates.is_empty());
        assert_eq!(waiting.scan.as_ref().unwrap().next_cursor, 100);
        assert!(!waiting.scan.as_ref().unwrap().has_more);
        let repeated = job.response(0, 100).unwrap();
        assert_eq!(
            serde_json::to_value(first.candidates).unwrap(),
            serde_json::to_value(repeated.candidates).unwrap()
        );
        job.state.lock().unwrap().phase = HiddenScanPhase::Stopped;
        let last = job.response(100, 100).unwrap();
        assert_eq!(last.candidates.len(), 50);
        assert!(!last.scan.as_ref().unwrap().has_more);
    }

    #[test]
    fn complete_pages_advance_and_reject_out_of_range_cursors() {
        let job = job(250, 250);
        job.state.lock().unwrap().phase = HiddenScanPhase::Complete;
        for (cursor, length, has_more) in [
            (0, 100, true),
            (100, 100, true),
            (200, 50, false),
            (250, 0, false),
        ] {
            let page = job.response(cursor, 100).unwrap();
            assert_eq!(page.candidates.len(), length);
            let progress = page.scan.unwrap();
            assert_eq!(progress.next_cursor, cursor + length);
            assert_eq!(progress.has_more, has_more);
        }
        assert!(job.response(251, 100).is_err());
    }
}
