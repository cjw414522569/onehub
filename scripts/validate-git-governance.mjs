import { existsSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const errors = [];
const REQUIRED_CI_CONTEXTS = [
  'ci / governance',
  'ci / cargo',
  'ci / license',
  'ci / toolchain-contract',
  'ci / supply-chain',
  'ci / fuzz-smoke',
  'ci / control-ledger',
  'ci / platform-windows',
  'ci / platform-macos',
  'ci / platform-ios',
  'ci / platform-linux',
  'ci / platform-android',
];

const requiredFiles = [
  '.gitignore',
  '.gitattributes',
  '.gitmessage',
  '.github/CODEOWNERS',
  '.github/branch-protection.yml',
  '.github/pull_request_template.md',
  '.github/ISSUE_TEMPLATE/bug_report.yml',
  '.github/ISSUE_TEMPLATE/feature_request.yml',
  '.github/workflows/ci.yml',
  'scripts/init-git-governance.ps1',
  'scripts/validate-commit-message.mjs',
];

function absolute(relativePath) {
  return join(ROOT, relativePath.replaceAll('/', '\\'));
}

function read(relativePath) {
  const file = absolute(relativePath);
  if (!existsSync(file) || !statSync(file).isFile()) {
    errors.push(`missing governance artifact: ${relativePath}`);
    return '';
  }
  return readFileSync(file, 'utf8').replace(/^\uFEFF/, '');
}

function exists(relativePath) {
  const file = absolute(relativePath);
  return existsSync(file) && statSync(file).isFile();
}

function requireText(relativePath, tokens) {
  const text = read(relativePath);
  for (const token of tokens) {
    if (!text.includes(token)) errors.push(`${relativePath} is missing required token: ${token}`);
  }
  return text;
}

function scalar(text, key) {
  const match = text.match(new RegExp(`^\\s*${key}:\\s*([^#\\r\\n]+)`, 'm'));
  return match ? match[1].trim().replace(/^['"]|['"]$/g, '') : null;
}

function booleanScalar(text, key) {
  const value = scalar(text, key);
  return value === 'true' ? true : value === 'false' ? false : null;
}

function validateBranchProtection() {
  const policy = requireText('.github/branch-protection.yml', [
    'schema_version: 1',
    'provider: github',
    'required_pull_request_reviews:',
    'required_status_checks:',
  ]);
  if (scalar(policy, 'branch') !== 'main') errors.push('branch protection must target main');
  if (booleanScalar(policy, 'require_pull_request') !== true) errors.push('main must require pull requests');
  const approvals = Number.parseInt(scalar(policy, 'required_approving_review_count') ?? '', 10);
  if (!Number.isInteger(approvals) || approvals < 1) errors.push('main must require at least one approving review');
  if (booleanScalar(policy, 'require_code_owner_reviews') !== true) errors.push('main must require CODEOWNERS review');
  if (booleanScalar(policy, 'dismiss_stale_reviews') !== true) errors.push('stale approvals must be dismissed');
  if (booleanScalar(policy, 'require_last_push_approval') !== true) errors.push('last-push approval is required');
  if (booleanScalar(policy, 'strict') !== true) errors.push('required status checks must be strict');
  if (booleanScalar(policy, 'enforce_admins') !== true) errors.push('administrators must be subject to protection');
  if (booleanScalar(policy, 'required_conversation_resolution') !== true) errors.push('conversation resolution must be required');
  if (booleanScalar(policy, 'require_linear_history') !== true) errors.push('linear history must be required');
  if (booleanScalar(policy, 'allow_force_pushes') !== false) errors.push('force pushes must be disabled');
  if (booleanScalar(policy, 'allow_deletions') !== false) errors.push('branch deletion must be disabled');
  for (const context of REQUIRED_CI_CONTEXTS) {
    if (!policy.includes(`"${context}"`)) errors.push(`required CI context is missing: ${context}`);
  }
}

function validateCodeowners() {
  const text = requireText('.github/CODEOWNERS', ['* @ssh-client/core-maintainers', '/architecture/', '/protocol/', '/.github/', '/scripts/']);
  const entries = text.split(/\r?\n/).filter((line) => line.trim() && !line.trim().startsWith('#'));
  if (!entries.some((line) => /^\*\s+@[A-Za-z0-9][A-Za-z0-9/_-]*/.test(line))) errors.push('CODEOWNERS must define a default owner');
  for (const entry of entries) {
    if (!/^\S+\s+@[A-Za-z0-9][A-Za-z0-9/_-]*(\s+@[A-Za-z0-9][A-Za-z0-9/_-]*)*$/.test(entry.trim())) errors.push(`invalid CODEOWNERS entry: ${entry}`);
  }
}

function validateWorkflow() {
  const workflow = requireText('.github/workflows/ci.yml', [
    'name: CI',
    'pull_request:',
    'runs-on: windows-latest',
    'name: ci / governance',
    'name: ci / cargo',
    'name: ci / license',
    'name: ci / toolchain-contract',
    'name: ci / supply-chain',
    'name: ci / fuzz-smoke',
    'name: ci / control-ledger',
    'name: ci / platform-windows',
    'name: ci / platform-macos',
    'name: ci / platform-ios',
    'name: ci / platform-linux',
    'name: ci / platform-android',
    'validate-git-governance.mjs',
    'validate-control.ps1',
  ]);
  if (!workflow.includes('permissions:\n  contents: read')) errors.push('CI must use read-only contents permission');
}

function validateGitConfig() {
  const gitProbe = spawnSync('git', ['-C', ROOT, 'rev-parse', '--git-dir'], { encoding: 'utf8' });
  if (gitProbe.status !== 0) {
    errors.push('Git repository is not initialized');
    return;
  }
  const settings = new Map([
    ['core.hooksPath', '.githooks'],
    ['commit.template', '.gitmessage'],
    ['init.defaultBranch', 'main'],
    ['pull.ff', 'only'],
    ['fetch.prune', 'true'],
    ['rerere.enabled', 'true'],
  ]);
  for (const [key, expected] of settings) {
    const result = spawnSync('git', ['-C', ROOT, 'config', '--get', key], { encoding: 'utf8' });
    if (result.status !== 0 || result.stdout.trim() !== expected) errors.push(`git config ${key} must equal ${expected}`);
  }
  const branch = spawnSync('git', ['-C', ROOT, 'branch', '--show-current'], { encoding: 'utf8' });
  if (branch.status === 0 && branch.stdout.trim() && branch.stdout.trim() !== 'main') errors.push(`current branch must be main during initialization, got ${branch.stdout.trim()}`);
}

function validateHooks() {
  const commitHook = exists('.githooks/commit-msg')
    ? requireText('.githooks/commit-msg', ['#!/bin/sh', 'validate-commit-message.mjs'])
    : '';
  const pushHook = exists('.githooks/pre-push')
    ? requireText('.githooks/pre-push', ['#!/bin/sh', 'validate-git-governance.mjs', 'validate-control.ps1'])
    : '';
  if ((commitHook && !commitHook.includes('set -eu')) || (pushHook && !pushHook.includes('set -eu'))) errors.push('Git hooks must fail closed with set -eu');
}

function validateTemplatesAndDocs() {
  requireText('.gitignore', ['**/target/', '*.pem', '*.key', 'known_hosts']);
  requireText('.gitattributes', ['* text=auto', '*.ps1 text eol=crlf', '*.sh text eol=lf']);
  requireText('.gitmessage', ['Conventional Commits', 'BREAKING CHANGE']);
  requireText('.github/pull_request_template.md', ['I ran the tests listed in the control document', 'No secrets', 'Third-party notices']);
  requireText('.github/ISSUE_TEMPLATE/bug_report.yml', ['name:', 'body:', 'reproduction', 'required: true']);
  requireText('.github/ISSUE_TEMPLATE/feature_request.yml', ['name:', 'body:', 'acceptance', 'required: true']);
  if (exists('docs/GIT_WORKFLOW.md')) requireText('docs/GIT_WORKFLOW.md', ['main', 'Pull Request', 'CODEOWNERS', 'CI', 'Conventional Commits']);
  requireText('scripts/validate-commit-message.mjs', ['Conventional Commits', 'WIP', 'do not merge']);
}

try {
  for (const file of requiredFiles) read(file);
  validateBranchProtection();
  validateCodeowners();
  validateWorkflow();
  validateHooks();
  validateTemplatesAndDocs();
  validateGitConfig();
  if (errors.length > 0) throw new Error(errors.join('; '));
  console.log(`git governance validated: root=${ROOT}, branch=main, pull_request_reviews=1+, required_ci=${REQUIRED_CI_CONTEXTS.length}`);
} catch (error) {
  console.error(`git governance validation failed: ${error.message}`);
  process.exit(1);
}
