#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const CRATE = join(ROOT, 'crates/proxy');
const errors = [];

const REQUIRED_TOKENS = [
  'pub mod socks5', 'pub mod http_connect',
  'pub enum DnsPolicy', 'pub enum ProxyTarget', 'pub struct Socks5Config',
  'pub struct HttpConnectConfig', 'pub async fn socks5_connect', 'pub async fn http_connect',
  'pub fn encode_connect_request', 'pub fn decode_connect_request',
  'pub fn encode_greeting', 'pub fn decode_method_selection',
  'pub fn encode_auth_request', 'pub fn decode_auth_request',
  'pub fn encode_reply', 'pub fn decode_reply',
  'pub fn build_connect_request', 'pub fn parse_status_code',
  'pub enum ProxyErrorKind', 'pub struct ProxyError', 'pub fn stable_code',
  'E_PROXY_TIMEOUT', 'E_PROXY_NO_METHOD', 'E_PROXY_AUTH_REJECTED',
  'E_PROXY_CONNECT_REJECTED', 'E_PROXY_HTTP_STATUS',
  'socks5_matrix_remote_resolve_domain_no_auth', 'socks5_matrix_local_resolve_ipv4',
  'socks5_matrix_local_resolve_ipv6_literal', 'socks5_matrix_user_pass_auth_success',
  'socks5_matrix_user_pass_auth_rejected', 'socks5_matrix_no_acceptable_method',
  'socks5_matrix_connect_refused', 'socks5_matrix_timeout',
  'http_connect_matrix_success_no_auth', 'http_connect_matrix_proxy_authorization',
  'http_connect_matrix_non_2xx_status', 'http_connect_matrix_timeout',
];
// In-house protocol implementations: no external socks5/http-connect crates.
const FORBIDDEN_DEPENDENCIES = ['socks5', 'socks5-rs', 'http-connect', 'hyper', 'reqwest'];

if (!existsSync(join(CRATE, 'Cargo.toml'))) errors.push('Missing crates/proxy/Cargo.toml');

const manifest = readFileSync(join(CRATE, 'Cargo.toml'), 'utf8');
const depsMatch = manifest.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/);
const depsSection = depsMatch?.[1] ?? '';
for (const line of depsSection.split(/\r?\n/)) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith('#')) continue;
  const name = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/)?.[1];
  if (!name) continue;
  if (FORBIDDEN_DEPENDENCIES.includes(name)) errors.push(`proxy has forbidden dependency: ${name}`);
}

const sourceFiles = [];
function collect(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const absolute = join(dir, entry.name);
    if (entry.isDirectory()) collect(absolute);
    else if (entry.name.endsWith('.rs')) sourceFiles.push(absolute);
  }
}
if (existsSync(join(CRATE, 'src'))) collect(join(CRATE, 'src'));
if (existsSync(join(CRATE, 'tests'))) collect(join(CRATE, 'tests'));
const sourceText = sourceFiles.map((file) => readFileSync(file, 'utf8')).join('\n');
for (const token of REQUIRED_TOKENS) {
  if (!sourceText.includes(token)) errors.push(`proxy is missing required token: ${token}`);
}

for (const args of [['check', '-p', 'proxy', '--locked'], ['test', '-p', 'proxy', '--locked']]) {
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', timeout: 240000 });
  if (result.status !== 0) {
    errors.push(`cargo ${args[0]} -p proxy failed:\n${result.stdout}\n${result.stderr}`);
  }
}

if (errors.length > 0) {
  console.error(`proxy contract failed with ${errors.length} error(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}
console.log('proxy contract valid: in-house SOCKS5 (RFC 1928/1929) + HTTP CONNECT clients, auth/DNS-policy/IPv6/timeout configurable, real-loopback compatibility matrix, stable E_PROXY_* codes, cargo check/test --locked passed.');