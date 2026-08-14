import { readFileSync } from 'node:fs';

const messageFile = process.argv[2];
if (!messageFile) {
  console.error('commit message file is required');
  process.exit(1);
}

const firstLine = readFileSync(messageFile, 'utf8')
  .split(/\r?\n/)
  .map((line) => line.trim())
  .find((line) => line && !line.startsWith('#')) ?? '';
const conventional = /^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-z0-9][a-z0-9._/-]*\))?!?: [^\r\n]{1,72}$/u;
if (!conventional.test(firstLine)) {
  console.error('commit message must match Conventional Commits: type(scope): subject (subject <=72 chars)');
  process.exit(1);
}
if (/\b(WIP|do not merge)\b/i.test(firstLine)) {
  console.error('WIP and do-not-merge commits are not allowed');
  process.exit(1);
}
console.log(`commit message valid: ${firstLine}`);

