#!/usr/bin/env node

// T150 contract: public compatibility / troubleshooting / security-response
// docs. Runs link validation, command-example validation against the real
// binary, decision-tree validation, and the vulnerability-channel check.

import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const DOCS = join(ROOT, 'docs');
const errors = [];

const files = ['COMPATIBILITY.md', 'TROUBLESHOOTING.md', 'SECURITY.md'];

// 1. Link validation: internal relative links must exist; external URLs
//    must be http(s).
for (const file of files) {
  const text = readFileSync(join(DOCS, file), 'utf8');
  for (const match of text.matchAll(/\[[^\]]*\]\(([^)]+)\)/g)) {
    const link = match[1];
    if (/^https?:\/\//.test(link)) continue;
    if (link.startsWith('#')) continue;
    if (link.startsWith('mailto:')) continue;
    const target = link.startsWith('../')
      ? resolve(join(dirname(join(DOCS, file)), link))
      : join(DOCS, link);
    if (!existsSync(target)) errors.push(`${file}: broken internal link: ${link}`);
  }
}

// 2. Command-example validation against the real binary usage.
const help = spawnSync(join(ROOT, 'target/debug/ssh-cli.exe'), ['--help'], { encoding: 'utf8' });
if (help.status !== 0) errors.push('ssh-cli --help failed');
const usage = help.stdout ?? '';
const knownSubcommands = ['--version', '--help', '--json', 'config', 'cap', '--config', 'exec'];
for (const sub of knownSubcommands) {
  if (!usage.includes(sub)) errors.push(`ssh-cli usage missing documented command: ${sub}`);
}
for (const file of files) {
  const text = readFileSync(join(DOCS, file), 'utf8');
  for (const match of text.matchAll(/`(ssh-cli [^`]+)`/g)) {
    const command = match[1];
    if (!command.startsWith('ssh-cli')) continue;
    const tokens = command.split(/\s+/);
    const first = tokens[1] ?? '';
    if (!['--version', '--help', '--json', 'config', 'cap', '--config'].includes(first)) {
      errors.push(`${file}: undocumented ssh-cli command example: ${command}`);
    }
  }
}

// 3. Decision-tree validation: every troubleshooting node that asks a
//    check must end in a resolution.
const trouble = readFileSync(join(DOCS, 'TROUBLESHOOTING.md'), 'utf8');
const checks = trouble.match(/Check:/g)?.length ?? 0;
const resolved = trouble.match(/Resolved:/g)?.length ?? 0;
if (checks === 0) errors.push('TROUBLESHOOTING.md has no decision-tree checks');
if (resolved < checks) errors.push('TROUBLESHOOTING.md decision tree has unresolved leaves');
for (const expected of ['cannot connect', 'config --check', 'Offline - not connected', 'exit code 4']) {
  if (!trouble.includes(expected)) errors.push(`TROUBLESHOOTING.md missing expected guidance: ${expected}`);
}

// 4. Security response: a vulnerability reporting channel and a response SLA.
const security = readFileSync(join(DOCS, 'SECURITY.md'), 'utf8');
if (!/security@[^\s]+/.test(security) && !security.includes('security/advisories')) {
  errors.push('SECURITY.md missing a vulnerability reporting channel');
}
for (const expected of ['Response SLA', 'Critical', 'High', 'Medium', 'Low', 'Disclosure']) {
  if (!security.includes(expected)) errors.push(`SECURITY.md missing section: ${expected}`);
}

if (errors.length > 0) {
  console.error(`public-docs contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log(`public-docs contract valid: ${files.join(', ')} links resolve, ssh-cli command examples match the real binary usage, the troubleshooting decision tree has ${checks} checks all resolved, and SECURITY.md defines a vulnerability reporting channel with a response SLA.`);