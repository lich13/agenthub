#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "../webui/node_modules/vite/dist/node/index.js";

const root = fileURLToPath(new URL("../", import.meta.url));
const vectorsDir = resolve(root, "contracts/parity-vectors");

const vite = await createServer({
  root: resolve(root, "webui"),
  logLevel: "silent",
  server: { middlewareMode: true, hmr: false },
  appType: "custom",
});

try {
  const mdMod = await vite.ssrLoadModule("/src/lib/domain/visibleMarkdown.ts");
  const ctxMod = await vite.ssrLoadModule("/src/lib/domain/userContext.ts");
  const storageMod = await vite.ssrLoadModule("/src/lib/domain/sessionStorage.ts");
  const selMod = await vite.ssrLoadModule("/src/lib/domain/sessionSelection.ts");
  const refMod = await vite.ssrLoadModule("/src/lib/desktopReferences.ts");

  const mdVec = JSON.parse(readFileSync(resolve(vectorsDir, "visible_markdown.json"), "utf8"));
  for (const c of mdVec.cleanCases) {
    assert.equal(mdMod.visibleMarkdown(c.input), c.expected, `visibleMarkdown: ${c.name}`);
  }
  for (const c of mdVec.instructionActivityCases) {
    assert.equal(mdMod.isInstructionFileActivity(...c.inputs), c.expected);
  }
  for (const c of mdVec.instructionSegmentCases) {
    assert.deepEqual(mdMod.instructionSegments(c.input), c.expected, `instructionSegments: ${c.name}`);
  }

  const ctxVec = JSON.parse(readFileSync(resolve(vectorsDir, "user_context.json"), "utf8"));
  for (const c of ctxVec.cases) {
    assert.deepEqual(ctxMod.projectUserMessage(c.input), c.expected, `projectUserMessage: ${c.name}`);
  }

  const storageVec = JSON.parse(readFileSync(resolve(vectorsDir, "session_storage.json"), "utf8"));
  for (const c of storageVec.cases) {
    assert.deepEqual(storageMod.formatSessionStorage(c.input), c.expected, `formatSessionStorage: ${c.name}`);
  }

  const selVec = JSON.parse(readFileSync(resolve(vectorsDir, "session_selection.json"), "utf8"));
  for (const step of selVec.steps) {
    const actual = selMod.clickSessionSelection(step.initial, selVec.visible, step.key, step.click, step.activeKey);
    assert.deepEqual(actual, step.expected, `clickSessionSelection: ${step.name}`);
  }

  const refVec = JSON.parse(readFileSync(resolve(vectorsDir, "desktop_references.json"), "utf8"));
  for (const c of refVec.cases) {
    const initial = { text: c.initialText, references: [], spans: [] };
    const { draft, caret } = refMod.insertDesktopReference(initial, c.insert.item, c.insert.start, c.insert.end);
    assert.equal(draft.text, c.expectedText, `insertDesktopReference text: ${c.name}`);
    assert.equal(caret, c.expectedCaretUtf16, `insertDesktopReference caret: ${c.name}`);
    assert.deepEqual(draft.spans, c.expectedUtf16Spans, `insertDesktopReference utf16 spans: ${c.name}`);
    assert.deepEqual(refMod.toDesktopReferenceSpans(draft), c.expectedUtf8Spans, `toDesktopReferenceSpans: ${c.name}`);
  }

  console.log("Shared JSON parity vectors verified against WebUI TypeScript modules: ok");
} finally {
  await vite.close();
}
