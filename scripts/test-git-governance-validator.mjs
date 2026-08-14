import assert from 'node:assert/strict';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.cwd());
const VALIDATOR = resolve(ROOT, 'scripts/validate-git-governance.mjs');
const INIT_SCRIPT = resolve(ROOT, 'scripts/init-git-governance.ps1');

assert.equal(existsSync(VALIDATOR), true, 'git governance validator must exist');
assert.equal(existsSync(INIT_SCRIPT), true, 'git governance initializer must exist');

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
  '.githooks/commit-msg',
  '.githooks/pre-push',
  'docs/GIT_WORKFLOW.md',
  'scripts/init-git-governance.ps1',
  'scripts/validate-commit-message.mjs',
  'scripts/validate-git-governance.mjs',
];
for (const relativePath of requiredFiles) {
  assert.equal(existsSync(resolve(ROOT, relativePath)), true, `missing governance artifact: ${relativePath}`);
}

function runValidator(root) {
  return spawnSync(process.execPath, [VALIDATOR, root], { cwd: ROOT, encoding: 'utf8' });
}

function runInit(root) {
  return spawnSync('powershell.exe', [
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', INIT_SCRIPT,
    '-RepositoryRoot', root,
  ], { cwd: ROOT, encoding: 'utf8' });
}

const rootInit = runInit(ROOT);
assert.equal(rootInit.status, 0, `${rootInit.stdout}\n${rootInit.stderr}`);
const baseline = runValidator(ROOT);
assert.equal(baseline.status, 0, `${baseline.stdout}\n${baseline.stderr}`);

const tempRoot = mkdtempSync(join(tmpdir(), 'ssh-git-governance-'));
try {
  for (const relativePath of requiredFiles) {
    const source = resolve(ROOT, relativePath);
    const target = join(tempRoot, relativePath);
    if (relativePath.includes('/')) {
      const parent = target.slice(0, target.lastIndexOf('\\'));
      if (!existsSync(parent)) mkdirSync(parent, { recursive: true });
    }
    cpSync(source, target);
  }

  const init = runInit(tempRoot);
  assert.equal(init.status, 0, `${init.stdout}\n${init.stderr}`);
  const initialized = runValidator(tempRoot);
  assert.equal(initialized.status, 0, `${initialized.stdout}\n${initialized.stderr}`);

  const git = (args, options = {}) => spawnSync('git', ['-C', tempRoot, ...args], { encoding: 'utf8', ...options });
  assert.equal(git(['config', '--get', 'core.hooksPath']).stdout.trim(), '.githooks');
  assert.equal(git(['config', '--get', 'commit.template']).stdout.trim(), '.gitmessage');
  assert.equal(git(['config', '--get', 'init.defaultBranch']).stdout.trim(), 'main');

  writeFileSync(join(tempRoot, 'README.md'), 'governance fixture\n', 'utf8');
  assert.equal(git(['add', 'README.md']).status, 0);
  const badCommit = git(['-c', 'user.name=Governance Test', '-c', 'user.email=governance@example.invalid', 'commit', '-m', 'bad message']);
  assert.notEqual(badCommit.status, 0, 'commit-msg hook must reject non-conventional messages');
  const goodCommit = git(['-c', 'user.name=Governance Test', '-c', 'user.email=governance@example.invalid', 'commit', '-m', 'docs: validate governance']);
  assert.equal(goodCommit.status, 0, `${goodCommit.stdout}\n${goodCommit.stderr}`);

  assert.equal(git(['checkout', '-b', 'feature/governance-test']).status, 0);
  writeFileSync(join(tempRoot, 'feature.txt'), 'feature\n', 'utf8');
  assert.equal(git(['add', 'feature.txt']).status, 0);
  assert.equal(git(['-c', 'user.name=Governance Test', '-c', 'user.email=governance@example.invalid', 'commit', '-m', 'test: exercise governance']).status, 0);
} finally {
  rmSync(tempRoot, { recursive: true, force: true });
}

function expectInvalid(relativePath, mutate, expected) {
  const fixtureRoot = mkdtempSync(join(tmpdir(), 'ssh-git-governance-negative-'));
  try {
    for (const sourcePath of requiredFiles) {
      const source = resolve(ROOT, sourcePath);
      const target = join(fixtureRoot, sourcePath);
      const parent = target.slice(0, target.lastIndexOf('\\'));
      if (!existsSync(parent)) mkdirSync(parent, { recursive: true });
      cpSync(source, target);
    }
    mutate(join(fixtureRoot, relativePath));
    const result = runValidator(fixtureRoot);
    assert.notEqual(result.status, 0, `${relativePath} negative fixture must fail`);
    assert.match(`${result.stdout}\n${result.stderr}`, expected);
  } finally {
    rmSync(fixtureRoot, { recursive: true, force: true });
  }
}

expectInvalid('.github/CODEOWNERS', (file) => writeFileSync(file, '# no owners\n', 'utf8'), /CODEOWNERS/i);
expectInvalid('.github/branch-protection.yml', (file) => writeFileSync(file, readFileSync(file, 'utf8').replace('required_approving_review_count: 1', 'required_approving_review_count: 0'), 'utf8'), /review|approval/i);
expectInvalid('.github/workflows/ci.yml', (file) => writeFileSync(file, readFileSync(file, 'utf8').replace('name: CI', 'name: Broken'), 'utf8'), /workflow|CI/i);

console.log('git governance validator contract passed');
