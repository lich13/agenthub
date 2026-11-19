#!/usr/bin/env node
/** Zero-dependency contract & Swift/Rust wire-type guard. */
import { readFileSync, writeFileSync } from "node:fs";
import { resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const root = fileURLToPath(new URL("../", import.meta.url));
const fixturePath = resolve(root, "contracts/wire-types.json");
const tokenPattern = /"""[\s\S]*?"""|"(?:\\.|[^"\\])*"|\/\*[\s\S]*?\*\/|\/\/[^\n]*|\s+|[A-Za-z_][A-Za-z0-9_]*|[{}();=?:,.<>[\]]|./gs;

export function tokenize(text) {
  return (text.match(tokenPattern) ?? []).filter(token => !/^\s+$|^\/\*|^\/\//.test(token));
}

export function normalized(text) {
  return tokenize(text).join("");
}

/** Extract top-level Swift struct/enum/typealias declarations and RPC command calls. */
export function parseSwiftSource(text) {
  const toks = tokenize(text);
  const types = {};
  const calls = [];
  let depth = 0;
  for (let i = 0; i < toks.length; i++) {
    const tok = toks[i];
    if (tok === "{") {
      depth++;
      continue;
    }
    if (tok === "}") {
      depth = Math.max(0, depth - 1);
      continue;
    }
    if (depth === 0 && (tok === "struct" || tok === "enum")) {
      const name = toks[i + 1];
      if (name && /^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
        let j = i + 2;
        while (j < toks.length && toks[j] !== "{") j++;
        if (j < toks.length) {
          let inner = 1;
          let k = j + 1;
          while (k < toks.length && inner > 0) {
            if (toks[k] === "{") inner++;
            else if (toks[k] === "}") inner--;
            k++;
          }
          types[name] = toks.slice(i, k).join("");
        }
      }
    } else if (depth === 0 && tok === "typealias") {
      const name = toks[i + 1];
      if (name && /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && toks[i + 2] === "=") {
        let k = i + 3;
        let angle = 0;
        let bracket = 0;
        let paren = 0;
        while (k < toks.length) {
          const t = toks[k];
          if (t === "<") angle++;
          else if (t === ">") angle = Math.max(0, angle - 1);
          else if (t === "[") bracket++;
          else if (t === "]") bracket = Math.max(0, bracket - 1);
          else if (t === "(") paren++;
          else if (t === ")") paren = Math.max(0, paren - 1);
          else if (angle === 0 && bracket === 0 && paren === 0 && (t === "public" || t === "internal" || t === "private" || t === "struct" || t === "enum" || t === "final" || t === "actor" || t === "func" || t === "typealias")) {
            break;
          }
          k++;
        }
        types[name] = toks.slice(i, k).join("");
      }
    }
    if (
      ["invoke", "invokeJSON", "invokeVoid", "invokeCommand", "invokeRemoteOrLocal"].includes(tok) &&
      toks[i + 1] === "("
    ) {
      let paren = 1;
      let k = i + 2;
      while (k < toks.length && paren > 0) {
        if (toks[k] === "(") paren++;
        else if (toks[k] === ")") paren--;
        k++;
      }
      calls.push(toks.slice(i, k).join(""));
    }
  }
  return { types, calls };
}

/** Validate only the checked-in schema vocabulary; unknown keywords fail closed. */
export function assertRegistrySchema(value, schema, document = schema, path = "$root") {
  const supported = new Set(["$schema", "$id", "$defs", "$ref", "title", "type", "const", "enum", "required", "properties", "additionalProperties", "items", "uniqueItems", "minItems", "minLength", "minProperties"]);
  for (const key of Object.keys(schema)) assert.ok(supported.has(key), `${path}: unsupported schema keyword ${key}`);
  if (schema.$ref) {
    assert.ok(schema.$ref.startsWith("#/"), "only local schema references are supported");
    const target = schema.$ref.slice(2).split("/").reduce((node, key) => node?.[key.replaceAll("~1", "/").replaceAll("~0", "~")], document);
    assert.ok(target, `${path}: unresolved schema reference`);
    assertRegistrySchema(value, target, document, path);
  }
  if (schema.type) {
    const matches = { object: value !== null && typeof value === "object" && !Array.isArray(value), array: Array.isArray(value), string: typeof value === "string", integer: Number.isInteger(value) };
    assert.ok(matches[schema.type], `${path}: expected ${schema.type}`);
  }
  if ("const" in schema) assert.deepEqual(value, schema.const, `${path}: constant mismatch`);
  if (schema.enum) assert.ok(schema.enum.some(item => JSON.stringify(item) === JSON.stringify(value)), `${path}: enum mismatch`);
  if (schema.minLength !== undefined) assert.ok([...value].length >= schema.minLength, `${path}: string too short`);
  if (schema.type === "array") {
    if (schema.minItems !== undefined) assert.ok(value.length >= schema.minItems, `${path}: array too short`);
    if (schema.uniqueItems) assert.equal(new Set(value.map(item => JSON.stringify(item))).size, value.length, `${path}: duplicate items`);
    if (schema.items) value.forEach((item, i) => assertRegistrySchema(item, schema.items, document, `${path}[${i}]`));
  }
  if (schema.type === "object") {
    if (schema.minProperties !== undefined) assert.ok(Object.keys(value).length >= schema.minProperties, `${path}: object too small`);
    for (const key of schema.required ?? []) assert.ok(Object.hasOwn(value, key), `${path}: missing required ${key}`);
    for (const [key, item] of Object.entries(value)) {
      const child = schema.properties?.[key];
      if (child) assertRegistrySchema(item, child, document, `${path}.${key}`);
      else if (schema.additionalProperties === false) assert.fail(`${path}: unexpected property ${key}`);
      else if (schema.additionalProperties && typeof schema.additionalProperties === "object") assertRegistrySchema(item, schema.additionalProperties, document, `${path}.${key}`);
    }
  }
}

export function assertCatalog(fixture, registry, actual) {
  assert.deepEqual(Object.keys(fixture.catalog).sort(), Object.keys(registry.dtoCatalog).sort(), "actual-type mapping missing for a registry DTO");
  for (const [name, mapping] of Object.entries(fixture.catalog)) {
    if (mapping.swift) {
      const [file, type] = mapping.swift.split("#");
      assert.ok(actual[file]?.types[type], `${name}: actual Swift type missing (${mapping.swift})`);
    } else {
      assert.ok(mapping.inlineSwift?.trim(), `${name}: unclassified inline Swift payload`);
    }
    assert.ok(mapping.rust || mapping.freeJson?.trim(), `${name}: unclassified Rust payload`);
  }
}

export function current(fixture) {
  const output = {};
  for (const file of [...fixture.swiftFiles].sort()) {
    const text = readFileSync(resolve(root, file), "utf8");
    output[file] = parseSwiftSource(text);
  }
  return output;
}

export function check({ update = false } = {}) {
  const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));
  const actual = current(fixture);
  const registry = JSON.parse(readFileSync(resolve(root, "contracts/agenthub-contract.json"), "utf8"));
  assertRegistrySchema(registry, JSON.parse(readFileSync(resolve(root, "contracts/agenthub-contract.schema.json"), "utf8")));
  assertCatalog(fixture, registry, actual);
  if (update) {
    fixture.swift = actual;
    delete fixture.typescript;
    delete fixture.typescriptFiles;
    writeFileSync(fixturePath, JSON.stringify(fixture, null, 2) + "\n");
  }
  assert.deepEqual(actual, fixture.swift, "actual Swift DTO, optionality, enum, or RPC call shape drift; follow Shared changes");
  console.log(`Wire Swift 6 DTOs and RPC calls: ${Object.keys(actual).length} files checked`);
}

if (process.argv[1] && relative(resolve(root), resolve(process.argv[1])) === "scripts/check-wire-types.mjs") {
  check({ update: process.argv.includes("--update") });
}
