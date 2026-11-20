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
  console.log("2. Add or update core use-case, DTO, and plan ownership in crates/agenthub-core.");
  console.log("3. Update Swift 6 models/stores in macos/Sources/AgentHubCore/ and contracts/wire-types.json.");
  console.log("4. Add thin Linux RPC (agenthub-webd) and FFI dispatch (agenthub-ffi) adapter mappings.");
  console.log("5. Run cargo test, swift test (AGENTHUB_RENDER_TESTS=1), and check-wire-types.mjs.");
  console.log("");
}

function printActionChecklist(action) {
  console.log(`Action: ${action.id}`);
  console.log(`scope: ${action.scope}`);
  console.log(`kind: ${action.kind}`);
  console.log(`coreUseCase: ${action.coreUseCase}`);
  if (action.linuxRpc) console.log(`linuxRpc: ${action.linuxRpc}`);
  if (action.ffiHandler) console.log(`ffiAdapter: ${action.ffiHandler}`);
  if (action.hostOnlyReason) console.log(`hostOnlyReason: ${action.hostOnlyReason}`);
  console.log(dtoLine(action));
  console.log("Required sync points:");
  if (action.scope === "shared") {
    console.log("- contract registry: action id, scope=shared, DTO owner, requestDto, responseDto, capability/visual rule if user-facing.");
    console.log("- core use-case/DTO: expose the shared plan through AgentHubUseCases and register DTO names in contract_dtos.");
    console.log("- Linux RPC: keep /api/rpc/:command mapped to the same action id.");
    console.log("- FFI dispatch: register the typed command in crates/agenthub-ffi/src/commands/dispatch.rs.");
    console.log("- Swift 6 client: update AgentHubCore Models/Stores and run node scripts/check-wire-types.mjs --update.");
    console.log("- tests: Rust wire_contract + drift_guard, Swift XCTest parity & render suites, test-install-script.sh.");
  } else if (action.scope === "transport") {
    console.log("- contract registry: scope=transport, DTO owner, requestDto, responseDto.");
    console.log("- transport runtime: update upload/event-stream transport without adding ad hoc command strings.");
    console.log("- tests: transport allowlist, Swift & FFI stream guards, install-script guard.");
  } else {
    console.log("- contract registry: scope=host_only and a concrete hostOnlyReason are mandatory.");
    console.log("- host policy: keep the action behind SystemCapabilities, host surface, or FFI host boundary.");
    console.log("- adapter: expose only on the owning host surface; do not leak into shared Linux RPC/FFI parity.");
    console.log("- tests: host capability rendering and forbidden-surface checks.");
  }
  console.log("");
}

printGlobalChecklist();
for (const action of actionsForRequest()) {
  printActionChecklist(action);
}
