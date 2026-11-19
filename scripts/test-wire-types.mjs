import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { assertCatalog, assertRegistrySchema, normalized } from "./check-wire-types.mjs";

const fixture = JSON.parse(readFileSync(new URL("../contracts/wire-types.json", import.meta.url), "utf8"));
const registry = JSON.parse(readFileSync(new URL("../contracts/agenthub-contract.json", import.meta.url), "utf8"));

test("every catalog entry resolves to a real Swift/Rust type or explicit inline payload", () => {
  assertCatalog(fixture, registry, fixture.swift);
});

test("a missing catalog mapping fails", () => {
  const copy = structuredClone(fixture);
  delete copy.catalog.ThreadsDetailResponse;
  assert.throws(() => assertCatalog(copy, registry, copy.swift), /mapping missing/);
});

test("a marker without a real Swift declaration fails", () => {
  const copy = structuredClone(fixture);
  const [file, name] = copy.catalog.ThreadsDetailResponse.swift.split("#");
  delete copy.swift[file].types[name];
  assert.throws(() => assertCatalog(copy, registry, copy.swift), /actual Swift type missing/);
});

test("unknown dynamic JSON cannot silently bypass classification", () => {
  const copy = structuredClone(fixture);
  copy.catalog.NoRequest = {};
  assert.throws(() => assertCatalog(copy, registry, copy.swift), /unclassified/);
});

test("normalization preserves optionality, types, CodingKeys and literal spaces", () => {
  const original = normalized('struct A: Codable { let value: String?; let state = "has space" }');
  assert.equal(original, normalized('struct A:Codable{let value:String?;let state="has space"}'));
  for (const text of [
    'struct A:Codable{let value:String;let state="has space"}',
    'struct A:Codable{let state="has space"}',
    'struct A:Codable{let value:String?;let state="hasspace"}',
  ]) {
    assert.notEqual(normalized(text), original);
  }
});

const schema = JSON.parse(readFileSync(new URL("../contracts/agenthub-contract.schema.json", import.meta.url), "utf8"));
test("registry satisfies the actual schema", () => assertRegistrySchema(registry, schema));

test("schema rejects missing, mistyped and unexpected fields", () => {
  for (const mutate of [
    value => delete value.actions[0].coreUseCase,
    value => (value.schemaVersion = "1"),
    value => (value.actions[0].unregistered = true),
  ]) {
    const copy = structuredClone(registry);
    mutate(copy);
    assert.throws(() => assertRegistrySchema(copy, schema));
  }
});

test("unsupported schema constraints cannot silently pass", () => {
  assert.throws(() => assertRegistrySchema(registry, { ...schema, not: {} }), /unsupported schema keyword/);
});
