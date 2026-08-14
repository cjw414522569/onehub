#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';

const ROOT = resolve(process.argv[2] ?? process.cwd());
const OUT = resolve(ROOT, 'artifacts/reports/SBOM_CDX.json');
const METADATA_FILES = [
  'artifacts/license-metadata-wgpu-full.json',
  'artifacts/license-metadata-terminal-full.json',
  'artifacts/license-metadata-ssh-full.json',
  'artifacts/license-metadata-contract-full.json',
];

function readJson(file) {
  const absolute = resolve(ROOT, file);
  if (!existsSync(absolute)) throw new Error(`missing metadata: ${file}`);
  const bytes = readFileSync(absolute);
  const text = bytes[0] === 0xff && bytes[1] === 0xfe
    ? bytes.toString('utf16le', 2)
    : bytes.toString('utf8').replace(/^\uFEFF/, '');
  return JSON.parse(text);
}

function sha256(file) {
  return createHash('sha256').update(readFileSync(resolve(ROOT, file))).digest('hex');
}

function purlFor(pkg) {
  if (typeof pkg.source !== 'string') return null;
  if (pkg.source.startsWith('registry+')) return `pkg:cargo/${pkg.name}@${pkg.version}`;
  if (pkg.source.startsWith('git+')) return `pkg:cargo/${pkg.name}@${pkg.version}?repository_url=${encodeURIComponent(pkg.source.replace(/^git\+/, ''))}`;
  return null;
}

function componentFor(pkg) {
  const component = {
    type: 'library',
    'bom-ref': pkg.id,
    name: pkg.name,
    version: pkg.version,
  };
  if (typeof pkg.description === 'string' && pkg.description.trim()) component.description = pkg.description;
  if (typeof pkg.license === 'string' && pkg.license.trim()) {
    component.licenses = [{ license: { id: pkg.license } }];
  }
  const purl = purlFor(pkg);
  if (purl) component.purl = purl;
  if (typeof pkg.repository === 'string' && pkg.repository.trim()) {
    component.externalReferences = [{ type: 'vcs', url: pkg.repository }];
  }
  return component;
}

const packagesById = new Map();
const resolveNodes = new Map();
const metadataInputs = [];
for (const file of METADATA_FILES) {
  const metadata = readJson(file);
  metadataInputs.push({
    path: file,
    sha256: sha256(file),
    bytes: statSync(resolve(ROOT, file)).size,
    package_count: metadata.packages.length,
    workspace_members: metadata.workspace_members ?? [],
  });
  for (const pkg of metadata.packages) {
    if (!packagesById.has(pkg.id)) packagesById.set(pkg.id, pkg);
  }
  for (const node of metadata.resolve?.nodes ?? []) {
    if (!resolveNodes.has(node.id)) resolveNodes.set(node.id, node.dependencies ?? []);
  }
}

const components = [...packagesById.values()].map(componentFor);
const componentIds = new Set([...packagesById.keys()]);
const dependencies = [];
const seenEdges = new Set();
for (const [nodeId, dependsOn] of resolveNodes) {
  if (!componentIds.has(nodeId)) continue;
  const filtered = dependsOn.filter((dep) => componentIds.has(dep));
  if (filtered.length === 0) continue;
  const edge = JSON.stringify([nodeId, filtered]);
  if (seenEdges.has(edge)) continue;
  seenEdges.add(edge);
  dependencies.push({ ref: nodeId, dependsOn: filtered });
}

const sbom = {
  bomFormat: 'CycloneDX',
  specVersion: '1.5',
  serialNumber: `urn:uuid:${randomUUID()}`,
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    tools: [{ vendor: 'multi-platform-ssh-client', name: 'generate-sbom', version: '1.0.0' }],
    component: {
      type: 'application',
      name: 'multi-platform-ssh-client',
      version: '0.1.0',
    },
    properties: metadataInputs.map((input) => ({
      name: 'source-metadata',
      value: `${input.path} sha256=${input.sha256} bytes=${input.bytes} packages=${input.package_count}`,
    })),
  },
  components,
  dependencies,
};

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, `${JSON.stringify(sbom, null, 2)}\n`, 'utf8');
console.log(`SBOM generated: ${OUT} (components=${components.length}, dependencies=${dependencies.length}, metadata_inputs=${metadataInputs.length})`);