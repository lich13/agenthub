#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const contractPath = resolve(root, "contracts/agenthub-contract.json");
const contract = JSON.parse(readFileSync(contractPath, "utf8"));
const requestedActionId = process.argv[2] ?? null;

function actionsForRequest() {
  if (!requestedActionId) {
    return contract.actions;
  }
  const action = contract.actions.find((candidate) => candidate.id === requestedActionId);
  if (!action) {
    console.error(`Unknown contract action: ${requestedActionId}`);
    console.error("Run without an action id to list the global sync workflow.");
    process.exit(1);
  }
  return [action];
}

function dtoLine(action) {
  if (!action.requestDto && !action.responseDto) {
    return "DTO: host-only action; document hostOnlyReason and payload boundary if it gains a request or response shape.";
  }
  const owner = action.dtoOwner ? `owner=${action.dtoOwner}; ` : "";
  return `DTO: ${owner}requestDto=${action.requestDto}; responseDto=${action.responseDto}`;
}

function printGlobalChecklist() {
  console.log("AgentHub contract-driven next action checklist");
  console.log(`Source: ${contractPath}`);
  console.log("");
  console.log("Default order for new or changed functionality:");
  console.log("1. Update contracts/agenthub-contract.json first.");
  console.log("2. Add or update core use-case, DTO, and plan ownership.");
  console.log("3. Update WebUI query/domain/runtime wrappers and visual/capability rules.");
  console.log("4. Add thin Linux RPC and Tauri invoke adapter mappings.");
  console.log("5. Run parity guards, then Browser/Computer Use acceptance for affected surfaces.");
  console.log("");
}

function printActionChecklist(action) {
  console.log(`Action: ${action.id}`);
  console.log(`scope: ${action.scope}`);
  console.log(`kind: ${action.kind}`);
  console.log(`coreUseCase: ${action.coreUseCase}`);
  if (action.linuxRpc) console.log(`linuxRpc: ${action.linuxRpc}`);
  if (action.tauriCommand) console.log(`tauriCommand: ${action.tauriCommand}`);
  if (action.webuiWrapper) console.log(`webuiWrapper: ${action.webuiWrapper}`);
  if (action.hostOnlyReason) console.log(`hostOnlyReason: ${action.hostOnlyReason}`);
  console.log(dtoLine(action));
  console.log("Required sync points:");
  if (action.scope === "shared") {
    console.log("- contract registry: action id, scope=shared, DTO owner, requestDto, responseDto, capability/visual rule if user-facing.");
    console.log("- core use-case/DTO: expose the shared plan through AgentHubUseCases and register DTO names in contract_dtos.");
    console.log("- Linux RPC: keep /api/rpc/:command mapped to the same action id.");
    console.log("- Tauri command: register the typed invoke command and keep bundled-helper behavior thin.");
    console.log("- WebUI wrapper: update query/domain/runtime wrapper and contractDtoMap marker.");
    console.log("- tests: Rust contract guard, Tauri guard, WebUI contract/visual/capability tests, install-script guard.");
    console.log("- acceptance: Installed macOS App for local and remote API targets when the visible surface changes.");
  } else if (action.scope === "transport") {
    console.log("- contract registry: scope=transport, DTO owner, requestDto, responseDto, and explicit wrapper name.");
    console.log("- transport runtime: update upload/event-stream transport without adding ad hoc command strings.");
    console.log("- WebUI wrapper: update domain wrapper and contractDtoMap marker.");
    console.log("- tests: transport allowlist, WebUI wrapper guard, install-script guard.");
  } else {
    console.log("- contract registry: scope=host_only and a concrete hostOnlyReason are mandatory.");
    console.log("- host policy: keep the action behind SystemCapabilities, host surface, or runtime transport.");
    console.log("- adapter: expose only on the owning host surface; do not leak into shared Linux RPC/Tauri parity.");
    console.log("- tests: host capability rendering and forbidden-surface checks.");
  }
  console.log("");
}

printGlobalChecklist();
for (const action of actionsForRequest()) {
  printActionChecklist(action);
}
