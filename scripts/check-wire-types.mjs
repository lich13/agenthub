/** Development-only AST contract. Reuses the locked TypeScript compiler API. */
import { API } from "../webui/node_modules/typescript/dist/api/sync/api.js";
import * as ts from "../webui/node_modules/typescript/dist/ast/index.js";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const root = fileURLToPath(new URL("../", import.meta.url));
const fixturePath = resolve(root, "contracts/wire-types.json");
const tokens = /"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\/\*[\s\S]*?\*\/|\/\/[^\n]*|\s+|./gs;
export function normalized(text) {
  return (text.match(tokens) ?? []).filter(token => !/^\s+$|^\/\*|^\/\//.test(token)).join("");
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
    if (mapping.ts) {
      const [file, type] = mapping.ts.split("#");
      assert.ok(actual[file]?.types[type], `${name}: actual TypeScript type missing`);
    } else assert.ok(mapping.inlineTs?.trim(), `${name}: unclassified inline TypeScript payload`);
    assert.ok(mapping.rust || mapping.freeJson?.trim(), `${name}: unclassified Rust payload`);
  }
}
export function current(fixture) {
  const api = new API({ cwd: resolve(root,"webui") });
  let snapshot;
  try {
    const config=resolve(root,"webui/tsconfig.app.json");
    snapshot=api.updateSnapshot({openProjects:[config]});
    const project=snapshot.getProject(config);
    assert.ok(project);
    const files = new Set([...fixture.typescriptFiles,
      ...readdirSync(resolve(root,"webui/src/lib/api")).filter(name=>name.endsWith(".ts")&&!name.endsWith(".test.ts")&&name!=="demo.ts").map(name=>`webui/src/lib/api/${name}`)]);
    const output={};
    for (const file of [...files].sort()) {
      const source=project.program.getSourceFile(resolve(root,file));
      assert.ok(source,`TypeScript source missing: ${file}`);
      const types={},calls=[];
      const visit=node=>{
        if (ts.isTypeAliasDeclaration(node) || ts.isInterfaceDeclaration(node)) {
          types[node.name.text]=normalized(node.getText(source));
        }
        if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && ["callCommand","invokeNative","runtimeRpc","changeConnection","startProbeCommand","runTypedUpdateCommand"].includes(node.expression.text)) {
          calls.push(normalized(node.getText(source)));
        }
        node.forEachChild(visit);
      };
      visit(source);
      output[file]={types,calls};
    }
    return output;
  } finally { try {snapshot?.dispose();} finally {api.close();} }
}
export function check({update=false}={}) {
  const fixture=JSON.parse(readFileSync(fixturePath,"utf8"));
  const actual=current(fixture);
  const registry=JSON.parse(readFileSync(resolve(root,"contracts/agenthub-contract.json"),"utf8"));
  assertRegistrySchema(registry,JSON.parse(readFileSync(resolve(root,"contracts/agenthub-contract.schema.json"),"utf8")));
  assertCatalog(fixture,registry,actual);
  if (update) {fixture.typescript=actual;writeFileSync(fixturePath,JSON.stringify(fixture,null,2)+"\n");}
  assert.deepEqual(actual,fixture.typescript,"actual TypeScript DTO, optional/null, enum, or wrapper shape drift; follow Shared changes");
  console.log(`Wire TypeScript fields and RPC wrappers: ${Object.keys(actual).length} files checked`);
}
if (process.argv[1] && relative(resolve(root),resolve(process.argv[1]))==="scripts/check-wire-types.mjs") check({update:process.argv.includes("--update")});
