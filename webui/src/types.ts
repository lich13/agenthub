export type TimelineEntry = { id: string; title: string; preview?: string };
export type HistoryMetadata = { epoch: string; revision: string; changedIds?: string[]; reset?: boolean; changes?: { revision: string; changedIds: string[]; reset?: boolean }[] };

export type UserAttachment = { id: string; name: string; kind: "image" | "file"; path?: string | null; reason?: string | null };
export type UserContextBlock = { kind: "instructions" | "environment" | "runtime" | "files"; text: string };
export type UserMessageContent = { id: string; text: string; attachments: UserAttachment[]; contextBlocks?: UserContextBlock[] };

export type ThreadStatus = "Recent" | "Running" | "ReplyNeeded" | "Recoverable" | "Archived";

export type SessionStorageSize = { bytes: number | null; scope: "file" | "directory"; status: "complete" | "partial" | "pending" | "unavailable" };

export type GrokSessionSummary = {
  deletionEligibility?: { allowed: boolean; reason: string | null } | null;
  storageSize?: SessionStorageSize | null;
  handoffPath?: string | null;
  id: string;
  title: string;
  cwd: string;
  path: string;
  updatedAt?: string | null;
  messageCount: number;
  lastMessage?: string | null;
  status: string;
};

export type GrokHistoryEvent = {
  id?: string;
  userMessage?: UserMessageContent | null;
  callId?: string;
  status?: string;
  detail?: string;
  timestamp?: string | null;
  kind: string;
  text?: string | null;
  method?: string | null;
};

export type GrokSessionDetail = { timeline?: TimelineEntry[]; afterCursor?: string | null; history?: HistoryMetadata | null; totalEvents?: number; hasMore?: boolean; beforeCursor?: string | null; summary: GrokSessionSummary; events: GrokHistoryEvent[] };
export type GrokDeletePreview = { id: string; title: string; path: string; fingerprint: string; fileCount: number; bytes: number };
export type GrokDeleteRequest = { id: string; confirmed: boolean; fingerprint?: string };
export type GrokDeleteResult = { id: string; deleted: boolean; bytes: number; residuals?: string[] };

export type ClaudeSessionSummary = {
  storageSize?: SessionStorageSize | null;
  id: string;
  sessionKey: string;
  title: string;
  cwd: string;
  path: string;
  updatedAt?: string | null;
  messageCount: number;
  lastMessage?: string | null;
  status: string;
  formatVersion: string;
  readWarning?: string | null;
  canRename: boolean;
  renameBlockReason?: string | null;
  canDelete: boolean;
  deleteBlockReason?: string | null;
  readError?: string | null;
};
export type ClaudeHistoryEvent = { id: string; turnId?: string | null; result?: string | null; userMessage?: UserMessageContent | null; timestamp?: string | null; kind: string; role?: string | null; text?: string | null; callId?: string | null; status?: string | null; detail?: string | null; assistantPhase?: "intermediate" | "final" | null };
export type ClaudeDetailRequest = { sessionKey: string; limit?: number; before?: string };
export type ClaudeSessionDetail = { timeline?: TimelineEntry[]; afterCursor?: string | null; history?: HistoryMetadata | null; summary: ClaudeSessionSummary; events: ClaudeHistoryEvent[]; totalEvents: number; hasMore: boolean; beforeCursor?: string | null };
export type ClaudeDeletePreview = { sessionKey: string; id: string; title: string; path: string; paths?: string[]; fingerprint: string; fileCount: number; bytes: number; warnings?: string[] | null };
export type ClaudeOwnershipProcess = { pid: number; role: string; registrationVerified: boolean; mismatchedFields: string[] };
export type ClaudeOwnershipDiagnostics = { ownership: string; reasonCodes: string[]; processes: ClaudeOwnershipProcess[] };
export type ClaudeDeleteRequest = { sessionKey: string; confirmed: boolean; fingerprint?: string };
export type ClaudeDeleteResult = { sessionKey: string; deleted: boolean; bytes: number; residuals?: string[] };

export type ProviderListRequest = { q?: string; archived?: boolean; limit?: number };
export type ProviderDetailRequest = { sessionKey: string; limit?: number; before?: string };
export type ProviderRenameRequest = { sessionKey: string; title: string };
export type ProviderSessionRequest = { sessionKey: string };
export type ProviderSessionSummary = {
  storageSize?: SessionStorageSize | null;
  id: string;
  sessionKey: string;
  title: string;
  cwd: string;
  path: string;
  archived: boolean;
  updatedAt?: string | null;
  messageCount: number;
  lastMessage?: string | null;
  status: string;
  formatVersion: string;
  canRename: boolean;
  renameBlockReason?: string | null;
  canArchive: boolean;
  archiveBlockReason?: string | null;
  canDelete: boolean;
  deleteBlockReason?: string | null;
  readError?: string | null;
  readWarning?: string | null;
};
export type ProviderHistoryEvent = {
  id: string;
  timestamp?: string | null;
  kind: string;
  role?: string | null;
  text?: string | null;
  callId?: string | null;
  turnId?: string | null;
  status?: string | null;
  detail?: string | null;
  result?: string | null;
  userMessage?: UserMessageContent | null;
};
export type ProviderSessionDetail = {
  timeline?: TimelineEntry[];
  afterCursor?: string | null;
  history?: HistoryMetadata | null;
  summary: ProviderSessionSummary;
  events: ProviderHistoryEvent[];
  totalEvents: number;
  hasMore: boolean;
  beforeCursor?: string | null;
};
export type ProviderListResponse = ProviderSessionSummary[];
export type ProviderDetailResponse = ProviderSessionDetail;
export type ProviderMutationResponse = ProviderSessionSummary;



export type ThreadSummary = {
  storageSize?: SessionStorageSize | null;
  id: string;
  title: string;
  status: ThreadStatus;
  updated_at?: string | null;
  archived_at?: string | null;
  message_count: number;
  latest_message?: string | null;
  cwd?: string | null;
  model?: string | null;
  rollout_path?: string | null;
  active_turn_id?: string | null;
  active_job_id?: string | null;
  pending_elicitation?: PendingElicitation | null;
  last_event_kind?: string | null;
  thread_source?: string | null;
  threadSource?: string | null;
  source_kind?: string | null;
  sourceKind?: string | null;
  parent_thread_id?: string | null;
  parentThreadId?: string | null;
  source?: unknown;
  agent_nickname?: string | null;
  agentNickname?: string | null;
  agent_role?: string | null;
  agentRole?: string | null;
  agent_path?: string | null;
  agentPath?: string | null;
  has_user_event?: number | boolean | null;
  hasUserEvent?: number | boolean | null;
  first_user_message?: string | null;
  firstUserMessage?: string | null;
  preview?: string | null;
};

export type CodexMessage = {
  role: string;
  kind: string;
  text: string;
  created_at?: string | null;
};

export type UserInputOption = {
  label: string;
  description?: string | null;
};

export type UserInputQuestion = {
  id: string;
  header?: string | null;
  question: string;
  options: UserInputOption[];
};

export type PendingElicitation = {
  turn_id?: string | null;
  item_id?: string | null;
  questions: UserInputQuestion[];
};

export type MessageBlock = {
  subagent?: SubagentActivity | null;
  user_message?: UserMessageContent | null;
  id: string;
  role: string;
  kind: string;
  display_kind?: string | null;
  status?: string | null;
  text?: string | null;
  summary?: string | null;
  input?: string | null;
  truncated?: boolean | null;
  resolved?: boolean | null;
  answers?: UserInputAnswer[];
  plan_status?: string | null;
  group_id?: string | null;
  tool_name?: string | null;
  call_id?: string | null;
  turn_id?: string | null;
  item_id?: string | null;
  created_at?: string | null;
  questions: UserInputQuestion[];
  payload?: unknown;
};

export type SubagentActivity = {
  eventKind?: "started" | "completed" | "interrupted" | "interacted" | null;
  eventId?: string | null;
  agentId?: string | null;
  name: string;
  role?: string | null;
  status: "creating" | "running" | "completed" | "failed" | "interrupted" | "unknown";
  available: boolean;
  unavailableReason?: string | null;
  delegation?: string | null;
};

export type SubagentCollection = {
  agents: SubagentActivity[];
  counts: Record<SubagentActivity["status"], number>;
  complete: boolean;
  warning?: string | null;
};

export type SubagentDetailRequest = {
  rootThreadId: string;
  agentId: string;
  limit?: number;
  before?: string | null;
};

export type SubagentDetailResponse = {
  rootThreadId: string;
  parentThreadId: string;
  agent: SubagentActivity;
  detail: ThreadDetail;
};

export type UserInputAnswer = {
  question_id: string;
  answers: string[];
  note?: string | null;
};

export type ThreadDetail = {
  timeline?: TimelineEntry[];
  after_cursor?: string | null;
  history?: HistoryMetadata | null;
  subagents?: SubagentCollection | null;
  subagent_updates?: Record<string, SubagentActivity>;
  summary: ThreadSummary;
  messages: CodexMessage[];
  blocks: MessageBlock[];
  raw_event_count: number;
  total_blocks?: number;
  has_more_blocks?: boolean;
  before_cursor?: string | null;
};

export type ThreadBlockPage = {
  history?: HistoryMetadata | null;
  thread_id: string;
  blocks: MessageBlock[];
  total_blocks: number;
  has_more_blocks: boolean;
  before_cursor?: string | null;
};

export type SearchProvider = "codex" | "claude_code" | "grok" | "antigravity" | "dsh";
export type SearchScope = "thread" | "provider";
export type SessionSearchRequest = {
  provider: SearchProvider;
  scope: SearchScope;
  sessionKey?: string | null;
  query: string;
  cursor?: string | null;
  limit?: number;
};
export type SessionSearchResult = {
  resultId: string;
  sessionKey: string;
  nativeId: string;
  title: string;
  cwd?: string | null;
  matchKind: string;
  positionKey: string;
  snippet: string;
  timestamp?: string | null;
};
export type SessionSearchResponse = {
  provider: SearchProvider;
  scope: SearchScope;
  query: string;
  results: SessionSearchResult[];
  nextCursor?: string | null;
  truncated: boolean;
  warnings?: string[];
};





export type HostSurface = "linux_server_api" | "desktop_embedded_tauri";

export type SystemCapabilities = {
  threads: boolean;
  jobs: boolean;
  probe: boolean;
  settings: boolean;
  job_history: boolean;
  app_updater: boolean;
  systemd: boolean;
  linux_update_job: boolean;
  prune_backups: boolean;
  thread_cleanup?: boolean;
  hidden_cleanup_scan?: boolean;
  dsh_archive_cleanup?: boolean;
  antigravity_notifications?: boolean;
  dsh_notifications?: boolean;
  antigravity_sessions?: boolean;
  dsh_sessions?: boolean;
  codex_delete_tree?: boolean;
  codex_desktop?: boolean;
  thread_archive_actions?: boolean;
  thread_subagents?: boolean;
};

export type SystemCapabilitiesResponse = {
  api_version: number;
  host_surface: HostSurface;
  capabilities: SystemCapabilities;
};

export type SystemVersion = {
  panel_current: string;
  panel_latest?: string | null;
  panel_update_available?: boolean | null;
  codex_current?: string | null;
  codex_latest?: string | null;
  codex_update_available?: boolean | null;
  codex_user?: string | null;
  codex_root?: string | null;
  codex_raw?: string | null;
};

export type UpdateExecutionMethod = "linux_systemd_job" | "macos_tauri_updater" | "unsupported";
export type UpdateState = "idle" | "checking" | "ready" | "installing" | "succeeded" | "failed" | "unsupported";

export type UpdateStatus = {
  current_version: string;
  latest_version?: string | null;
  update_available?: boolean | null;
  channel: string;
  method: UpdateExecutionMethod;
  state: UpdateState;
  failure_category?: string | null;
  recommended_action: string;
  capabilities: string[];
};


export type JobRecord = {
  id: string;
  kind: string;
  status: string;
  title: string;
  thread_id?: string | null;
  turn_id?: string | null;
  started_at: number;
  finished_at?: number | null;
  exit_code?: number | null;
  output: string;
  error?: string | null;
  analysis?: string | null;
  explanation?: string | null;
  failure_analysis?: {
    category: string;
    explanation: string;
    suggestions: string[];
  } | null;
};

export type ArchiveDeletePlan = {
  total_threads: number;
  active_threads: number;
  archived_threads: number;
  session_index_lines: number;
  rollout_files: number;
  archived_ids: string[];
  integrity: string;
};

export type ArchiveDeleteResult = {
  items?: HiddenThreadItemResult[];
  before: ArchiveDeletePlan;
  after_total_threads: number;
  after_active_threads: number;
  after_archived_threads: number;
  after_integrity: string;
  deleted_rollout_files: number;
};

export type HiddenThreadSelection = { id: string; fingerprint?: string | null };
export type HiddenThreadCandidate = HiddenThreadSelection & { title: string; allowed: boolean; reason: string | null };
export type HiddenThreadItemResult = { id: string; status: "deleted" | "skipped" | "failed"; reason: string | null; residuals?: string[] };
export type CleanupExecuteRequest = { confirmed: boolean; expectedCount?: number; candidates?: HiddenThreadSelection[] };

export type HiddenThreadDeletePlan = {
  scan?: HiddenScanProgress | null;
  total_threads: number;
  visible_threads: number;
  hidden_threads: number;
  archived_threads: number;
  session_index_lines: number;
  rollout_files: number;
  hidden_ids: string[];
  candidates: HiddenThreadCandidate[];
  hidden_source_counts: Record<string, number>;
  integrity: string;
};

export type HiddenScanRequest = { mode?: "start" | "poll" | "cancel"; scanId?: string; cursor?: number; limit?: number };
export type HiddenScanProgress = {
  id: string; phase: "preparing" | "scanning" | "complete" | "stopping" | "stopped" | "failed";
  processed: number; total: number; cursor: number; nextCursor: number; hasMore: boolean; error: string | null;
};

export type HiddenThreadDeleteResult = {
  skipped_threads: number;
  failed_threads: number;
  items: HiddenThreadItemResult[];
  before: HiddenThreadDeletePlan;
  deleted_threads: number;
  after_total_threads: number;
  after_visible_threads: number;
  after_hidden_threads: number;
  after_archived_threads: number;
  after_integrity: string;
  visible_threads: number;
  hidden_threads: number;
  integrity: string;
  deleted_rollout_files: number;
};

export type OptionalResult<T> = {
  available: boolean;
  data?: T;
  error?: string;
  reason?: string | null;
};

export type AgentProviderInfo = {
  id: "codex" | "grok_build" | "cursor" | "gemini" | string;
  label: string;
  status: "ready" | "preview" | "planned" | string;
  description?: string;
  capabilities?: string[];
  safety?: string;
};

export type PlatformOverview = {
  kind: "linux" | "macos" | "windows" | string;
  data_dir: string;
  config_file: string;
  log_dir: string;
  service_name: string;
  service_kind: string;
};

export type PluginInfo = {
  id: string;
  label: string;
  status: "ready" | "preview" | "planned" | string;
  kind: "builtin" | "external" | string;
  description?: string | null;
  unavailable_reason?: string | null;
  invocation_template?: string | null;
};

export type ProbeLiveSession = {
  provider: "codex" | "claude_code" | "grok" | "antigravity" | "dsh" | string;
  id: string;
  session_key: string;
  title: string;
  bucket: "running" | "reply_needed" | "recoverable" | string;
  updated_at?: string | null;
  cwd?: string | null;
};

export type ProbeStatus = {
  provider_notifications?: { provider: "grok" | "claude_code"; enabled: boolean; last_scan_at: number; streams: number; read_errors: number; failed_deliveries: number; pending_deliveries: number; failure_supported: boolean }[];
  label?: string | null;
  enabled: boolean;
  available?: boolean | null;
  platform: "linux" | "macos" | "windows" | string;
  service_kind: string;
  service_name: string;
  flavor?: string | null;
  hook_status: string;
  hook_command?: string | null;
  actual_commands?: string[] | null;
  stale_command_count?: number | null;
  empty_group_count?: number | null;
  bark_status: string;
  recent_event_count: number;
  running_count: number;
  reply_needed_count: number;
  recoverable_count: number;
  running_threads?: ThreadSummary[];
  reply_needed_threads?: ThreadSummary[];
  recoverable_threads?: ThreadSummary[];
  active_sessions?: ProbeLiveSession[];
  config_path: string;
  lifecycle_status?: string | null;
  doctor_status?: string | null;
  runtime_version?: string | null;
  codex_home?: string | null;
  codex_hooks_path?: string | null;
  dsh_sessions_path?: string | null;
  configured_codex_home?: string | null;
  resolved_codex_home?: string | null;
  codex_home_source?: string | null;
  logs_db_source?: string | null;
  discovery_warnings?: string[] | null;
  host_label?: string | null;
  snapshot_age_seconds?: number | null;
  is_refreshing?: boolean | null;
  snapshot_status?: string | null;
  error_monitor_enabled?: boolean | null;
  error_monitor_status?: string | null;
  error_monitor_last_scan_at?: string | number | null;
  error_monitor_last_error?: string | null;
  error_monitor_incident_count?: number | null;
};

export type ProbeEvent = {
  id: string;
  kind: string;
  thread_id?: string | null;
  title?: string | null;
  message?: string | null;
  dedupe_key?: string | null;
  source: string;
  payload: Record<string, unknown>;
  created_at: string | number;
  handled_at?: string | number | null;
};

export type ProbeEventsResponse = {
  events: ProbeEvent[];
  limit?: number | null;
};

export type ProbeJobAction = "bark-test" | "hooks-install" | "hooks-remove";

export type ProbeSettings = {
  codex: {
    home?: string | null;
    configured_codex_home?: string | null;
    resolved_codex_home?: string | null;
    codex_home_source?: string | null;
    logs_db_source?: string | null;
    discovery_warnings?: string[] | null;
    workspace?: string | null;
    host_label: string;
  };
  probe: Record<string, unknown> & {
    enabled?: boolean;
    poll_seconds?: number;
    recent_limit?: number;
    hooks?: Record<string, unknown> & {
      manage_stop_hook?: boolean;
    };
    error_monitor?: Record<string, unknown> & {
      enabled?: boolean;
    };
    notifications?: Record<string, unknown> & {
      enabled?: boolean;
      server_url?: string;
      sound?: string | null;
      group?: string;
      url?: string | null;
      notify_completion?: boolean;
      notify_reply_needed?: boolean;
      notify_recoverable?: boolean;
      notify_codex?: boolean;
      notify_grok?: boolean;
      notify_grok_completion?: boolean;
      notify_grok_failure?: boolean;
      notify_grok_reply_needed?: boolean;
    notify_claude?: boolean;
    notify_claude_completion?: boolean;
    notify_claude_failure?: boolean;
    notify_claude_reply_needed?: boolean;
    notify_antigravity?: boolean;
    notify_antigravity_completion?: boolean;
    notify_antigravity_failure?: boolean;
    notify_antigravity_reply_needed?: boolean;
    notify_antigravity_authorization?: boolean;
    notify_dsh?: boolean;
    notify_dsh_completion?: boolean;
    notify_dsh_failure?: boolean;
    notify_dsh_reply_needed?: boolean;
    notify_dsh_authorization?: boolean;


    };
    observability?: Record<string, unknown> & {
      event_retention_days?: number;
      hook_event_max_lines?: number;
      hook_cooldown_max_lines?: number;
      log_max_bytes?: number;
    };
  };
  discovery_warnings?: string[] | null;
  notifications: Record<string, unknown> & {
    enabled?: boolean;
    device_key?: string;
    device_key_configured?: boolean;
    server_url?: string;
    sound?: string | null;
    group?: string;
    url?: string | null;
    notify_completion?: boolean;
    notify_reply_needed?: boolean;
    notify_recoverable?: boolean;
      notify_codex?: boolean;
      notify_grok?: boolean;
      notify_grok_completion?: boolean;
      notify_grok_failure?: boolean;
      notify_grok_reply_needed?: boolean;
    notify_claude?: boolean;
    notify_claude_completion?: boolean;
    notify_claude_failure?: boolean;
    notify_claude_reply_needed?: boolean;
    notify_antigravity?: boolean;
    notify_antigravity_completion?: boolean;
    notify_antigravity_failure?: boolean;
    notify_antigravity_reply_needed?: boolean;
    notify_antigravity_authorization?: boolean;
    notify_dsh?: boolean;
    notify_dsh_completion?: boolean;
    notify_dsh_failure?: boolean;
    notify_dsh_reply_needed?: boolean;
    notify_dsh_authorization?: boolean;


  };
};




export type CodexConfig = {
  model?: string | null;
  service_tier?: string | null;
  reasoning_effort?: string | null;
  cwd?: string | null;
  permission_profile?: string | null;
  approval_policy?: string | null;
  sandbox_mode?: string | null;
  network_access?: boolean | null;
  collaboration_mode?: string | null;
};

export type { SshHostList, RemoteMachineSummary, RemoteConnectionView, RemoteConnectionCredentials, RemoteRevisionRequest, RemoteSelectionRequest, RemoteInvokeRequest, RemoteInvokeResponse } from "./lib/runtime";

export type ClaudeHooksAction = "status" | "install" | "remove";
export type ClaudeHooksRequest = { action: ClaudeHooksAction; confirmed?: boolean };
export type ClaudeHooksStatus = { installed: boolean; managedCount: number; changed: boolean; configPath?: string };

export type DshArchiveSelection = { id: string; archiveId: string };
export type DshArchiveCandidate = DshArchiveSelection & { parentId: string | null; reason: string | null };
export type DshArchiveDeletePlan = { available: boolean; archivedThreads: number; candidates: DshArchiveCandidate[]; issues: string[] };
export type DshArchiveDeleteRequest = { confirmed: boolean; targets: DshArchiveSelection[] };
export type DshArchiveDeleteResult = { items: HiddenThreadItemResult[]; deletedThreads: number; deletedBytes: number };

export type UnifiedCleanupDryRunRequest = { scopes?: string[]; includeCodexHidden?: boolean };
export type UnifiedCleanupCandidate = {
  id: string;
  title: string;
  estimatedBytes: number;
  fingerprint?: string | null;
  blockedReason: string | null;
};
export type UnifiedCleanupTargetSelection = {
  scope: string;
  id: string;
  fingerprint?: string | null;
};
export type UnifiedCleanupScopePlan = {
  scope: string;
  provider: string;
  label: string;
  supported: boolean;
  scanned: boolean;
  unavailableReason: string | null;
  candidateCount: number;
  deletableCount: number;
  estimatedBytes: number;
  candidates: UnifiedCleanupCandidate[];
  issues: string[];
};
export type UnifiedCleanupPlan = {
  totalCandidates: number;
  totalDeletable: number;
  totalEstimatedBytes: number;
  scopes: UnifiedCleanupScopePlan[];
};
export type UnifiedCleanupExecuteRequest = {
  confirmed: boolean;
  scopes?: string[];
  targets?: UnifiedCleanupTargetSelection[];
};
export type UnifiedCleanupScopeResult = {
  scope: string;
  provider: string;
  label: string;
  deletedCount: number;
  skippedCount: number;
  failedCount: number;
  deletedBytes: number;
  residuals: string[];
  issues: string[];
};
export type UnifiedCleanupResult = {
  deletedCount: number;
  skippedCount: number;
  failedCount: number;
  deletedBytes: number;
  stoppedEarly: boolean;
  scopes: UnifiedCleanupScopeResult[];
};
