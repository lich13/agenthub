#!/usr/bin/env node
// CI-only audit policy. No runtime dependency or advisory exemptions.
import { spawnSync } from 'node:child_process';
import { writeFileSync, appendFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

function requireReport(condition) {
  if (!condition) throw new Error('Security audit report is incomplete or invalid');
}

// FIRST CVSS 3.0/3.1 base equations (not temporal/environmental scoring):
// FIRST CVSS v3.1 Specification Document, section 7.1.
// Unknown scoring formats remain unclassified and therefore block the gate.
export function baseScore(vector) {
  if (typeof vector !== 'string' || !/^CVSS:3\.[01]\//.test(vector)) return null;
  const fields = vector.split('/').slice(1).map((part) => part.split(':'));
  if (fields.some((pair) => pair.length !== 2) || new Set(fields.map(([k]) => k)).size !== fields.length) return null;
  const m = Object.fromEntries(fields);
  if (!['U', 'C'].includes(m.S)) return null;
  const av = { N: .85, A: .62, L: .55, P: .2 }[m.AV];
  const ac = { L: .77, H: .44 }[m.AC];
  const pr = (m.S === 'U' ? { N: .85, L: .62, H: .27 } : { N: .85, L: .68, H: .5 })[m.PR];
  const ui = { N: .85, R: .62 }[m.UI];
  const impacts = ['C', 'I', 'A'].map((k) => ({ H: .56, L: .22, N: 0 })[m[k]]);
  if ([av, ac, pr, ui, ...impacts].some((x) => x === undefined)) return null;
  const iss = 1 - impacts.reduce((product, value) => product * (1 - value), 1);
  const impact = m.S === 'U' ? 6.42 * iss : 7.52 * (iss - .029) - 3.25 * (iss - .02) ** 15;
  if (impact <= 0) return 0;
  const score = Math.min((impact + 8.22 * av * ac * pr * ui) * (m.S === 'U' ? 1 : 1.08), 10);
  // Avoid floating-point noise before rounding up to one decimal place.
  return Math.ceil(Math.round(score * 100000) / 10000) / 10;
}

export function assessReport(kind, report, status) {
  requireReport([0, 1].includes(status) && report && typeof report === 'object' && !report.error);
  const result = { blocking: [], notices: [], maintenance: 0 };
  if (kind === 'cargo') {
    const v = report.vulnerabilities;
    requireReport(report.settings && Array.isArray(report.settings.ignore) && report.settings.ignore.length === 0 && report.settings.severity === null);
    requireReport(report.database?.['advisory-count'] > 0 && report.lockfile?.['dependency-count'] > 0);
    requireReport(v && Array.isArray(v.list) && v.count === v.list.length && v.found === (v.count > 0));
    requireReport(report.warnings && typeof report.warnings === 'object' && !Array.isArray(report.warnings));
    for (const item of v.list) {
      const a = item.advisory;
      requireReport(a && /^RUSTSEC-\d{4}-\d+$/.test(a.id) && typeof a.package === 'string');
      if (a.withdrawn) continue;
      const score = baseScore(a.cvss);
      const entry = `${a.id}: ${score === null ? 'unclassified' : score.toFixed(1)}`;
      (score === null || score >= 7 ? result.blocking : result.notices).push(entry);
    }
    for (const values of Object.values(report.warnings)) {
      requireReport(Array.isArray(values));
      result.maintenance += values.length;
    }
  } else if (kind === 'pnpm') {
    requireReport(report.advisories && typeof report.advisories === 'object' && !Array.isArray(report.advisories));
    const counts = report.metadata?.vulnerabilities;
    requireReport(counts && ['info', 'low', 'moderate', 'high', 'critical'].every((k) => Number.isInteger(counts[k]) && counts[k] >= 0));
    const advisories = Object.values(report.advisories);
    requireReport(Object.values(counts).reduce((n, x) => n + x, 0) === advisories.length);
    for (const a of advisories) {
      requireReport(a && /^\d+$/.test(String(a.id)));
      const severity = a.severity;
      const entry = `npm advisory ${a.id}: ${['info', 'low', 'moderate', 'high', 'critical'].includes(severity) ? severity : 'unclassified'}`;
      (['info', 'low', 'moderate'].includes(severity) ? result.notices : result.blocking).push(entry);
    }
  } else throw new Error('Unsupported audit kind');
  // Exit 1 with a clean report must not disguise a tool/database failure.
  requireReport(status === 0 || result.blocking.length + result.notices.length + result.maintenance > 0);
  return result;
}

export function runAudit(kind, output, lockfile) {
  const command = kind === 'cargo' ? 'cargo' : 'pnpm';
  const args = kind === 'cargo' ? ['audit', '--json', '--file', lockfile] : ['--dir', 'webui', 'audit', '--json'];
  const scan = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, timeout: 300000 });
  try {
    requireReport(!scan.error && !scan.signal);
    const report = JSON.parse(scan.stdout);
    const result = assessReport(kind, report, scan.status);
    writeFileSync(output, scan.stdout);
    const maintenance = kind === 'cargo' ? Object.entries(report.warnings).flatMap(([category, items]) => items.map((item) => {
      const id = item.advisory?.id;
      return /^RUSTSEC-\d{4}-\d+$/.test(id ?? '') && ['unmaintained', 'unsound', 'notice'].includes(category) ? `${id}: ${category}` : 'Maintenance warning (see JSON artifact)';
    })) : [];
    const summary = `Security audit (${kind}): ${result.blocking.length} blocking, ${result.notices.length} low/medium, ${result.maintenance} maintenance warnings.\n${[...result.blocking, ...result.notices, ...maintenance].map((x) => `- ${x}\n`).join('')}`;
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
    console.log(summary.trim());
    return result.blocking.length > 0 ? 1 : 0;
  } catch {
    // Never forward raw tool errors, registry credentials or untrusted server text.
    writeFileSync(output, JSON.stringify({ scanFailed: true }) + '\n');
    const message = 'Security audit failed to produce a complete report; gate blocked.\n';
    if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, message);
    console.error(message.trim());
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [kind, output, lockfile = 'Cargo.lock'] = process.argv.slice(2);
  if (!['cargo', 'pnpm'].includes(kind) || !output) throw new Error('Usage: security-audit.mjs cargo|pnpm output.json [Cargo.lock]');
  process.exitCode = runAudit(kind, output, lockfile);
}
