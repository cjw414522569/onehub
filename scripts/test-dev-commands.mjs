#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const root = path.resolve(process.argv[2] ?? process.cwd());
const dispatcher = path.join(root, 'scripts', 'dev.ps1');
const commands = ['bootstrap', 'doctor', 'format', 'lint', 'test', 'benchmark'];
const errors = [];

if (!fs.existsSync(dispatcher)) errors.push('Missing scripts/dev.ps1 dispatcher.');
else {
  const text = fs.readFileSync(dispatcher, 'utf8');
  for (const command of commands) {
    if (!text.includes(`'${command}'`)) errors.push(`Dispatcher does not declare ${command}.`);
    const wrapper = path.join(root, 'scripts', `${command}.ps1`);
    if (!fs.existsSync(wrapper)) errors.push(`Missing wrapper scripts/${command}.ps1.`);
    else {
      const wrapperText = fs.readFileSync(wrapper, 'utf8');
      if (!wrapperText.includes('dev.ps1') || (!wrapperText.includes(`-Command ${command}`) && !wrapperText.includes(`'-Command', '${command}'`))) {
        errors.push(`Wrapper scripts/${command}.ps1 does not delegate to dev.ps1 -Command ${command}.`);
      }
    }
  }
  if (!text.includes('[switch]$Ci')) errors.push('Dispatcher must expose -Ci.');
  if (!text.includes('[switch]$SkipPlatform')) errors.push('Dispatcher must expose -SkipPlatform.');
}

if (errors.length > 0) {
  console.error(`Developer command contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

const probe = spawnSync(
  'powershell',
  ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', dispatcher, '-Command', 'doctor', '-SkipPlatform'],
  { cwd: root, encoding: 'utf8', windowsHide: true },
);
if (probe.status !== 0) {
  console.error('Developer command contract: doctor -SkipPlatform failed.');
  console.error(probe.stdout);
  console.error(probe.stderr);
  process.exit(1);
}

console.log('Developer command contract passed: dispatcher, wrappers, and doctor smoke test.');
