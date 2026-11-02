import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { baseScore, assessReport } from './security-audit.mjs';

const high = 'CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H';
const medium = 'CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:L/I:N/A:N';
const low = 'CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:L/I:N/A:N';
const advisory = (cvss, extra = {}) => ({ advisory: { id: 'RUSTSEC-2099-0001', package: 'fixture', cvss, ...extra } });
const cargo = (list = [], warnings = {}) => ({ settings: { ignore: [], severity: null }, database: { 'advisory-count': 10 }, lockfile: { 'dependency-count': 1 }, vulnerabilities: { found: list.length > 0, count: list.length, list }, warnings });
const pnpm = (severity) => ({ advisories: severity ? { 1: { id: 1, severity } } : {}, metadata: { vulnerabilities: { info: 0, low: 0, moderate: 0, high: 0, critical: 0, ...(severity ? { [severity]: 1 } : {}) } } });

test('CVSS base scores cover low, medium, high, critical and changed scope', () => {
  assert.equal(baseScore(low), 3.7);
  assert.equal(baseScore(medium), 5.3);
  assert.equal(baseScore(high), 7.5);
  assert.equal(baseScore('CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H'), 9.8);
  assert.equal(baseScore('CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H'), 10);
  assert.equal(baseScore('CVSS:3.0/AV:N/AC:H/PR:H/UI:R/S:C/C:N/I:N/A:N'), 0);
  for (const value of [null, '', 'CVSS:4.0/AV:N', high + '/S:U', high.replace('/AC:L', '')]) assert.equal(baseScore(value), null);
});

test('cargo active high and unscored advisories block, maintenance is informational', () => {
  const result = assessReport('cargo', cargo([advisory(high), advisory(null), advisory(low), advisory(medium)], { unmaintained: [{}], unsound: [{}] }), 1);
  assert.equal(result.blocking.length, 2);
  assert.equal(result.notices.length, 2);
  assert.equal(result.maintenance, 2);
  assert.equal(assessReport('cargo', cargo([advisory(null, { withdrawn: '2099-01-01' })]), 0).blocking.length, 0);
});

test('pnpm high, critical and unknown severities block; lower severities remain visible', () => {
  for (const severity of ['high', 'critical', 'unknown']) assert.equal(assessReport('pnpm', pnpm(severity), 1).blocking.length, 1);
  for (const severity of ['info', 'low', 'moderate']) assert.equal(assessReport('pnpm', pnpm(severity), 1).notices.length, 1);
  assert.deepEqual(assessReport('pnpm', pnpm(), 0), { blocking: [], notices: [], maintenance: 0 });
});

test('missing/invalid reports, inconsistent counts and tool failures cannot pass', () => {
  for (const kind of ['cargo', 'pnpm']) {
    for (const value of [null, {}, [], { error: 'fixture' }]) assert.throws(() => assessReport(kind, value, 0));
    const report = kind === 'cargo' ? cargo() : pnpm();
    for (const status of [null, 1, 2, 127]) assert.throws(() => assessReport(kind, report, status));
  }
  const ignored = cargo(); ignored.settings.ignore = ['RUSTSEC-2099-0001'];
  assert.throws(() => assessReport('cargo', ignored, 0));
  const incomplete = cargo(); incomplete.database['advisory-count'] = 0;
  assert.throws(() => assessReport('cargo', incomplete, 0));
  const mismatch = pnpm('high'); mismatch.metadata.vulnerabilities.high = 0;
  assert.throws(() => assessReport('pnpm', mismatch, 0));
});

test('scanner errors are blocking and raw diagnostic text never reaches logs or artifacts', () => {
  const root = mkdtempSync(join(tmpdir(), 'agenthub-audit-test-'));
  try {
    const executable = join(root, 'cargo');
    const report = join(root, 'result.json');
    const summary = join(root, 'summary.txt');
    writeFileSync(executable, `#!${process.execPath}\nconsole.log('untrusted diagnostic marker'); console.error('untrusted diagnostic marker'); process.exit(2);\n`, { mode: 0o755 });
    const result = spawnSync(process.execPath, [resolve('scripts/security-audit.mjs'), 'cargo', report], { encoding: 'utf8', env: { ...process.env, PATH: root, GITHUB_STEP_SUMMARY: summary } });
    assert.equal(result.status, 1);
    for (const output of [result.stdout, result.stderr, readFileSync(report, 'utf8'), readFileSync(summary, 'utf8')]) assert.ok(!output.includes('untrusted diagnostic marker'));
    assert.deepEqual(JSON.parse(readFileSync(report, 'utf8')), { scanFailed: true });
    writeFileSync(executable, `#!${process.execPath}\nconsole.log(${JSON.stringify(JSON.stringify(cargo()))});\n`, { mode: 0o755 });
    const success = spawnSync(process.execPath, [resolve('scripts/security-audit.mjs'), 'cargo', report], { encoding: 'utf8', env: { ...process.env, PATH: root, GITHUB_STEP_SUMMARY: summary } });
    assert.equal(success.status, 0);
    assert.deepEqual(JSON.parse(readFileSync(report, 'utf8')), cargo());
  } finally { rmSync(root, { recursive: true }); }
});
