#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const PLATFORM_JOBS = {
  windows: {
    jobId: 'platform-windows',
    context: 'ci / platform-windows',
    runner: 'windows-latest',
    contract: 'clients/windows/contract.json',
    status: 'windows-first-buildable',
    descriptors: [],
  },
  macos: {
    jobId: 'platform-macos',
    context: 'ci / platform-macos',
    runner: 'macos-latest',
    contract: 'clients/apple/contract.json',
    status: 'interface-only',
    descriptors: ['clients/apple/Package.swift'],
  },
  ios: {
    jobId: 'platform-ios',
    context: 'ci / platform-ios',
    runner: 'macos-latest',
    contract: 'clients/apple/contract.json',
    status: 'interface-only',
    descriptors: ['clients/apple/Package.swift'],
  },
  linux: {
    jobId: 'platform-linux',
    context: 'ci / platform-linux',
    runner: 'ubuntu-latest',
    contract: 'clients/linux/contract.json',
    status: 'interface-only',
    descriptors: ['clients/linux/meson.build'],
  },
  android: {
    jobId: 'platform-android',
    context: 'ci / platform-android',
    runner: 'ubuntu-latest',
    contract: 'clients/android/contract.json',
    status: 'interface-only',
    descriptors: ['clients/android/settings.gradle.kts', 'clients/android/build.gradle.kts'],
  },
};

function read(root, relativePath) {
  return fs.readFileSync(path.join(root, relativePath), 'utf8');
}

function jobBlock(workflow, jobId) {
  const startToken = `  ${jobId}:`;
  const start = workflow.indexOf(startToken);
  if (start < 0) return '';
  const rest = workflow.slice(start + startToken.length);
  const next = rest.search(/\n  [A-Za-z0-9_-]+:\n/);
  return workflow.slice(start, next < 0 ? workflow.length : start + startToken.length + next);
}

export function validatePlatformCiText({ workflow, branchProtection, rootDir }) {
  const errors = [];
  if (!workflow.includes('pull_request:')) errors.push('CI workflow must run on pull_request.');
  if (!workflow.includes('runs-on: windows-latest')) errors.push('CI workflow must retain a Windows runner.');
  for (const [platform, spec] of Object.entries(PLATFORM_JOBS)) {
    const block = jobBlock(workflow, spec.jobId);
    if (!block) {
      errors.push(`Missing independent platform job: ${spec.jobId}`);
      continue;
    }
    if (!block.includes(`name: ${spec.context}`)) errors.push(`${spec.jobId} must expose required context ${spec.context}.`);
    if (!block.includes(`runs-on: ${spec.runner}`)) errors.push(`${spec.jobId} must use ${spec.runner}.`);
    for (const step of ['name: Build', 'name: Lint', 'name: Unit tests']) {
      if (!block.includes(step)) errors.push(`${spec.jobId} is missing independent ${step} step.`);
    }
    if (!block.includes(`--platform ${platform}`)) errors.push(`${spec.jobId} must validate the ${platform} boundary.`);
    if (rootDir) {
      for (const descriptor of spec.descriptors) {
        const descriptorPath = path.join(rootDir, descriptor);
        if (!fs.existsSync(descriptorPath)) {
          errors.push(`Missing ${platform} CI descriptor: ${descriptor}`);
          continue;
        }
        const descriptorText = fs.readFileSync(descriptorPath, 'utf8');
        if (descriptor.endsWith('Package.swift')) {
          for (const token of ['swift-tools-version: 6.2', 'SSHClientAppleBoundary', '.testTarget']) {
            if (!descriptorText.includes(token)) errors.push(`${descriptor} is missing required token: ${token}`);
          }
        }
        if (descriptor.endsWith('build.gradle.kts')) {
          for (const token of ['platformBuild', 'platformLint', 'platformTest', 'interface-only']) {
            if (!descriptorText.includes(token)) errors.push(`${descriptor} is missing required token: ${token}`);
          }
        }
        if (descriptor.endsWith('meson.build')) {
          for (const token of ['project(', 'executable(', "test('"]) {
            if (!descriptorText.includes(token)) errors.push(`${descriptor} is missing required token: ${token}`);
          }
        }
      }
      try {
        const contract = JSON.parse(read(rootDir, spec.contract));
        if (contract.status !== spec.status) errors.push(`${spec.contract} must declare status=${spec.status}.`);
      } catch (error) {
        errors.push(`${spec.contract} is missing or invalid JSON: ${error.message}`);
      }
    }
    if (!branchProtection.includes(`"${spec.context}"`)) errors.push(`Branch protection must require ${spec.context}.`);
  }
  return errors;
}

export function validatePlatformCi(rootDir) {
  const root = path.resolve(rootDir);
  return validatePlatformCiText({
    workflow: read(root, '.github/workflows/ci.yml'),
    branchProtection: read(root, '.github/branch-protection.yml'),
    rootDir: root,
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  const root = process.argv[2] ?? process.cwd();
  const errors = validatePlatformCi(root);
  if (errors.length > 0) {
    console.error(`Platform CI contract failed with ${errors.length} error(s):`);
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
  } else {
    console.log(`Platform CI contract valid: ${Object.keys(PLATFORM_JOBS).length} independent build/lint/test jobs and required contexts.`);
  }
}
