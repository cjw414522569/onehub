#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const RUST_ROOTS = ['crates/', 'apps/', 'services/'];
const PLATFORM_CLIENTS = [
  {
    path: 'clients/windows',
    status: 'windows-first-buildable',
    bridge: 'abi-c',
    requiresCargo: true,
  },
  {
    path: 'clients/apple',
    status: 'interface-only',
    bridge: 'bindings-swift',
    requiresCargo: false,
  },
  {
    path: 'clients/android',
    status: 'interface-only',
    bridge: 'bindings-kotlin',
    requiresCargo: false,
  },
  {
    path: 'clients/linux',
    status: 'interface-only',
    bridge: 'abi-c',
    requiresCargo: false,
  },
  {
    path: 'clients/web',
    status: 'interface-only',
    bridge: 'wasm',
    requiresCargo: false,
  },
];
const PLATFORM_BINDINGS = [
  { path: 'bindings/swift', bridge: 'abi-c' },
  { path: 'bindings/kotlin', bridge: 'abi-c' },
  { path: 'bindings/csharp', bridge: 'abi-c' },
];

function exists(root, relativePath) {
  return fs.existsSync(path.join(root, relativePath));
}

function read(root, relativePath) {
  return fs.readFileSync(path.join(root, relativePath), 'utf8');
}

function hasWorkspaceMember(cargoToml, memberPath) {
  const normalized = memberPath.replaceAll('\\', '/');
  const escaped = normalized.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`['"]${escaped}['"]`).test(cargoToml);
}

function expectedRustModules(rules) {
  return rules.modules
    .filter((module) => RUST_ROOTS.some((prefix) => module.path.startsWith(prefix)))
    .map((module) => module.path)
    .concat('clients/windows');
}

function parseJson(root, relativePath, errors) {
  try {
    return JSON.parse(read(root, relativePath));
  } catch (error) {
    errors.push(`${relativePath} is missing or invalid JSON: ${error.message}`);
    return null;
  }
}

function parseCargoPathDependencies(cargoToml) {
  const section = cargoToml.match(/\[dependencies\]([\s\S]*?)(?=\n\s*\[[^\]]+\]|$)/)?.[1] ?? '';
  const dependencies = [];
  for (const line of section.split(/\r?\n/)) {
    const match = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*\{\s*path\s*=\s*["']([^"']+)["']/);
    if (match) dependencies.push({ name: match[1], relativePath: match[2] });
  }
  return dependencies;
}

export function validateWorkspace(rootDir) {
  const root = path.resolve(rootDir);
  const errors = [];
  let rules;

  if (!exists(root, 'architecture/dependency-rules.json')) {
    errors.push('Missing architecture/dependency-rules.json.');
    return errors;
  }
  rules = parseJson(root, 'architecture/dependency-rules.json', errors);
  if (!rules || !Array.isArray(rules.modules)) return errors;

  if (!exists(root, 'Cargo.toml')) {
    errors.push('Missing root Cargo.toml workspace manifest.');
  } else {
    const cargoToml = read(root, 'Cargo.toml');
    if (!/\[workspace\]/.test(cargoToml)) errors.push('Root Cargo.toml must declare [workspace].');
    if (!/resolver\s*=\s*["']2["']/.test(cargoToml)) errors.push('Root Cargo.toml must use resolver = "2".');
    for (const member of expectedRustModules(rules)) {
      if (!hasWorkspaceMember(cargoToml, member)) {
        errors.push(`Root workspace is missing member: ${member}`);
      }
    }
  }

  for (const module of rules.modules) {
    if (!exists(root, module.path)) {
      errors.push(`Missing module directory: ${module.path}`);
      continue;
    }
    if (!exists(root, `${module.path}/README.md`)) {
      errors.push(`Missing module README: ${module.path}/README.md`);
    }
  }

  for (const modulePath of expectedRustModules(rules)) {
    if (!exists(root, `${modulePath}/Cargo.toml`)) {
      errors.push(`Missing Rust manifest: ${modulePath}/Cargo.toml`);
    }
    if (!exists(root, `${modulePath}/src`)) {
      errors.push(`Missing Rust source directory: ${modulePath}/src`);
    }
    if (!exists(root, `${modulePath}/src/lib.rs`) && !exists(root, `${modulePath}/src/main.rs`)) {
      errors.push(`Missing Rust entrypoint: ${modulePath}/src/lib.rs or src/main.rs`);
    }
  }

  const moduleByPath = new Map(rules.modules.map((module) => [module.path.replaceAll('\\', '/'), module]));
  moduleByPath.set('clients/windows', { id: 'clients-windows', path: 'clients/windows', dependencies: ['abi-c'] });
  for (const modulePath of expectedRustModules(rules)) {
    if (!exists(root, `${modulePath}/Cargo.toml`)) continue;
    const module = moduleByPath.get(modulePath);
    if (!module) {
      errors.push(`Rust module is not represented by dependency rules: ${modulePath}`);
      continue;
    }
    const manifest = read(root, `${modulePath}/Cargo.toml`);
    const actualIds = [];
    for (const dependency of parseCargoPathDependencies(manifest)) {
      const target = path.normalize(path.join(root, modulePath, dependency.relativePath));
      const targetPath = path.relative(root, target).replaceAll('\\', '/');
      const targetModule = moduleByPath.get(targetPath);
      if (!targetModule) {
        errors.push(`${modulePath} has path dependency outside dependency rules: ${dependency.relativePath}`);
        continue;
      }
      actualIds.push(targetModule.id);
    }
    const expectedIds = [...new Set((module.dependencies ?? []).map(String))].sort();
    const actualSorted = [...new Set(actualIds)].sort();
    if (expectedIds.join('|') !== actualSorted.join('|')) {
      errors.push(`${modulePath} Cargo path dependencies [${actualSorted.join(', ')}] do not match ADR-002 [${expectedIds.join(', ')}].`);
    }
  }

  const coreForbidden = (rules.required_rules?.core_forbidden_external_imports ?? []).map(String);
  for (const module of rules.modules.filter((item) => ['L0', 'L1'].includes(String(item.layer)))) {
    for (const sourceName of ['src/lib.rs', 'src/main.rs']) {
      const sourcePath = `${module.path}/${sourceName}`;
      if (!exists(root, sourcePath)) continue;
      const source = read(root, sourcePath).toLowerCase();
      for (const forbidden of coreForbidden) {
        if (source.includes(forbidden.toLowerCase())) {
          errors.push(`${sourcePath} contains forbidden core import token: ${forbidden}`);
        }
      }
    }
  }

  for (const client of PLATFORM_CLIENTS) {
    if (!exists(root, `${client.path}/README.md`)) {
      errors.push(`Missing client README: ${client.path}/README.md`);
    }
    const contract = parseJson(root, `${client.path}/contract.json`, errors);
    if (contract) {
      if (contract.path !== client.path) errors.push(`${client.path}/contract.json has wrong path.`);
      if (contract.status !== client.status) errors.push(`${client.path}/contract.json must declare status=${client.status}.`);
      if (contract.approved_bridge !== client.bridge) errors.push(`${client.path}/contract.json must use approved bridge ${client.bridge}.`);
      if (client.requiresCargo && contract.build_verification !== 'cargo-check-on-windows') {
        errors.push(`${client.path}/contract.json must declare cargo-check-on-windows verification.`);
      }
      if (!client.requiresCargo && contract.build_verification !== 'blocked-unavailable-toolchain') {
        errors.push(`${client.path}/contract.json must declare blocked-unavailable-toolchain verification.`);
      }
    }
  }

  for (const binding of PLATFORM_BINDINGS) {
    if (!exists(root, `${binding.path}/INTERFACE.md`)) {
      errors.push(`Missing binding interface boundary: ${binding.path}/INTERFACE.md`);
    }
    const contract = parseJson(root, `${binding.path}/contract.json`, errors);
    if (contract) {
      if (contract.path !== binding.path) errors.push(`${binding.path}/contract.json has wrong path.`);
      if (contract.status !== 'interface-only') errors.push(`${binding.path}/contract.json must declare status=interface-only.`);
      if (contract.approved_bridge !== binding.bridge) errors.push(`${binding.path}/contract.json must use approved bridge ${binding.bridge}.`);
    }
  }

  for (const requiredFile of ['tools/README.md']) {
    if (!exists(root, requiredFile)) errors.push(`Missing T016 support file: ${requiredFile}`);
  }

  return errors;
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  const root = process.argv[2] ?? process.cwd();
  const errors = validateWorkspace(root);
  if (errors.length > 0) {
    console.error(`Workspace contract failed with ${errors.length} error(s):`);
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
  } else {
    console.log('Workspace contract valid: Rust workspace, platform boundaries, bindings, tools, and docs verified.');
  }
}
