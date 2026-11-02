import type { RuntimeCapabilityMatrix } from "./capabilities";

export type HostCapabilityPolicy = {
  showRemoteApiCapabilities: boolean;
  copyRedactionEnabled: boolean;
  failureLabels: Record<string, string>;
};

const linuxFailureLabels: Record<string, string> = {
  systemd_failure: "systemd 失败",
  nginx_failure: "Nginx 失败",
  permission_denied_sudo: "权限或 sudo 失败"
};

const genericFailureLabels: Record<string, string> = {
  systemd_failure: "服务失败",
  nginx_failure: "更新失败",
  permission_denied_sudo: "权限失败"
};

export function hostCapabilityPolicy(capabilities: RuntimeCapabilityMatrix): HostCapabilityPolicy {
  return capabilities.hostSurface === "linux_server_api"
    ? {
      showRemoteApiCapabilities: true,
      copyRedactionEnabled: false,
      failureLabels: linuxFailureLabels
    }
    : {
      showRemoteApiCapabilities: false,
      copyRedactionEnabled: true,
      failureLabels: genericFailureLabels
    };
}

export function redactHostCopy(value: string, fallback: string): string {
  const sanitized = value
    .replace(/https?:\/\/[^\s)"']+/gi, "本机入口")
    .replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/g, "本机地址")
    .replace(/\b(?:[a-z0-9-]+\.)+[a-z]{2,}\b/gi, "本机域名")
    .replace(/\/opt\/agenthub[^\s)"']*/gi, "本机路径")
    .replace(/\/root\/\.codex[^\s)"']*/gi, "本机 Codex 目录")
    .replace(/\/home\/[^/\s]+[^\s)"']*/gi, "本机工作区")
    .replace(/公网入口/g, "本机入口")
    .replace(/\bsystemd\b/gi, "服务")
    .replace(/\bnginx\b/gi, "服务")
    .replace(/Linux prune/gi, "清理")
    .replace(/Linux update/gi, "更新")
    .replace(/\bLinux\b/g, "当前宿主")
    .replace(/\bsudo\b/gi, "权限")
    .trim();
  return /systemd|nginx|Linux prune|Linux update|sudo|\/opt\/agenthub|\/root\/\.codex|\/home\/[^/\s]+/i.test(sanitized)
    ? fallback
    : sanitized || fallback;
}
