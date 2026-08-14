import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { validateRunnerPreflight } from './validate-runner-preflight.mjs';

const valid = JSON.parse(readFileSync('artifacts/perf/wgpu-feasibility/runner-preflight.json', 'utf8'));
assert.deepEqual(validateRunnerPreflight(valid), []);

const inconsistentStatus = structuredClone(valid);
inconsistentStatus.status = 'ready';
assert.match(validateRunnerPreflight(inconsistentStatus).join('\n'), /status must be partial/);

const inconsistentRunners = structuredClone(valid);
inconsistentRunners.matrix.available_runners = [...inconsistentRunners.matrix.available_runners, 'native_metal'];
assert.match(validateRunnerPreflight(inconsistentRunners).join('\n'), /available_runners does not match/);

const missingField = structuredClone(valid);
delete missingField.runners.native_windows;
assert.match(validateRunnerPreflight(missingField).join('\n'), /missing runner/);

const invalidFixture = spawnSync(process.execPath, [
  'scripts/validate-runner-preflight.mjs',
  'scripts/testdata/invalid-runner-preflight-ready.json',
], { cwd: process.cwd(), encoding: 'utf8' });
assert.equal(invalidFixture.status, 1, `${invalidFixture.stdout}\n${invalidFixture.stderr}`);
assert.match(`${invalidFixture.stdout}\n${invalidFixture.stderr}`, /status must be partial/);

console.log('runner preflight validator contract passed');
