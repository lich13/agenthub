import { API } from "typescript/unstable/sync";
import * as ts from "typescript/unstable/ast";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import {
  contractDtoCatalog,
  contractHostSurfaces,
  contractVisual,
  contractCapabilitiesByHostSurface,
  sharedContractActions
} from "./contractRegistry";
import { contractDtoNameSet, contractDtoNames } from "./contractDtoMap";
import {
  desktopTauriOnlyVisualSurfaces,
  remoteApiOnlyVisualSurfaces,
  macosForbiddenVisualSurfaces,
  sharedActionLabels,
  sharedCorePanelTitles,
  sharedDisabledStates,
  sharedNavigationLabels
} from "./visualContract";

type ContractAction = {
  id: string;
  kind: "rpc" | "transport";
  scope: "shared" | "host_only" | "transport";
  linuxRpc?: string;
  tauriCommand?: string;
  webuiWrapper?: string;
  dtoOwner?: string;
  requestDto?: string;
  responseDto?: string;
  hostOnlyReason?: string;
};

type AgentHubContract = {
  hostSurfaces: string[];
  visual: {
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
  actions: ContractAction[];
  dtoCatalog: Record<string, { core: string; webui: string }>;
};

function contract(): AgentHubContract {
  const url = new URL("../../../../contracts/agenthub-contract.json", import.meta.url);
  return JSON.parse(readFileSync(fileURLToPath(url), "utf8")) as AgentHubContract;
}

function repositoryFile(path: string): string {
  const url = new URL(`../../../../${path}`, import.meta.url);
  return readFileSync(fileURLToPath(url), "utf8");
}

const apiSources = import.meta.glob([
  "../api/**/*.ts",
  "../runtime.ts",
  "!../api/**/*.test.ts"
], {
  eager: true,
  query: "?raw",
  import: "default"
}) as Record<string, string>;

function apiCommandLiterals(): Set<string> {
  const out = new Set<string>();
  const wrappers = new Set(["callCommand", "invokeNative", "runtimeRpc", "changeConnection", "startProbeCommand", "runTypedUpdateCommand"]);
  const cwd = fileURLToPath(new URL("../../../", import.meta.url));
  const configPath = fileURLToPath(new URL("../../../tsconfig.app.json", import.meta.url));
  const api = new API({ cwd });
  let snapshot: ReturnType<typeof api.updateSnapshot> | undefined;
  try {
    snapshot = api.updateSnapshot({ openProjects: [configPath] });
    const project = snapshot.getProject(configPath);
    if (!project) throw new Error(`TypeScript API did not open project ${configPath}`);
    for (const path of Object.keys(apiSources)) {
      const sourcePath = fileURLToPath(new URL(path, import.meta.url));
      const sourceFile = project.program.getSourceFile(sourcePath);
      if (!sourceFile) throw new Error(`TypeScript project did not include source ${sourcePath}`);
      const visit = (node: ts.Node) => {
        if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && wrappers.has(node.expression.text)) {
          const command = node.arguments[0];
          if (command && ts.isStringLiteral(command)) out.add(command.text);
        }
        node.forEachChild(visit);
      };
      visit(sourceFile);
    }
    return out;
  } finally {
    try {
      snapshot?.dispose();
    } finally {
      api.close();
    }
  }
}

describe("contract registry", () => {
  test("has a schema and architecture audit that describe the supported parity boundary", () => {
    const schema = JSON.parse(repositoryFile("contracts/agenthub-contract.schema.json")) as {
      $id?: string;
      required?: string[];
      properties?: Record<string, unknown>;
    };
    expect(schema.$id).toBe("https://github.com/lich13/agenthub/contracts/agenthub-contract.schema.json");
    expect(schema.required).toEqual([
      "schemaVersion",
      "hostSurfaces",
      "capabilities",
      "capabilitiesByHostSurface",
      "visual",
      "actions",
      "retiredActions",
      "dtoCatalog"
    ]);
    expect(Object.keys(schema.properties ?? {})).toEqual([
      "retiredActions",
      "schemaVersion",
      "hostSurfaces",
      "capabilities",
      "capabilitiesByHostSurface",
      "visual",
      "actions",
      "dtoCatalog"
    ]);

    const audit = repositoryFile("docs/ARCHITECTURE.md");
    for (const required of [
      "macOS ARM64",
      "Linux x86_64",
      "agenthub-webd-linux-x86_64.tar.gz",
      "darwin-aarch64",
      "contracts/agenthub-contract.json"
    ]) {
      expect(audit, `audit missing ${required}`).toContain(required);
    }
  });

  test("exports registry-derived host surfaces, visual copy, and capabilities without test-only file reads", () => {
    expect(contractHostSurfaces).toEqual(contract().hostSurfaces);
    expect(contractVisual.navigation).toEqual(contract().visual.navigation);
    expect(contractVisual.corePanelTitles).toEqual(contract().visual.corePanelTitles);
    expect(contractVisual.actionLabels).toEqual(contract().visual.actionLabels);
    expect(contractVisual.disabledStates).toEqual(contract().visual.disabledStates);
    expect(contractDtoCatalog).toEqual(contract().dtoCatalog);
    expect(contractCapabilitiesByHostSurface.desktop_embedded_tauri).toEqual(
      contract().capabilitiesByHostSurface.desktop_embedded_tauri
    );
    expect(sharedContractActions.map((action) => action.id)).toEqual(
      contract().actions.filter((action) => action.scope === "shared").map((action) => action.id)
    );
  });

  test("shared and host-only action declarations are complete enough to prevent one-sided adapters", () => {
    for (const action of contract().actions) {
      if (action.scope === "shared") {
        expect(action.coreUseCase, `${action.id} coreUseCase`).toBeTruthy();
        expect(action.linuxRpc, `${action.id} linuxRpc`).toBeTruthy();
        expect(action.tauriCommand, `${action.id} tauriCommand`).toBeTruthy();
        expect(action.webuiWrapper, `${action.id} webuiWrapper`).toBeTruthy();
        expect(action.dtoOwner, `${action.id} dtoOwner`).toBeTruthy();
        expect(action.requestDto, `${action.id} requestDto`).toBeTruthy();
        expect(action.responseDto, `${action.id} responseDto`).toBeTruthy();
      }
      if (action.scope === "transport") {
        expect(action.webuiWrapper, `${action.id} webuiWrapper`).toBeTruthy();
        expect(action.dtoOwner, `${action.id} dtoOwner`).toBeTruthy();
        expect(action.requestDto, `${action.id} requestDto`).toBeTruthy();
        expect(action.responseDto, `${action.id} responseDto`).toBeTruthy();
      }
      if (action.scope === "host_only") {
        expect(action.hostOnlyReason, `${action.id} hostOnlyReason`).toBeTruthy();
      }
    }
  });

  test("shared and transport DTO declarations resolve through the WebUI DTO marker map", () => {
    const dtoCatalog = contract().dtoCatalog;
    expect(new Set(contractDtoNames)).toEqual(new Set(Object.values(dtoCatalog).map((entry) => entry.webui)));
    for (const action of contract().actions) {
      if (action.scope !== "shared" && action.scope !== "transport") continue;
      for (const key of ["requestDto", "responseDto"] as const) {
        const dtoName = action[key];
        expect(dtoName, `${action.id} ${key}`).toBeTruthy();
        expect(dtoCatalog[dtoName ?? ""], `${action.id} ${key} catalog entry`).toBeTruthy();
        expect(contractDtoNameSet.has(dtoCatalog[dtoName ?? ""].webui), `${action.id} ${key} WebUI DTO marker`).toBe(true);
      }
    }
  });

  test("declares the shared visual vocabulary used by both WebUI and Tauri shells", () => {
    const visual = contract().visual;
    expect(visual.navigation).toEqual([...sharedNavigationLabels]);
    expect(visual.corePanelTitles).toEqual([...sharedCorePanelTitles]);
    expect(visual.actionLabels).toEqual(sharedActionLabels);
    expect(visual.disabledStates).toEqual(sharedDisabledStates);
    expect(visual.remoteApiOnly).toEqual([...remoteApiOnlyVisualSurfaces]);
    expect(visual.desktopTauriOnly).toEqual([...desktopTauriOnlyVisualSurfaces]);
    expect(visual.forbidden.desktopEmbeddedTauri).toEqual([...macosForbiddenVisualSurfaces]);

    const visualContractSource = repositoryFile("webui/src/lib/domain/visualContract.ts");
    expect(visualContractSource).not.toContain("./contractRegistry");
    expect(visualContractSource).not.toContain("agenthub-contract.json");
  });

  test("covers WebUI runtime command wrappers without ad hoc command strings", () => {
    const commandLiterals = apiCommandLiterals();
    const contractCommands = new Set(
      contract().actions
        .filter((action) => typeof action.webuiWrapper === "string" && action.webuiWrapper.trim().length > 0)
        .flatMap((action) => [action.linuxRpc, action.id])
        .filter((value): value is string => typeof value === "string" && value.length > 0)
    );

    expect(commandLiterals).toEqual(contractCommands);
  });

  test("keeps host surface names explicit for runtime capability policy", () => {
    expect(contract().hostSurfaces).toEqual([
      "linux_server_api",
      "desktop_embedded_tauri",
    ]);
  });
});
