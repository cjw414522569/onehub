#!/usr/bin/env node

// T134 gateway threat-model lint + security review gate.

import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const path = join(ROOT, 'security', 'gateway-threat-model.json');
const model = JSON.parse(readFileSync(path, 'utf8'));
const errors = [];
const REQUIRED_AREAS = ['ssrf', 'internal_access', 'credentials', 'audit', 'rate_limit', 'abuse'];

if (model.schema_version !== 1) errors.push('schema_version must be 1');
if (!model.tenant_boundary || model.tenant_boundary.length === 0) {
  errors.push('tenant boundary must be explicit');
}
if (!model.deployment_topology?.length) errors.push('deployment topology must be listed');

for (const area of REQUIRED_AREAS) {
  const entry = model.controls?.[area];
  if (!entry) {
    errors.push(`missing control area: ${area}`);
    continue;
  }
  if (!entry.threat || entry.threat.length === 0) errors.push(`${area}: missing threat`);
  if (!entry.controls?.length) errors.push(`${area}: missing controls`);
}

if (errors.length > 0) {
  console.error(`gateway-threat-model lint failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gateway-threat-model lint valid: SSRF, internal access, credentials, audit, rate limiting, and abuse each have an explicit threat and controls; the tenant boundary and deployment topology are defined; the security review gate passes.');