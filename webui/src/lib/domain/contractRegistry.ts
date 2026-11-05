import registry from "../../../../contracts/agenthub-contract.json";
import type { HostSurface } from "../../types";

export type ContractActionScope = "shared" | "host_only" | "transport";
export type ContractActionKind = "rpc" | "transport";

export type ContractAction = {
  id: string;
  kind: ContractActionKind;
  scope: ContractActionScope;
  coreUseCase?: string;
  linuxRpc?: string;
  tauriCommand?: string;
  webuiWrapper?: string;
  dtoOwner?: string;
  requestDto?: string;
  responseDto?: string;
  hostOnlyReason?: string;
};

export type ContractDtoCatalogEntry = {
  core: string;
  webui: string;
};

export type ContractVisual = {
  navigation: string[];
  corePanelTitles: string[];
  actionLabels: Record<string, string>;
  disabledStates: Record<string, string>;
  remoteApiOnly: string[];
  desktopTauriOnly: string[];
  forbidden: {
    desktopEmbeddedTauri: string[];
  };
};

export type AgentHubContractRegistry = {
  schemaVersion: number;
  hostSurfaces: HostSurface[];
  capabilities: string[];
  capabilitiesByHostSurface: Record<HostSurface, string[]>;
  visual: ContractVisual;
  actions: ContractAction[];
  dtoCatalog: Record<string, ContractDtoCatalogEntry>;
};

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string" && item.trim().length > 0);
}

function assertContractRegistry(value: unknown): asserts value is AgentHubContractRegistry {
  if (!isObject(value)) {
    throw new Error("contract registry must be an object");
  }
  for (const key of ["schemaVersion", "hostSurfaces", "capabilities", "capabilitiesByHostSurface", "visual", "actions", "dtoCatalog"]) {
    if (!(key in value)) {
      throw new Error(`contract registry missing ${key}`);
    }
  }
  if (value.schemaVersion !== 1) {
    throw new Error("contract registry schemaVersion must be 1");
  }
  if (!isStringArray(value.hostSurfaces)) {
    throw new Error("contract registry hostSurfaces must be a non-empty string array");
  }
  if (!isStringArray(value.capabilities)) {
    throw new Error("contract registry capabilities must be a non-empty string array");
  }
  if (!isObject(value.capabilitiesByHostSurface)) {
    throw new Error("contract registry capabilitiesByHostSurface must be an object");
  }
  for (const surface of value.hostSurfaces) {
    if (!isStringArray(value.capabilitiesByHostSurface[surface])) {
      throw new Error(`contract registry missing capabilities for ${surface}`);
    }
  }
  if (!isObject(value.visual) || !isObject(value.visual.forbidden)) {
    throw new Error("contract registry visual rules must be objects");
  }
  for (const key of ["navigation", "corePanelTitles", "remoteApiOnly", "desktopTauriOnly"]) {
    if (!isStringArray(value.visual[key])) {
      throw new Error(`contract registry visual.${key} must be a string array`);
    }
  }
  if (!isObject(value.visual.actionLabels) || !isObject(value.visual.disabledStates)) {
    throw new Error("contract registry visual labels and disabled states must be objects");
  }
  if (!isStringArray(value.visual.forbidden.desktopEmbeddedTauri)) {
    throw new Error("contract registry forbidden visual rules must cover desktop host surfaces");
  }
  if (!Array.isArray(value.actions)) {
    throw new Error("contract registry actions must be an array");
  }
  if (!isObject(value.dtoCatalog)) {
    throw new Error("contract registry dtoCatalog must be an object");
  }
  for (const [name, entry] of Object.entries(value.dtoCatalog)) {
    if (!isObject(entry)) {
      throw new Error(`contract registry dtoCatalog.${name} must be an object`);
    }
    for (const key of ["core", "webui"]) {
      if (typeof entry[key] !== "string" || entry[key].trim().length === 0) {
        throw new Error(`contract registry dtoCatalog.${name}.${key} must be a non-empty string`);
      }
    }
  }
  for (const action of value.actions) {
    if (!isObject(action)) {
      throw new Error("contract registry action must be an object");
    }
    const id = action.id;
    if (typeof id !== "string" || id.trim().length === 0) {
      throw new Error("contract registry action id must be a non-empty string");
    }
    if (action.kind !== "rpc" && action.kind !== "transport") {
      throw new Error(`contract registry action ${id} has unsupported kind`);
    }
    if (action.scope !== "shared" && action.scope !== "host_only" && action.scope !== "transport") {
      throw new Error(`contract registry action ${id} has unsupported scope`);
    }
    if (typeof action.coreUseCase !== "string" || action.coreUseCase.trim().length === 0) {
      throw new Error(`contract registry action ${id} must declare coreUseCase`);
    }
    if (action.scope === "shared") {
      for (const key of ["linuxRpc", "tauriCommand", "webuiWrapper", "dtoOwner", "requestDto", "responseDto"]) {
        if (typeof action[key] !== "string" || action[key].trim().length === 0) {
          throw new Error(`shared contract action ${id} must declare ${key}`);
        }
      }
    }
    if (action.scope === "transport") {
      for (const key of ["webuiWrapper", "dtoOwner", "requestDto", "responseDto"]) {
        if (typeof action[key] !== "string" || action[key].trim().length === 0) {
          throw new Error(`transport contract action ${id} must declare ${key}`);
        }
      }
    }
    for (const key of ["requestDto", "responseDto"]) {
      const dtoName = action[key];
      if (typeof dtoName === "string" && !(dtoName in value.dtoCatalog)) {
        throw new Error(`contract registry action ${id} references unknown ${key} ${dtoName}`);
      }
    }
    if (action.scope === "host_only" && (typeof action.hostOnlyReason !== "string" || action.hostOnlyReason.trim().length === 0)) {
      throw new Error(`host-only contract action ${id} must declare hostOnlyReason`);
    }
  }
}

assertContractRegistry(registry);

export const contractRegistry = registry;

export const contractHostSurfaces = contractRegistry.hostSurfaces;
export const contractCapabilities = contractRegistry.capabilities;
export const contractCapabilitiesByHostSurface = contractRegistry.capabilitiesByHostSurface;
export const contractVisual = contractRegistry.visual;
export const contractActions = contractRegistry.actions;
export const contractDtoCatalog = contractRegistry.dtoCatalog;
export const sharedContractActions = contractActions.filter((action) => action.scope === "shared");
export const hostOnlyContractActions = contractActions.filter((action) => action.scope === "host_only");
