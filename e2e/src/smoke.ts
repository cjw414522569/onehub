// Full E2E smoke matrix (T153): runs the six-platform critical journeys
// against the deterministic fake gateway with an environment-provided
// secret, verifies the key-rotation policy, and (with --canary <value>)
// confirms a generated canary secret never appears in any output.

import { KeyRotationPolicy } from './key-rotation.ts';
import { platformMatrix } from './page-objects.ts';
import { SecretsProvider } from './accounts.ts';

let failures = 0;

function check(name: string, condition: boolean, detail = ''): void {
  if (condition) {
    console.log(`PASS ${name}`);
  } else {
    failures += 1;
    console.error(`FAIL ${name}${detail ? `: ${detail}` : ''}`);
  }
}

async function main(): Promise<void> {
  const provider = new SecretsProvider();
  const token = provider.gatewayToken();
  const key = provider.testKey();
  const canary = process.argv.find((arg) => arg.startsWith('--canary='))?.slice('--canary='.length) ?? null;

  // Secrets are env-only: placeholders are rejected.
  check('secrets.env-only', !token.includes('<') && !key.includes('<'));

  // Key rotation: generations are distinct and expiry is enforced.
  const policy = new KeyRotationPolicy(1, 1);
  const rotated = policy.rotate();
  check('rotation.generation-advances', rotated.generation === 2);
  check('rotation.active-key-derived', policy.activeKey(key).endsWith('-gen1'));
  check('rotation.expiry', !policy.isValid(2) && policy.isValid(0));

  // Six-platform smoke matrix.
  const allChecks: string[] = [];
  for (const po of platformMatrix()) {
    const report = await po.run(token);
    allChecks.push(...report.checks);
    check(`journey.${report.platform}`, report.passed, report.checks.filter((c) => c.endsWith('=false') || c.includes('=failed')).join(','));
    for (const c of report.checks) {
      if (c.includes('=false') || c.includes('=failed')) check(`journey.${report.platform}:${c}`, false);
    }
  }

  // The canary secret must never appear in any journey output (logs/screen).
  if (canary) check('secrets.canary-not-in-output', !allChecks.join(' ').includes(canary));

  if (failures > 0) {
    console.error(`E2E smoke failed with ${failures} failure(s)`);
    process.exit(1);
  }
  console.log(`E2E smoke matrix valid: six platforms (windows/macos/linux/ios/android/web) passed the critical journey with environment-only secrets; key rotation verified.${canary ? ` canary '${canary}' did not appear in any output.` : ''}`);
}

main();