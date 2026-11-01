//! Existing cli implementation, split without changing dispatch order.
use super::*;

#[derive(Debug, Parser)]
#[command(
    name = "nexushub-webd",
    version,
    about = "AgentHub management API and background monitor"
)]
pub(super) struct Cli {
    #[arg(long, env = "NEXUSHUB_CONFIG", default_value_os_t = Config::current_default_config_path())]
    pub(super) config: PathBuf,

    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    #[command(hide = true)]
    ClaudeHook,
    #[command(hide = true)]
    GrokHook,
    #[command(hide = true)]
    AntigravityHook,
    Serve {
        #[arg(long, default_value_t = HostSurface::LinuxServerApi)]
        surface: HostSurface,
    },
    Doctor,
    InitConfig,
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    Probe {
        #[command(subcommand)]
        command: ProbeCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum AdminCommand {
    #[command(name = "key-generate")]
    Generate {
        #[arg(long)]
        output: PathBuf,
    },
    #[command(name = "key-rotate")]
    Rotate {
        #[arg(long)]
        output: PathBuf,
    },
    #[command(name = "key-revoke")]
    Revoke,
}

#[derive(Debug, Subcommand)]
pub(super) enum ProbeCommand {
    Status,
    HookStatus,
    Events {
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    Running {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    ReplyNeeded {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Recoverable {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    HookStop {
        #[arg(long)]
        thread_id: Option<String>,
        #[arg(long)]
        turn_id: Option<String>,
        #[arg(long, default_value = "hook-stop")]
        kind: String,
    },
    #[command(hide = true)]
    HookRequestUserInput,
    #[command(hide = true)]
    HookRequestUserInputConfirm,
    HooksInstall {
        #[arg(long)]
        dry_run: bool,
    },
    HooksRemove {
        #[arg(long)]
        dry_run: bool,
    },
    NotifyCompletion {
        #[arg(long)]
        thread_id: Option<String>,
        #[arg(long)]
        turn_id: Option<String>,
    },
    BarkTest,
    LifecycleRepair,
    ServiceRestart,
    #[command(hide = true)]
    LegacyImport,
    #[command(hide = true)]
    MonitorErrors,
    #[command(name = "diagnose-antigravity", hide = true)]
    DiagnoseAntigravity { cascade_id: String },
}
