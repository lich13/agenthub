//! GitHub Actions summaries. gh credentials stay in the source process.
use crate::{config::Config, db::PanelDb};
use anyhow::{Context, Result};
use reqwest::{header, Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use tokio::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub refresh: bool,
    #[serde(default)]
    pub cached_only: bool,
}

const STATE: &str = "github_ci_state";
const REFRESH: &str = "github_ci_refresh";
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
    #[serde(default)]
    pub is_bot: bool,
}
fn one() -> u64 {
    1
}
pub fn matches_run_filter(run: &Run, repositories: &[String], filter_bots: bool) -> bool {
    if filter_bots && run.is_bot {
        return false;
    }
    if repositories.is_empty() {
        return true;
    }
    let repo_id = run.repository_id.to_string();
    repositories.iter().any(|item| {
        let trimmed = item.trim();
        !trimmed.is_empty()
            && (trimmed.eq_ignore_ascii_case(&run.repository_name) || trimmed == repo_id)
    })
}
fn is_bot_identity(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower.ends_with("[bot]")
        || matches!(lower.as_str(), "dependabot" | "renovate" | "github-actions")
}
pub(crate) fn detect_bot_run(raw: &serde_json::Value) -> bool {
    for actor_key in ["actor", "triggering_actor"] {
        if let Some(actor) = raw.get(actor_key) {
            if actor
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("bot"))
            {
                return true;
            }
            if actor
                .get("login")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_bot_identity)
            {
                return true;
            }
        }
    }
    if let Some(commit) = raw.get("head_commit") {
        for role in ["author", "committer"] {
            if commit
                .get(role)
                .and_then(|person| person.get("name"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_bot_identity)
            {
                return true;
            }
        }
    }
    raw.get("head_branch")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|branch| {
            let lower = branch.trim().to_ascii_lowercase();
            lower.starts_with("dependabot/") || lower.starts_with("renovate/")
        })
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
    pub fn notification_text(&self, machine: &str) -> (String, String) {
        let result = match self.conclusion.as_deref() {
            Some("success") => "成功",
            Some("timed_out") => "超时",
            _ => "失败",
        };
        let clean = |value: &str| {
            crate::security::redact_output(value)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(100)
                .collect::<String>()
        };
        (
            format!("GitHub CI｜{result}｜{}", clean(&self.repository_name)),
            format!(
                "{}\n{}\n{} · {}",
                clean(machine),
                clean(self.name.as_deref().unwrap_or("CI")),
                clean(self.head_branch.as_deref().unwrap_or("")),
                self.head_sha.chars().take(7).collect::<String>()
            ),
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
    pub baseline_ms: i64,
    pub error: Option<String>,
    pub refreshing: bool,
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
pub fn status(db: &PanelDb, config: &Config, options: Request) -> Result<Snapshot> {
    let stored = load(db)?;
    let mut snapshot = stored.snapshot;
    let mut request = request_ms(db)?;
    if enabled(config)
        && !options.cached_only
        && (options.refresh || snapshot.last_scan_ms == 0)
        && request <= stored.request_ms
    {
        request = now();
        db.set_setting(REFRESH, &request.to_string())?;
    }
    snapshot.baseline_ms = stored.baseline_ms;
    snapshot.enabled = enabled(config);
    snapshot.refreshing = enabled(config) && request > stored.request_ms;
    Ok(snapshot)
}
pub fn event_current(db: &PanelDb, config: &Config, event: &Event) -> Result<bool> {
    let state = load(db)?;
    let repositories = &config.probe.notifications.github_ci_repositories;
    let filter_bots = config.probe.notifications.github_ci_filter_bots;
    Ok(enabled(config)
        && matches_run_filter(&event.run, repositories, filter_bots)
        && event.observed_ms >= state.baseline_ms
        && state.snapshot.enabled
        && state.snapshot.account.as_ref() == Some(&event.account)
        && state.snapshot.runs.iter().any(|r| {
            r.id == event.run.id
                && r.repository_id == event.run.repository_id
                && r.run_attempt == event.run.run_attempt
                && r.status == "completed"
                && r.conclusion == event.run.conclusion
                && matches_run_filter(r, repositories, filter_bots)
        }))
}
pub fn settings_changed(db: &PanelDb, before: &Config, after: &Config) -> Result<()> {
    if enabled(before) == enabled(after) {
        if before.probe.notifications.github_ci_repositories
            != after.probe.notifications.github_ci_repositories
            || before.probe.notifications.github_ci_filter_bots
                != after.probe.notifications.github_ci_filter_bots
        {
            let mut state = load(db)?;
            state.pending.retain(|event| {
                matches_run_filter(
                    &event.run,
                    &after.probe.notifications.github_ci_repositories,
                    after.probe.notifications.github_ci_filter_bots,
                )
            });
            save(db, &state)?;
        }
        return Ok(());
    }
    let mut state = load(db)?;
    state.snapshot.enabled = false;
    state.baseline_ms = 0;
    state.pending.clear();
    save(db, &state)?;
    if enabled(after) {
        db.set_setting(REFRESH, &now().to_string())?;
    } else {
        db.set_setting(REFRESH, "0")?;
    }
    Ok(())
}

pub fn acknowledge(db: &PanelDb, keys: &[String]) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let keys: HashSet<_> = keys.iter().map(String::as_str).collect();
    let mut state = load(db)?;
    state
        .pending
        .retain(|event| !keys.contains(event.key().as_str()));
    save(db, &state)
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
            client: crate::http_client_builder()
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
    pub async fn tick(&mut self, db: &PanelDb, config: &Config) -> Result<Vec<Event>> {
        self.tick_with_token(db, config, gh_token()).await
    }

    async fn tick_with_token(
        &mut self,
        db: &PanelDb,
        config: &Config,
        token: impl std::future::Future<Output = Result<String>>,
    ) -> Result<Vec<Event>> {
        let request = request_ms(db)?;
        let active = enabled(config);
        let time = now();
        if self.active == Some(active)
            && request <= self.request_ms
            && (time < self.retry_at || !active || time - self.last_poll_ms < POLL_SECONDS * 1000)
        {
            return Ok(Vec::new());
        }
        let mut state = load(db)?;
        let baseline_before = state.baseline_ms;
        let enabled_before = state.snapshot.enabled;
        self.active = Some(active);
        if !active {
            let changed = enabled_before
                || baseline_before != 0
                || !state.pending.is_empty()
                || state.request_ms != request;
            state.snapshot.enabled = false;
            state.baseline_ms = 0;
            state.pending.clear();
            state.request_ms = request;
            self.request_ms = request;
            if changed {
                save(db, &state)?;
            }
            return Ok(Vec::new());
        }
        if time < self.retry_at {
            state.request_ms = request;
            self.request_ms = request;
            state.snapshot.error = Some("GitHub 请求限流，稍后刷新".into());
            save(db, &state)?;
            return Ok(Vec::new());
        }
        let before = serde_json::to_string(&state)?;
        let result = async {
            let token = token.await?;
            if token != self.token {
                self.cache.clear();
                self.token = token;
            }
            self.scan_with_config(&mut state, config, active, time)
                .await
        }
        .await;
        // Settings may reset the baseline while GitHub is responding. A refresh,
        // however, is satisfied by this scan instead of discarding its results.
        let current = load(db)?;
        if current.baseline_ms != baseline_before || current.snapshot.enabled != enabled_before {
            self.active = None;
            return Ok(Vec::new());
        }
        state.request_ms = request_ms(db)?;
        self.request_ms = state.request_ms;
        self.last_poll_ms = now();
        state.snapshot.error = result.err().map(|error| error.to_string());
        if serde_json::to_string(&state)? != before {
            state.snapshot.last_scan_ms = time;
            save(db, &state)?;
        }
        Ok(state.pending)
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
    #[cfg(test)]
    async fn scan(&mut self, state: &mut Stored, active: bool, time: i64) -> Result<()> {
        self.scan_with_config(state, &Config::default(), active, time)
            .await
    }
    async fn scan_with_config(
        &mut self,
        state: &mut Stored,
        config: &Config,
        active: bool,
        time: i64,
    ) -> Result<()> {
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
            let run_urls: HashSet<_> = repositories
                .iter()
                .filter(|repo| !repo.archived)
                .map(|repo| {
                    format!(
                        "{}/repos/{}/actions/runs?per_page=10",
                        self.api, repo.full_name
                    )
                })
                .collect();
            let repositories_prefix = format!("{}/repos/", self.api);
            self.cache
                .retain(|url, _| !url.starts_with(&repositories_prefix) || run_urls.contains(url));
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
            let result = self.repository_runs(repo).await;
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
        reconcile_with_config(state, account, runs, config, active, baseline, time);
        if !errors.is_empty() {
            anyhow::bail!("{}", errors.join("；"));
        }
        Ok(())
    }
    async fn repository_runs(&mut self, repo: &Repository) -> Result<Vec<Run>> {
        let (page, _) = self
            .get::<serde_json::Value>(&format!(
                "/repos/{}/actions/runs?per_page=10",
                repo.full_name
            ))
            .await?;
        let raw_runs = page
            .get("workflow_runs")
            .and_then(serde_json::Value::as_array)
            .context("GitHub 响应格式错误")?;
        let mut runs = Vec::with_capacity(raw_runs.len());
        for raw_run in raw_runs {
            let mut run: Run = serde_json::from_value(raw_run.clone())
                .map_err(|_| anyhow::anyhow!("GitHub 响应格式错误"))?;
            run.repository_id = repo.id;
            run.repository_name = repo.full_name.clone();
            run.is_bot = run.is_bot || detect_bot_run(raw_run);
            runs.push(run);
        }
        Ok(runs)
    }
}
#[cfg(test)]
fn reconcile(
    state: &mut Stored,
    account: Account,
    runs: Vec<Run>,
    active: bool,
    baseline: bool,
    time: i64,
) {
    reconcile_with_config(
        state,
        account,
        runs,
        &Config::default(),
        active,
        baseline,
        time,
    );
}
fn reconcile_with_config(
    state: &mut Stored,
    account: Account,
    mut runs: Vec<Run>,
    config: &Config,
    active: bool,
    baseline: bool,
    time: i64,
) {
    let repositories = &config.probe.notifications.github_ci_repositories;
    let filter_bots = config.probe.notifications.github_ci_filter_bots;
    if !active {
        state.pending.clear();
        state.baseline_ms = 0;
    }
    if active {
        let mut previous = HashMap::with_capacity(state.snapshot.runs.len());
        for run in &state.snapshot.runs {
            previous.entry((run.repository_id, run.id)).or_insert(run);
        }
        let mut pending: HashSet<_> = state
            .pending
            .iter()
            .map(|event| (event.run.repository_id, event.run.id, event.run.run_attempt))
            .collect();
        for run in &runs {
            if baseline
                || run.notification_kind().is_none()
                || !matches_run_filter(run, repositories, filter_bots)
            {
                continue;
            }
            let newly_finished = previous.get(&(run.repository_id, run.id)).map_or_else(
                || run.finished_ms() > state.baseline_ms,
                |old| old.run_attempt != run.run_attempt || old.status != "completed",
            );
            if newly_finished && pending.insert((run.repository_id, run.id, run.run_attempt)) {
                state.pending.push(Event {
                    observed_ms: time,
                    account: account.clone(),
                    run: run.clone(),
                });
            }
        }
    }
    retain_summaries(&mut runs);
    let current: HashSet<_> = runs
        .iter()
        .map(|run| {
            (
                run.repository_id,
                run.id,
                run.run_attempt,
                run.conclusion.as_deref(),
            )
        })
        .collect();
    state.pending.retain(|e| {
        matches_run_filter(&e.run, repositories, filter_bots)
            && current.contains(&(
                e.run.repository_id,
                e.run.id,
                e.run.run_attempt,
                e.run.conclusion.as_deref(),
            ))
    });
    state.snapshot.runs = runs;
    state.snapshot.account = Some(account);
    state.snapshot.enabled = active;
    if baseline {
        state.baseline_ms = time;
    }
}
fn retain_summaries(runs: &mut [Run]) {
    runs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
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
