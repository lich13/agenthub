//! GitHub Actions summaries. gh credentials stay in the source process.
use crate::{config::Config, db::PanelDb};
use anyhow::{Context, Result};
use reqwest::{header, Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};
use tokio::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub refresh: bool,
}

const STATE: &str = "github_ci_state";
const REFRESH: &str = "github_ci_refresh";
const MAX_RUNS: usize = 1000;
pub const POLL_SECONDS: i64 = 60;
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: u64,
    pub login: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Repository {
    pub id: u64,
    pub full_name: String,
    #[serde(default)]
    pub archived: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Run {
    pub id: u64,
    #[serde(default = "one")]
    pub run_attempt: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub head_branch: Option<String>,
    pub head_sha: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub repository_id: u64,
    #[serde(default)]
    pub repository_name: String,
}
fn one() -> u64 {
    1
}
impl Run {
    pub fn notification_kind(&self) -> Option<&'static str> {
        if self.status != "completed" {
            return None;
        }
        match self.conclusion.as_deref() {
            Some("success") => Some("completion"),
            Some("failure" | "timed_out") => Some("failure"),
            _ => None,
        }
    }
    pub fn url(&self) -> String {
        format!(
            "https://github.com/{}/actions/runs/{}",
            self.repository_name, self.id
        )
    }
    fn finished_ms(&self) -> i64 {
        chrono::DateTime::parse_from_rfc3339(&self.updated_at).map_or(0, |d| d.timestamp_millis())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    pub observed_ms: i64,
    pub account: Account,
    pub run: Run,
}
impl Event {
    pub fn key(&self) -> String {
        format!(
            "github-ci:github.com:{}:{}:{}:{}",
            self.account.id, self.run.repository_id, self.run.id, self.run.run_attempt
        )
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub account: Option<Account>,
    pub repositories: Vec<Repository>,
    pub runs: Vec<Run>,
    pub enabled: bool,
    pub last_scan_ms: i64,
    pub error: Option<String>,
    pub refreshing: bool,
    pub retry_at_ms: Option<i64>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    snapshot: Snapshot,
    baseline_ms: i64,
    repositories_at_ms: i64,
    request_ms: i64,
    pending: Vec<Event>,
}
fn load(db: &PanelDb) -> Result<Stored> {
    db.get_setting(STATE)?
        .map(|s| serde_json::from_str(&s).context("CI 状态读取失败"))
        .transpose()
        .map(Option::unwrap_or_default)
}
fn save(db: &PanelDb, state: &Stored) -> Result<()> {
    let next = serde_json::to_string(state)?;
    if db.get_setting(STATE)?.as_deref() != Some(&next) {
        db.set_setting(STATE, &next)?;
    }
    Ok(())
}
fn request_ms(db: &PanelDb) -> Result<i64> {
    Ok(db
        .get_setting(REFRESH)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}
pub fn enabled(config: &Config) -> bool {
    config.probe.enabled && config.probe.notifications.notify_github_ci
}
pub fn status(db: &PanelDb, config: &Config, refresh: bool) -> Result<Snapshot> {
    let stored = load(db)?;
    let mut snapshot = stored.snapshot;
    let mut request = request_ms(db)?;
    if (refresh || snapshot.last_scan_ms == 0) && request <= stored.request_ms {
        request = now();
        db.set_setting(REFRESH, &request.to_string())?;
    }
    snapshot.enabled = enabled(config);
    snapshot.refreshing = request > stored.request_ms;
    Ok(snapshot)
}
pub fn event_current(db: &PanelDb, config: &Config, event: &Event) -> Result<bool> {
    let state = load(db)?;
    Ok(enabled(config)
        && event.observed_ms >= state.baseline_ms
        && state.snapshot.enabled
        && state.snapshot.account.as_ref() == Some(&event.account)
        && state.snapshot.runs.iter().any(|r| {
            r.id == event.run.id
                && r.repository_id == event.run.repository_id
                && r.run_attempt == event.run.run_attempt
                && r.status == "completed"
                && r.conclusion == event.run.conclusion
        }))
}
pub fn settings_changed(db: &PanelDb, before: &Config, after: &Config) -> Result<()> {
    if enabled(before) == enabled(after) {
        return Ok(());
    }
    let mut state = load(db)?;
    state.snapshot.enabled = false;
    state.baseline_ms = 0;
    state.pending.clear();
    save(db, &state)?;
    if enabled(after) {
        db.set_setting(REFRESH, &now().to_string())?;
    }
    Ok(())
}

pub fn pending(db: &PanelDb) -> Result<Vec<Event>> {
    Ok(load(db)?.pending)
}
pub fn acknowledge(db: &PanelDb, key: &str) -> Result<()> {
    let mut s = load(db)?;
    s.pending.retain(|e| e.key() != key);
    save(db, &s)
}

#[derive(Clone, Serialize, Deserialize)]
struct RunPage {
    workflow_runs: Vec<Run>,
}
struct Cached {
    etag: Option<String>,
    value: serde_json::Value,
    next: Option<String>,
}
pub struct Monitor {
    client: Client,
    cache: HashMap<String, Cached>,
    token: String,
    retry_at: i64,
    api: String,
    last_poll_ms: i64,
    request_ms: i64,
    active: Option<bool>,
}
impl Monitor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent("AgentHub")
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(12))
                .build()?,
            cache: HashMap::new(),
            token: String::new(),
            retry_at: 0,
            api: "https://api.github.com".into(),
            last_poll_ms: 0,
            request_ms: 0,
            active: None,
        })
    }
    pub async fn tick(&mut self, db: &PanelDb, config: &Config) -> Result<bool> {
        let request = request_ms(db)?;
        let active = enabled(config);
        let time = now();
        if self.active == Some(active)
            && (time < self.retry_at
                || (request <= self.request_ms
                    && (!active || time - self.last_poll_ms < POLL_SECONDS * 1000)))
        {
            return Ok(false);
        }
        let mut state = load(db)?;
        let baseline_before = state.baseline_ms;
        let enabled_before = state.snapshot.enabled;
        let before = serde_json::to_string(&state)?;
        if !active {
            state.snapshot.enabled = false;
            state.baseline_ms = 0;
            state.pending.clear();
        }
        self.active = Some(active);
        if !active && request <= state.request_ms {
            if serde_json::to_string(&state)? != before {
                save(db, &state)?;
            }
            return Ok(false);
        }
        self.retry_at = self
            .retry_at
            .max(state.snapshot.retry_at_ms.unwrap_or_default());
        if time < self.retry_at {
            return Ok(false);
        }
        let result = async {
            let token = gh_token().await?;
            if token != self.token {
                self.cache.clear();
                self.token = token;
            }
            self.scan(&mut state, active, time).await
        }
        .await;
        // A settings save can reset the baseline while GitHub is responding.
        // Leave that reset intact and let the next tick read the new config.
        let current = load(db)?;
        if request_ms(db)? != request
            || current.baseline_ms != baseline_before
            || current.snapshot.enabled != enabled_before
        {
            self.active = None;
            return Ok(false);
        }
        state.request_ms = request;
        self.request_ms = request;
        self.last_poll_ms = now();
        state.snapshot.error = result.err().map(|e| e.to_string());
        state.snapshot.retry_at_ms = (self.retry_at > time).then_some(self.retry_at);
        if serde_json::to_string(&state)? != before {
            state.snapshot.last_scan_ms = time;
            save(db, &state)?;
        }
        Ok(true)
    }
    async fn get<T: DeserializeOwned + Serialize>(
        &mut self,
        path: &str,
    ) -> Result<(T, Option<String>)> {
        let url = if path.starts_with('/') {
            format!("{}{path}", self.api)
        } else {
            path.to_owned()
        };
        anyhow::ensure!(
            url.starts_with(&format!("{}/", self.api)),
            "GitHub 分页地址无效"
        );
        let mut req = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header(header::ACCEPT, "application/vnd.github+json");
        if let Some(etag) = self.cache.get(&url).and_then(|c| c.etag.as_ref()) {
            req = req.header(header::IF_NONE_MATCH, etag);
        }
        let response = req
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("GitHub 请求失败，请重试"))?;
        if response.status() == StatusCode::NOT_MODIFIED {
            let c = self.cache.get(&url).context("GitHub 缓存不可用")?;
            return Ok((serde_json::from_value(c.value.clone())?, c.next.clone()));
        }
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS
            || (status == StatusCode::FORBIDDEN
                && (response.headers().contains_key(header::RETRY_AFTER)
                    || response
                        .headers()
                        .get("x-ratelimit-remaining")
                        .is_some_and(|v| v == "0")))
        {
            let seconds = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|s| s.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok());
            let reset = response
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|s| s.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok());
            self.retry_at = seconds
                .map(|s| now() + s * 1000)
                .or(reset.map(|s| s * 1000))
                .unwrap_or_else(|| now() + 60000)
                .max(now() + 1000);
            anyhow::bail!("GitHub 请求限流，稍后刷新");
        }
        anyhow::ensure!(
            status.is_success(),
            match status.as_u16() {
                401 => "gh 登录已失效",
                403 => "当前账号没有仓库 Actions 读取权限",
                404 => "仓库或工作流已不可访问",
                _ => "GitHub 请求失败，请重试",
            }
        );
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let next = response
            .headers()
            .get(header::LINK)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').find(|p| p.contains("rel=\"next\"")))
            .and_then(|v| v.trim().strip_prefix('<'))
            .and_then(|v| v.split_once('>'))
            .map(|(v, _)| v.to_owned());
        let value: T = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("GitHub 响应格式错误"))?;
        if self.cache.len() >= 256 {
            self.cache.clear();
        }
        self.cache.insert(
            url,
            Cached {
                etag,
                value: serde_json::to_value(&value)?,
                next: next.clone(),
            },
        );
        Ok((value, next))
    }
    async fn scan(&mut self, state: &mut Stored, active: bool, time: i64) -> Result<()> {
        let (account, _) = self.get::<Account>("/user").await?;
        let changed = state
            .snapshot
            .account
            .as_ref()
            .is_none_or(|old| old.id != account.id);
        if changed {
            *state = Stored::default();
            state.snapshot.account = Some(account.clone());
        }
        if changed || time - state.repositories_at_ms >= 600000 {
            let mut next = Some("/user/repos?affiliation=owner&per_page=100".to_owned());
            let mut repositories = Vec::new();
            while let Some(path) = next {
                let (page, more) = self.get::<Vec<Repository>>(&path).await?;
                repositories.extend(page);
                next = more;
            }
            state.snapshot.repositories = repositories;
            state.repositories_at_ms = time;
        }
        let baseline = active && (!state.snapshot.enabled || state.baseline_ms == 0);
        let mut runs = Vec::new();
        let mut errors = Vec::new();
        let repositories = state.snapshot.repositories.clone();
        for (index, repo) in repositories.iter().enumerate() {
            if repo.archived {
                continue;
            }
            let result = self.repository_runs(repo, state).await;
            match result {
                Ok(mut incoming) => runs.append(&mut incoming),
                Err(e) => {
                    errors.push(format!("{}：{e}", repo.full_name));
                    runs.extend(
                        state
                            .snapshot
                            .runs
                            .iter()
                            .filter(|r| r.repository_id == repo.id)
                            .cloned(),
                    );
                    if self.retry_at > time {
                        runs.extend(
                            state
                                .snapshot
                                .runs
                                .iter()
                                .filter(|r| {
                                    repositories[index + 1..]
                                        .iter()
                                        .any(|repo| repo.id == r.repository_id)
                                })
                                .cloned(),
                        );
                        break;
                    }
                }
            }
        }
        reconcile(state, account, runs, active, baseline, time);
        if !errors.is_empty() {
            anyhow::bail!("{}", errors.join("；"));
        }
        Ok(())
    }
    async fn repository_runs(&mut self, repo: &Repository, state: &Stored) -> Result<Vec<Run>> {
        let mut path = Some(format!(
            "/repos/{}/actions/runs?per_page=100",
            repo.full_name
        ));
        let mut runs = BTreeMap::new();
        while let Some(url) = path {
            let (page, next) = self.get::<RunPage>(&url).await?;
            for mut run in page.workflow_runs {
                run.repository_id = repo.id;
                run.repository_name = repo.full_name.clone();
                runs.insert(run.id, run);
            }
            // Reruns retain their original creation order. Read every page so
            // an old run's new attempt, or a long-running job, cannot disappear.
            // Retain only the bounded summaries and all unfinished runs.
            if runs.len() > MAX_RUNS {
                let mut summaries = runs.into_values().collect::<Vec<_>>();
                retain_summaries(&mut summaries);
                runs = summaries.into_iter().map(|run| (run.id, run)).collect();
            }
            path = next;
        }
        for old in state
            .snapshot
            .runs
            .iter()
            .filter(|r| r.repository_id == repo.id && r.status != "completed")
        {
            if runs.contains_key(&old.id) {
                continue;
            }
            let (mut run, _) = self
                .get::<Run>(&format!(
                    "/repos/{}/actions/runs/{}",
                    repo.full_name, old.id
                ))
                .await?;
            run.repository_id = repo.id;
            run.repository_name = repo.full_name.clone();
            runs.insert(run.id, run);
        }
        Ok(runs.into_values().collect())
    }
}
fn reconcile(
    state: &mut Stored,
    account: Account,
    mut runs: Vec<Run>,
    active: bool,
    baseline: bool,
    time: i64,
) {
    if !active {
        state.pending.clear();
        state.baseline_ms = 0;
    }
    if active {
        for run in &runs {
            let old = state
                .snapshot
                .runs
                .iter()
                .find(|old| old.id == run.id && old.repository_id == run.repository_id);
            let newly_finished = if baseline {
                run.finished_ms() > time
            } else {
                old.map_or(run.finished_ms() > state.baseline_ms, |old| {
                    old.run_attempt != run.run_attempt || old.status != "completed"
                })
            };
            if run.notification_kind().is_some()
                && newly_finished
                && !state.pending.iter().any(|e| {
                    e.run.id == run.id
                        && e.run.run_attempt == run.run_attempt
                        && e.run.repository_id == run.repository_id
                })
            {
                state.pending.push(Event {
                    observed_ms: time,
                    account: account.clone(),
                    run: run.clone(),
                });
            }
        }
    }
    retain_summaries(&mut runs);
    state.pending.retain(|e| {
        runs.iter().any(|r| {
            r.id == e.run.id
                && r.repository_id == e.run.repository_id
                && r.run_attempt == e.run.run_attempt
                && r.conclusion == e.run.conclusion
        })
    });
    state.snapshot.runs = runs;
    state.snapshot.account = Some(account);
    state.snapshot.enabled = active;
    if baseline {
        state.baseline_ms = time;
    }
}
fn retain_summaries(runs: &mut Vec<Run>) {
    runs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    let mut completed = 0;
    runs.retain(|run| {
        if run.status != "completed" {
            return true;
        }
        completed += 1;
        completed <= MAX_RUNS
    });
}
async fn gh_token() -> Result<String> {
    let program = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|p| p.join("gh"))
        .chain(
            ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]
                .map(std::path::PathBuf::from),
        )
        .find(|p| p.is_file())
        .context("未安装 GitHub CLI")?;
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new(program)
            .args(["auth", "token", "--hostname", "github.com"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("gh 读取超时"))?
    .map_err(|_| anyhow::anyhow!("无法读取 gh 登录"))?;
    anyhow::ensure!(output.status.success(), "当前用户尚未登录 gh");
    let token = String::from_utf8(output.stdout)
        .map_err(|_| anyhow::anyhow!("gh 登录不可读取"))?
        .trim()
        .to_owned();
    anyhow::ensure!(!token.is_empty(), "当前用户尚未登录 gh");
    Ok(token)
}

#[cfg(test)]
mod tests;
