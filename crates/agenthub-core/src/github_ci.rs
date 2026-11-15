//! GitHub Actions summaries. gh credentials stay in the source process.
use crate::{config::Config, db::PanelDb};
use anyhow::{Context, Result};
use reqwest::{header, Client, StatusCode};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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
pub const ACTIVE_POLL_SECONDS: i64 = 5;
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
pub fn matches_repository_filter(repo: &Repository, repositories: &[String]) -> bool {
    if !repositories.iter().any(|item| !item.trim().is_empty()) {
        return true;
    }
    let repo_id = repo.id.to_string();
    repositories.iter().any(|item| {
        let trimmed = item.trim();
        !trimmed.is_empty() && (trimmed.eq_ignore_ascii_case(&repo.full_name) || trimmed == repo_id)
    })
}
pub fn matches_run_filter(run: &Run, repositories: &[String], filter_bots: bool) -> bool {
    if filter_bots && run.is_bot {
        return false;
    }
    if !repositories.iter().any(|item| !item.trim().is_empty()) {
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
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
    pub monitored_repositories: Vec<String>,
    pub filter_bots: bool,
    pub scanned_repository_ids: BTreeSet<u64>,
    pub repository_baselines_ms: BTreeMap<u64, i64>,
    pub bot_baseline_ms: i64,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct Stored {
    snapshot: Snapshot,
    baseline_ms: i64,
    repositories_at_ms: i64,
    request_ms: i64,
    #[serde(default)]
    monitored_repo_ids: BTreeSet<u64>,
    pending: Vec<Event>,
}
fn load(db: &PanelDb) -> Result<Stored> {
    db.get_setting(STATE)?
        .map(|s| serde_json::from_str(&s).context("CI 状态读取失败"))
        .transpose()
        .map(Option::unwrap_or_default)
}
#[cfg(test)]
fn save(db: &PanelDb, state: &Stored) -> Result<()> {
    update_state(db, |current, _| {
        *current = state.clone();
        Ok(())
    })
}
fn read_state(tx: &rusqlite::Transaction<'_>) -> Result<(Option<String>, Stored, i64)> {
    let read = |key: &str| -> Result<Option<String>> {
        Ok(tx
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    };
    let original = read(STATE)?;
    let state = original
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    let request = read(REFRESH)?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    Ok((original, state, request))
}
fn update_state<T>(
    db: &PanelDb,
    update: impl FnOnce(&mut Stored, &mut i64) -> Result<T>,
) -> Result<T> {
    db.with_transaction(|tx| update_state_in_transaction(tx, update))
}
fn update_state_in_transaction<T>(
    tx: &rusqlite::Transaction<'_>,
    update: impl FnOnce(&mut Stored, &mut i64) -> Result<T>,
) -> Result<T> {
    let (original, mut state, old_request) = read_state(tx)?;
    let mut request = old_request;
    let result = update(&mut state, &mut request)?;
    let next = serde_json::to_string(&state)?;
    let changed = original.as_deref() != Some(&next);
    for (key, value, changed) in [
        (STATE, next, changed),
        (REFRESH, request.to_string(), request != old_request),
    ] {
        if changed {
            tx.execute("INSERT INTO settings(key,value,updated_at) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at", rusqlite::params![key,value,chrono::Utc::now().timestamp()])?;
        }
    }
    Ok(result)
}
fn same_settings(before: &Config, after: &Config) -> bool {
    enabled(before) == enabled(after)
        && before.probe.notifications.github_ci_repositories
            == after.probe.notifications.github_ci_repositories
        && before.probe.notifications.github_ci_filter_bots
            == after.probe.notifications.github_ci_filter_bots
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
    snapshot.monitored_repositories = config.probe.notifications.github_ci_repositories.clone();
    snapshot.filter_bots = config.probe.notifications.github_ci_filter_bots;
    snapshot.scanned_repository_ids = stored.monitored_repo_ids;
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
pub(crate) fn settings_changed(
    tx: &rusqlite::Transaction<'_>,
    before: &Config,
    after: &Config,
) -> Result<()> {
    if same_settings(before, after) {
        return Ok(());
    }
    update_state_in_transaction(tx, |state, request| {
        state.snapshot.monitored_repositories =
            after.probe.notifications.github_ci_repositories.clone();
        state.snapshot.filter_bots = after.probe.notifications.github_ci_filter_bots;
        if enabled(before) == enabled(after) {
            let repos_changed = before.probe.notifications.github_ci_repositories
                != after.probe.notifications.github_ci_repositories;
            let bots_changed = before.probe.notifications.github_ci_filter_bots
                != after.probe.notifications.github_ci_filter_bots;
            if repos_changed || bots_changed {
                let after_repos = &after.probe.notifications.github_ci_repositories;
                state.pending.retain(|event| {
                    matches_run_filter(
                        &event.run,
                        after_repos,
                        after.probe.notifications.github_ci_filter_bots,
                    )
                });
                if bots_changed && !after.probe.notifications.github_ci_filter_bots {
                    state.snapshot.bot_baseline_ms = now();
                }
                if repos_changed {
                    state
                        .snapshot
                        .runs
                        .retain(|run| matches_run_filter(run, after_repos, false));
                    let retained_repo_ids: HashSet<u64> = state
                        .snapshot
                        .repositories
                        .iter()
                        .filter(|repo| {
                            !repo.archived && matches_repository_filter(repo, after_repos)
                        })
                        .map(|repo| repo.id)
                        .chain(state.snapshot.runs.iter().map(|run| run.repository_id))
                        .collect();
                    state
                        .monitored_repo_ids
                        .retain(|id| retained_repo_ids.contains(id));
                    state
                        .snapshot
                        .repository_baselines_ms
                        .retain(|id, _| retained_repo_ids.contains(id));
                    if after_repos.iter().any(|entry| {
                        let trimmed = entry.trim();
                        !trimmed.is_empty()
                            && !state.snapshot.repositories.iter().any(|repo| {
                                trimmed.eq_ignore_ascii_case(&repo.full_name)
                                    || trimmed == repo.id.to_string()
                            })
                    }) {
                        state.repositories_at_ms = 0;
                    }
                }

                if enabled(after) {
                    *request = now();
                }
            }
            return Ok(());
        }

        state.snapshot.enabled = false;
        state.baseline_ms = 0;
        state.monitored_repo_ids.clear();
        state.snapshot.repository_baselines_ms.clear();
        state.snapshot.bot_baseline_ms = 0;
        state.pending.clear();

        if enabled(after) {
            *request = now();
        } else {
            *request = 0;
        }
        Ok(())
    })
}

pub fn acknowledge(db: &PanelDb, keys: &[String]) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let keys: HashSet<_> = keys.iter().map(String::as_str).collect();
    update_state(db, |state, _| {
        state
            .pending
            .retain(|event| !keys.contains(event.key().as_str()));
        Ok(())
    })
}

#[derive(Debug, Clone)]
enum CachedValue {
    Account(Account),
    Repositories(Vec<Repository>),
    Runs(Vec<Run>),
}
struct Cached {
    etag: Option<String>,
    value: CachedValue,
    next: Option<String>,
}
pub struct Monitor {
    client: Client,
    cache: HashMap<String, Cached>,
    token: String,
    retry_at: i64,
    api: String,
    credentials_due_ms: i64,
    repository_due_ms: HashMap<u64, i64>,
    next_poll_ms: i64,
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
            credentials_due_ms: 0,
            repository_due_ms: HashMap::new(),
            next_poll_ms: 0,
            request_ms: 0,
            active: None,
        })
    }
    pub async fn tick(
        &mut self,
        db: &PanelDb,
        load_config: &(dyn Fn() -> Result<Config> + Sync),
    ) -> Result<Vec<Event>> {
        self.tick_with_token(db, load_config, gh_token()).await
    }

    async fn tick_with_token(
        &mut self,
        db: &PanelDb,
        load_config: &(dyn Fn() -> Result<Config> + Sync),
        token: impl std::future::Future<Output = Result<String>>,
    ) -> Result<Vec<Event>> {
        let request = request_ms(db)?;
        let active = enabled(&load_config()?);
        let time = now();
        if self.active == Some(active)
            && request <= self.request_ms
            && (time < self.retry_at || !active || time < self.next_poll_ms)
        {
            return Ok(Vec::new());
        }
        // The save path takes the same transaction before replacing the file.
        // Read the file cache here, never a caller's potentially stale Config.
        let (config, mut state, request, time) = db.with_transaction(|tx| {
            let (_, state, request) = read_state(tx)?;
            Ok((load_config()?, state, request, now()))
        })?;
        let active = enabled(&config);
        let original = state.clone();
        let enabled_before = state.snapshot.enabled;
        self.active = Some(active);
        let mut account_ready = false;
        if !active {
            if !state.snapshot.enabled
                && state.baseline_ms == 0
                && state.monitored_repo_ids.is_empty()
                && state.pending.is_empty()
                && state.request_ms == request
            {
                self.request_ms = request;
                return Ok(Vec::new());
            }
            state.snapshot.enabled = false;
            state.baseline_ms = 0;
            state.monitored_repo_ids.clear();
            state.snapshot.repository_baselines_ms.clear();
            state.snapshot.bot_baseline_ms = 0;
            state.pending.clear();
        } else if time < self.retry_at {
            state.snapshot.error = Some("GitHub 请求限流，稍后刷新".into());
        } else {
            let refresh = request > self.request_ms || !enabled_before;
            let check_account = refresh
                || self.token.is_empty()
                || time >= self.credentials_due_ms
                || state.snapshot.account.is_none();
            let result = async {
                if check_account {
                    // Only a verified account can extend the credential lease.
                    self.credentials_due_ms = 0;
                    let token = token.await?;
                    if token != self.token {
                        self.cache.clear();
                        self.repository_due_ms.clear();
                        self.token = token;
                    }
                }
                let account = if check_account {
                    let account = self.account().await?;
                    self.credentials_due_ms = time + POLL_SECONDS * 1000;
                    account
                } else {
                    state.snapshot.account.clone().expect("known account")
                };
                account_ready = true;
                self.scan_due(&mut state, &config, account, active, time, refresh)
                    .await
            }
            .await;
            self.next_poll_ms = if result.is_err()
                && (!account_ready
                    || state.repositories_at_ms == original.repositories_at_ms
                        && time - state.repositories_at_ms >= 600000)
            {
                // Directory failures occur before repository deadlines are advanced.
                if self.retry_at > time {
                    self.retry_at
                } else {
                    time + POLL_SECONDS * 1000
                }
            } else {
                self.repository_due_ms
                    .values()
                    .copied()
                    .chain(std::iter::once(self.credentials_due_ms))
                    .min()
                    .unwrap_or(time + POLL_SECONDS * 1000)
            };
            if self.retry_at > time {
                self.next_poll_ms = self.retry_at;
            }
            state.snapshot.error = result.err().map(|error| error.to_string());
        }
        state.snapshot.monitored_repositories =
            config.probe.notifications.github_ci_repositories.clone();
        state.snapshot.filter_bots = config.probe.notifications.github_ci_filter_bots;
        // No network request holds this transaction. Settings and acknowledgements
        // may commit while GitHub responds; merge only against their current state.
        let committed = update_state(db, |current, latest_request| {
            if !same_settings(&load_config()?, &config)
                || current.snapshot.monitored_repositories
                    != original.snapshot.monitored_repositories
                || current.snapshot.filter_bots != original.snapshot.filter_bots
                || current.baseline_ms != original.baseline_ms
                || current.snapshot.enabled != original.snapshot.enabled
                || current.snapshot.repository_baselines_ms
                    != original.snapshot.repository_baselines_ms
                || current.snapshot.bot_baseline_ms != original.snapshot.bot_baseline_ms
                || current.monitored_repo_ids != original.monitored_repo_ids
            {
                return Ok(false);
            }
            let pending_before: HashSet<_> = original.pending.iter().map(Event::key).collect();
            let pending_now: HashSet<_> = current.pending.iter().map(Event::key).collect();
            state.pending.retain(|event| {
                let key = event.key();
                !pending_before.contains(&key) || pending_now.contains(&key)
            });
            state.request_ms = *latest_request;
            if state != *current {
                state.snapshot.last_scan_ms = time;
                *current = state.clone();
            }
            Ok(true)
        })?;
        if !committed {
            self.active = None;
            return Ok(Vec::new());
        }
        self.request_ms = state.request_ms;
        Ok(if account_ready {
            state.pending
        } else {
            Vec::new()
        })
    }

    pub fn wait_duration(&self) -> Duration {
        if self.active != Some(true) {
            return Duration::from_secs(2);
        }
        let due = self.next_poll_ms.max(self.retry_at);
        Duration::from_millis(due.saturating_sub(now()).clamp(0, 2000) as u64)
    }

    async fn get(
        &mut self,
        path: &str,
        parse: impl FnOnce(serde_json::Value) -> Result<CachedValue>,
    ) -> Result<(CachedValue, Option<String>)> {
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
            return Ok((c.value.clone(), c.next.clone()));
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
        let raw = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("GitHub 响应格式错误"))?;
        let value = parse(raw).map_err(|_| anyhow::anyhow!("GitHub 响应格式错误"))?;
        self.cache.insert(
            url,
            Cached {
                etag,
                value: value.clone(),
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
    #[cfg(test)]
    async fn scan_with_config(
        &mut self,
        state: &mut Stored,
        config: &Config,
        active: bool,
        time: i64,
    ) -> Result<()> {
        let account = self.account().await?;
        self.scan_due(state, config, account, active, time, true)
            .await
    }
    async fn account(&mut self) -> Result<Account> {
        let (CachedValue::Account(account), _) = self
            .get("/user", |raw| {
                Ok(CachedValue::Account(serde_json::from_value(raw)?))
            })
            .await?
        else {
            anyhow::bail!("GitHub 响应格式错误")
        };
        Ok(account)
    }
    async fn scan_due(
        &mut self,
        state: &mut Stored,
        config: &Config,
        account: Account,
        active: bool,
        time: i64,
        refresh: bool,
    ) -> Result<()> {
        let changed = state
            .snapshot
            .account
            .as_ref()
            .is_none_or(|old| old.id != account.id);
        if changed {
            *state = Stored::default();
            self.repository_due_ms.clear();
            state.snapshot.account = Some(account.clone());
        }
        if changed || time - state.repositories_at_ms >= 600000 {
            let mut next = Some("/user/repos?affiliation=owner&per_page=100".to_owned());
            let mut repositories = Vec::new();
            while let Some(path) = next {
                let (CachedValue::Repositories(page), more) = self
                    .get(&path, |raw| {
                        Ok(CachedValue::Repositories(serde_json::from_value(raw)?))
                    })
                    .await?
                else {
                    anyhow::bail!("GitHub 响应格式错误")
                };
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
        let mut scanned_repo_ids = HashSet::new();
        let configured_repos = &config.probe.notifications.github_ci_repositories;
        let repositories = state.snapshot.repositories.clone();
        self.repository_due_ms.retain(|id, _| {
            repositories.iter().any(|repo| {
                repo.id == *id
                    && !repo.archived
                    && matches_repository_filter(repo, configured_repos)
            })
        });
        for (index, repo) in repositories.iter().enumerate() {
            if repo.archived || !matches_repository_filter(repo, configured_repos) {
                continue;
            }
            if !refresh
                && !baseline
                && self
                    .repository_due_ms
                    .get(&repo.id)
                    .is_some_and(|due| time < *due)
            {
                runs.extend(
                    state
                        .snapshot
                        .runs
                        .iter()
                        .filter(|run| run.repository_id == repo.id)
                        .cloned(),
                );
                continue;
            }
            let result = self.repository_runs(repo).await;
            let fast = result
                .as_ref()
                .unwrap_or(&state.snapshot.runs)
                .iter()
                .any(|run| {
                    run.repository_id == repo.id
                        && run.status != "completed"
                        && matches_run_filter(
                            run,
                            configured_repos,
                            config.probe.notifications.github_ci_filter_bots,
                        )
                });
            self.repository_due_ms.insert(
                repo.id,
                time + if fast {
                    ACTIVE_POLL_SECONDS
                } else {
                    POLL_SECONDS
                } * 1000,
            );
            match result {
                Ok(mut incoming) => {
                    scanned_repo_ids.insert(repo.id);
                    runs.append(&mut incoming);
                }
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
                                    repositories[index + 1..].iter().any(|repo| {
                                        !repo.archived
                                            && matches_repository_filter(repo, configured_repos)
                                            && repo.id == r.repository_id
                                    })
                                })
                                .cloned(),
                        );
                        break;
                    }
                }
            }
        }
        reconcile_with_config(state, account, runs, config, active, baseline, time);
        if active {
            state.monitored_repo_ids.extend(&scanned_repo_ids);
            for id in scanned_repo_ids {
                state
                    .snapshot
                    .repository_baselines_ms
                    .entry(id)
                    .or_insert(time);
            }
            state
                .snapshot
                .repository_baselines_ms
                .retain(|id, _| state.monitored_repo_ids.contains(id));
        }
        if !errors.is_empty() {
            anyhow::bail!("{}", errors.join("；"));
        }
        Ok(())
    }
    async fn repository_runs(&mut self, repo: &Repository) -> Result<Vec<Run>> {
        let (CachedValue::Runs(runs), _) = self
            .get(
                &format!("/repos/{}/actions/runs?per_page=10", repo.full_name),
                |mut page| {
                    let raw_runs = page
                        .get_mut("workflow_runs")
                        .and_then(serde_json::Value::as_array_mut)
                        .context("GitHub 响应格式错误")?;
                    let mut runs = Vec::with_capacity(raw_runs.len());
                    for raw in raw_runs.drain(..) {
                        let is_bot = detect_bot_run(&raw);
                        let mut run: Run = serde_json::from_value(raw)?;
                        run.repository_id = repo.id;
                        run.repository_name = repo.full_name.clone();
                        run.is_bot |= is_bot;
                        runs.push(run);
                    }
                    Ok(CachedValue::Runs(runs))
                },
            )
            .await?
        else {
            anyhow::bail!("GitHub 响应格式错误")
        };
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
        state.monitored_repo_ids.clear();
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
        let has_monitored_baseline =
            !state.monitored_repo_ids.is_empty() || !state.snapshot.repositories.is_empty();
        for run in &runs {
            let repo_newly_monitored = has_monitored_baseline
                && !state.monitored_repo_ids.contains(&run.repository_id)
                && !state
                    .snapshot
                    .runs
                    .iter()
                    .any(|existing| existing.repository_id == run.repository_id);
            if baseline
                || repo_newly_monitored
                || run.notification_kind().is_none()
                || !matches_run_filter(run, repositories, filter_bots)
            {
                continue;
            }
            let source_baseline = state
                .snapshot
                .repository_baselines_ms
                .get(&run.repository_id)
                .copied()
                .unwrap_or(0)
                .max(if run.is_bot {
                    state.snapshot.bot_baseline_ms
                } else {
                    0
                });
            let newly_finished = previous.get(&(run.repository_id, run.id)).map_or_else(
                || run.finished_ms() > state.baseline_ms.max(source_baseline),
                |old| {
                    (old.run_attempt != run.run_attempt || old.status != "completed")
                        && (state.snapshot.last_scan_ms >= source_baseline
                            || run.finished_ms() > source_baseline)
                },
            );
            if newly_finished && pending.insert((run.repository_id, run.id, run.run_attempt)) {
                state.pending.push(Event {
                    observed_ms: time,
                    account: account.clone(),
                    run: run.clone(),
                });
            }
        }
        state.monitored_repo_ids.retain(|id| {
            state.snapshot.repositories.iter().any(|repo| {
                repo.id == *id && !repo.archived && matches_repository_filter(repo, repositories)
            })
        });
        state
            .monitored_repo_ids
            .extend(runs.iter().map(|run| run.repository_id));
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
