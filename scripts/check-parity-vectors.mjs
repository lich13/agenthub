#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const vectorsDir = resolve(root, "contracts/parity-vectors");
const swiftSuite = readFileSync(
  resolve(root, "macos/Tests/AgentHubTests/GoldenVectorsAndDomainParityTests.swift"),
  "utf8"
);

const requiredFiles = [
  ["visible_markdown.json", doc => Array.isArray(doc.cleanCases) && doc.cleanCases.length > 0 && Array.isArray(doc.instructionActivityCases) && Array.isArray(doc.instructionSegmentCases)],
  ["user_context.json", doc => Array.isArray(doc.cases) && doc.cases.length > 0],
  ["session_storage.json", doc => Array.isArray(doc.cases) && doc.cases.length > 0],
  ["session_selection.json", doc => Array.isArray(doc.visible) && Array.isArray(doc.steps) && doc.steps.length > 0],
  ["desktop_references.json", doc => Array.isArray(doc.cases) && doc.cases.length > 0],
];

for (const [name, validate] of requiredFiles) {
  const doc = JSON.parse(readFileSync(resolve(vectorsDir, name), "utf8"));
  assert.ok(validate(doc), `parity vector file invalid or empty: ${name}`);
  assert.ok(
    swiftSuite.includes(`contracts/parity-vectors/${name}`),
    `GoldenVectorsAndDomainParityTests.swift must execute ${name}`
  );
}

console.log(`Shared JSON parity vectors (${requiredFiles.length} suites) verified and wired into Swift XCTest: ok`);
