#!/usr/bin/env node

// T136 contract: gateway target address policy and SSRF protection.

import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const GATEWAY = join(ROOT, 'services/gateway');
const errors = [];

const TOKENS = [
  'pub const DEFAULT_ALLOWED_PORTS', 'pub struct AddressPolicy', 'pub enum AddressPolicyError',
  'pub struct ResolvedTarget', 'pub fn check_ip', 'pub fn evaluate', 'pub fn verify_still_valid',
  'pub fn is_private', 'pub fn is_link_local', 'pub fn is_loopback', 'pub fn is_metadata',
  'pub fn is_reserved', 'DnsRebindingDetected', 'MetadataAddress', 'HostNotAllowed',
  'PortNotAllowed', 'dns_rebinding_guard',
  'private_ipv4_denied_by_default', 'private_ipv6_denied_by_default',
  'link_local_ipv4_denied_by_default', 'link_local_ipv6_denied_by_default',
  'loopback_denied_by_default', 'cloud_metadata_ipv4_denied_by_default',
  'cloud_metadata_ipv6_denied_by_default', 'ipv4_mapped_metadata_denied_by_default',
  'public_addresses_allowed_by_default', 'non_ssh_port_denied_by_default',
  'configured_extra_port_allowed', 'private_allowed_when_configured',
  'host_allowlist_required', 'host_allowlist_wildcard_suffix',
  'dns_rebinding_pinned_ip_change_rejected', 'dns_rebinding_same_answer_allowed',
  'empty_resolution_rejected', 'reserved_and_multicast_denied',
];

function collectRs(dir, files) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(absolute, files);
    else if (entry.name.endsWith('.rs')) files.push(absolute);
  }
}

const files = [];
collectRs(join(GATEWAY, 'src'), files);
const sourceText = files.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of TOKENS) {
  if (!sourceText.includes(token)) errors.push(`gateway missing required token: ${token}`);
}

for (const args of [
  ['check', '-p', 'gateway', '--locked'],
  ['test', '-p', 'gateway', '--locked'],
]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p gateway failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`gateway-address-policy contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('gateway-address-policy contract valid: AddressPolicy is a configurable gate over every target (private / link-local / loopback / cloud-metadata / reserved address classes, SSH port policy, host allowlist) with a DNS rebinding guard that pins validated addresses and rejects connect-time re-resolution outside the pinned set; the SSRF bypass test set passes; cargo check/test --locked passed.');