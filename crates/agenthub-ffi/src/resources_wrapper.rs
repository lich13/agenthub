use std::path::Path;

#[path = "resources_inner.rs"]
mod inner;

pub(crate) fn sync_agenthub_webd_helper_from_resource(resource_dir: &Path) -> Result<bool, String> {
    if crate::isolation::is_isolated_mode() {
        return Ok(false);
    }
    inner::sync_agenthub_webd_helper_from_resource(resource_dir)
}

pub(crate) fn repair_probe_error_monitor_launch_agent(
    config: &agenthub_core::config::Config,
    platform: &agenthub_core::platform::PlatformPaths,
    helper_changed: bool,
) -> Result<agenthub_core::probe_error_monitor::ProbeErrorMonitorLaunchAgentStatus, String> {
    if crate::isolation::is_isolated_mode() {
        return Ok(crate::isolation::skipped_launch_agent_status(
            config, platform,
        ));
    }
    inner::repair_probe_error_monitor_launch_agent(config, platform, helper_changed)
}
