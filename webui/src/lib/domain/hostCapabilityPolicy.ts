import type { RuntimeCapabilityMatrix } from "./capabilities";

export type HostCapabilityPolicy = {
  showRemoteApiCapabilities: boolean;
  failureLabels: Record<string, string>;
};

const linuxFailureLabels: Record<string, string> = {
  systemd_failure: "systemd 失败",
  permission_denied_sudo: "权限或 sudo 失败"
};

const genericFailureLabels: Record<string, string> = {
  systemd_failure: "服务失败",
  permission_denied_sudo: "权限失败"
};

export function hostCapabilityPolicy(capabilities: RuntimeCapabilityMatrix): HostCapabilityPolicy {
  return capabilities.hostSurface === "linux_server_api"
    ? {
      showRemoteApiCapabilities: true,
      failureLabels: linuxFailureLabels
    }
    : {
      showRemoteApiCapabilities: false,
      failureLabels: genericFailureLabels
    };
}
