use agenthub_core::services::commands as rpc_commands;

#[cfg(test)]
pub(crate) const RPC_COMMAND_ROUTE: &str = "/api/rpc/:command";

pub(crate) fn is_business_rpc_command(command: &str) -> bool {
    rpc_commands::is_allowed_rpc_command(command)
}

pub(crate) fn is_transport_rpc_command(command: &str) -> bool {
    rpc_commands::ALLOWED_TRANSPORT_COMMANDS.contains(&command)
}

pub(crate) fn is_retired_rpc_command(command: &str) -> bool {
    rpc_commands::is_retired_command(command)
}

#[cfg(test)]
mod tests {
    use super::{
        is_business_rpc_command, is_retired_rpc_command, is_transport_rpc_command,
        RPC_COMMAND_ROUTE,
    };
    use agenthub_core::services::commands as rpc_commands;

    #[test]
    fn routes_keep_transport_surface_explicit() {
        assert!(is_retired_rpc_command("threadEvents"));
        assert!(!is_transport_rpc_command("threadEvents"));
        assert!(!is_transport_rpc_command("uploadFiles"));
        assert!(is_retired_rpc_command("uploadFiles"));
        assert_eq!(RPC_COMMAND_ROUTE, "/api/rpc/:command");
    }

    #[test]
    fn business_transport_and_retired_command_sets_stay_disjoint() {
        for command in rpc_commands::ALLOWED_RPC_COMMANDS {
            assert!(is_business_rpc_command(command));
            assert!(!is_transport_rpc_command(command));
            assert!(!is_retired_rpc_command(command));
        }

        for command in rpc_commands::ALLOWED_TRANSPORT_COMMANDS {
            assert!(is_transport_rpc_command(command));
            assert!(!is_business_rpc_command(command));
        }

        for command in rpc_commands::RETIRED_COMMANDS {
            assert!(is_retired_rpc_command(command));
            assert!(!is_business_rpc_command(command));
        }
    }

    #[test]
    fn required_transport_and_retired_exceptions_stay_out_of_business_allowlist() {
        for command in [
            "threadEvents",
            "auth.login",
            "auth.logout",
            "auth.me",
            "security.get",
            "startProbeJob",
            "runUpdateAction",
            "getDesktopOverview",
            "getDesktopHome",
        ] {
            assert!(
                is_retired_rpc_command(command),
                "retired compatibility command must stay marked retired: {command}"
            );
            assert!(
                !is_business_rpc_command(command),
                "retired compatibility command must not re-enter business allowlist: {command}"
            );
            assert!(
                !is_transport_rpc_command(command),
                "retired compatibility command must not become a transport exception: {command}"
            );
        }
    }
}
