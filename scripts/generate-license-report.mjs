import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';

const ROOT = resolve(process.cwd());
const OUT_JSON = resolve(ROOT, 'artifacts/reports/LICENSE_COMPLIANCE.json');
const OUT_MD = resolve(ROOT, 'artifacts/reports/LICENSE_COMPLIANCE.md');
const OUT_THIRD_PARTY = resolve(ROOT, 'artifacts/reports/THIRD_PARTY_LICENSES.md');
const METADATA_FILES = [
  'artifacts/license-metadata-wgpu-full.json',
  'artifacts/license-metadata-terminal-full.json',
  'artifacts/license-metadata-ssh-full.json',
  'artifacts/license-metadata-contract-full.json',
];
const RELEASE_ROOTS = new Set(['wgpu-terminal']);
const DEVELOPMENT_ROOTS = new Set(['terminal-engine-spike', 'ssh-engine-spike-probe', 'terminal-contract']);

function readJson(file) {
  const bytes = readFileSync(resolve(ROOT, file));
  const text = bytes[0] === 0xff && bytes[1] === 0xfe
    ? bytes.toString('utf16le', 2)
    : bytes.toString('utf8').replace(/^\uFEFF/, '');
  return JSON.parse(text);
}

function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function fileEvidence(file) {
  const absolute = resolve(ROOT, file);
  return {
    path: file,
    exists: existsSync(absolute),
    bytes: existsSync(absolute) ? statSync(absolute).size : 0,
    sha256: existsSync(absolute) ? sha256(absolute) : null,
  };
}

function packageLicenseEvidence(pkg) {
  const manifestPath = typeof pkg.manifest_path === 'string' ? pkg.manifest_path : null;
  const packageDir = manifestPath ? dirname(manifestPath) : null;
  const candidates = [];
  if (typeof pkg.license_file === 'string' && pkg.license_file.trim()) {
    candidates.push(pkg.license_file.trim());
  }

  if (packageDir && existsSync(packageDir)) {
    try {
      for (const name of readdirSync(packageDir)) {
        if (/^(license|copying|notice|unlicense|patents?)([-_.].*)?$/i.test(name)) candidates.push(name);
      }
    } catch {
      // A package may disappear from the local Cargo cache after metadata generation.
    }
  }

  const found = [];
  const seen = new Set();
  for (const candidate of candidates) {
    if (seen.has(candidate)) continue;
    seen.add(candidate);
    const absolute = packageDir
      ? (isAbsolute(candidate) ? candidate : resolve(packageDir, candidate))
      : null;
    if (!absolute || !existsSync(absolute)) continue;
    try {
      if (!statSync(absolute).isFile()) continue;
      found.push({
        path: candidate.replaceAll('\\', '/'),
        bytes: statSync(absolute).size,
        sha256: sha256(absolute),
      });
    } catch {
      // Treat unreadable evidence as not located; the report remains review_required.
    }
  }

  return {
    status: found.length > 0 ? 'located' : 'not_located',
    review_required: found.length === 0,
    files: found,
  };
}

function licenseClass(license) {
  const value = String(license ?? '');
  if (!value) return 'review_required';
  if (/WTFPL|GPL|AGPL/.test(value)) return 'restricted_copyleft';
  return 'allowlist_candidate';
}

function rootPackageNames(metadata) {
  return metadata.packages.filter((pkg) => pkg.source == null).map((pkg) => pkg.name);
}

function collectPackages() {
  const map = new Map();
  const roots = [];
  const metadataInputs = [];
  for (const file of METADATA_FILES) {
    if (!existsSync(resolve(ROOT, file))) throw new Error(`missing metadata: ${file}`);
    const metadata = readJson(file);
    metadataInputs.push({
      ...fileEvidence(file),
      package_count: metadata.packages.length,
      workspace_members: metadata.workspace_members ?? [],
    });
    const workspaceIds = metadata.workspace_members ?? [];
    const packageById = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
    const nodeById = new Map((metadata.resolve?.nodes ?? []).map((node) => [node.id, node]));
    const rootIds = workspaceIds.filter((id) => packageById.has(id));
    roots.push(...rootPackageNames(metadata));
    const queue = rootIds.map((id) => ({ id, scope: RELEASE_ROOTS.has(packageById.get(id).name) ? 'release_candidate' : 'development_only' }));
    const visited = new Map();
    while (queue.length > 0) {
      const current = queue.shift();
      const previous = visited.get(current.id);
      if (previous === 'release_candidate' || previous === current.scope) continue;
      visited.set(current.id, current.scope);
      const node = nodeById.get(current.id);
      for (const dependencyId of node?.dependencies ?? []) {
        queue.push({ id: dependencyId, scope: current.scope });
      }
    }
    for (const pkg of metadata.packages) {
      if (pkg.source == null && !RELEASE_ROOTS.has(pkg.name) && !DEVELOPMENT_ROOTS.has(pkg.name)) continue;
      const key = `${pkg.name}@${pkg.version}`;
      const scope = visited.get(pkg.id) ?? (RELEASE_ROOTS.has(pkg.name) ? 'release_candidate' : DEVELOPMENT_ROOTS.has(pkg.name) ? 'development_only' : 'transitive');
      const existing = map.get(key);
      if (existing) {
        existing.scopes.add(scope);
      } else {
        map.set(key, { pkg, scopes: new Set([scope]) });
      }
    }
  }
  return {
    packages: [...map.values()].map(({ pkg, scopes }) => ({
      ...pkg,
      scope: scopes.has('release_candidate') ? 'release_candidate' : scopes.has('development_only') ? 'development_only' : 'transitive',
    })),
    roots: [...new Set(roots)],
    metadataInputs,
  };
}

function resourceInventory() {
  const extensions = new Set(['.ttf', '.otf', '.woff', '.woff2', '.svg', '.png', '.ico', '.jpg', '.jpeg', '.webp']);
  const found = [];
  function walk(dir) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === 'target' || entry.name === 'node_modules' || entry.name === '.git') continue;
      const absolute = join(dir, entry.name);
      if (entry.isDirectory()) walk(absolute);
      else if (extensions.has(entry.name.slice(entry.name.lastIndexOf('.')).toLowerCase())) {
        found.push({ path: relative(ROOT, absolute).replaceAll('\\', '/'), bytes: statSync(absolute).size, sha256: sha256(absolute), status: 'review_required' });
      }
    }
  }
  walk(ROOT);
  return found;
}

function buildReport() {
  const { packages, roots, metadataInputs } = collectPackages();
  const dependencies = packages.map((pkg) => {
    const scope = pkg.scope;
    const classification = licenseClass(pkg.license);
    const isRoot = pkg.source == null;
    const releaseEligible = scope === 'release_candidate' && classification === 'allowlist_candidate' && Boolean(pkg.license);
    return {
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? null,
      source: pkg.source ?? 'workspace',
      repository: pkg.repository ?? null,
      scope,
      release_eligible: releaseEligible,
      classification,
      link_mode: 'undetermined_until_packaging',
      evidence: isRoot ? 'cargo metadata workspace package' : 'cargo metadata resolved package',
      notes: pkg.license ? '' : 'workspace package has no publish license; project policy is Apache-2.0 and package manifest must be updated before publish',
      license_evidence: packageLicenseEvidence(pkg),
    };
  }).sort((a, b) => `${a.scope}/${a.name}/${a.version}`.localeCompare(`${b.scope}/${b.name}/${b.version}`));

  const restrictions = dependencies.filter((entry) => !entry.release_eligible || entry.classification !== 'allowlist_candidate').map((entry) => ({
    type: entry.classification,
    dependency: `${entry.name}@${entry.version}`,
    scope: entry.scope,
    reason: entry.license ? `license expression requires policy review: ${entry.license}` : 'license expression is missing',
  }));
  const releaseBlockers = restrictions.filter((entry) => entry.scope === 'release_candidate');
  const resources = resourceInventory();
  const cryptoPackages = dependencies.filter((entry) => /sha2|ring|crypto|openssl|rustls|aws-lc|ssh/i.test(entry.name));
  const projectLicense = fileEvidence('LICENSE');
  const projectNotice = fileEvidence('NOTICE');
  const report = {
    schema_version: 1,
    generated_at_utc: new Date().toISOString(),
    project_license: 'Apache-2.0',
    metadata_inputs: metadataInputs,
    scopes: ['release_candidate', 'development_only'],
    roots,
    policy: {
      document: 'docs/LICENSE_POLICY.md',
      spdx: true,
      release_allowlist: ['Apache-2.0', 'MIT', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', '0BSD', 'Unlicense', 'CC0-1.0', 'Unicode-3.0', 'Unicode-DFS-2016', 'NCSA'],
      forbidden_without_review: ['GPL', 'AGPL', 'LGPL', 'WTFPL', 'unknown', 'undeclared'],
    },
    dependencies,
    resources: { status: resources.length === 0 ? 'not_present' : 'review_required', files: resources },
    distribution: {
      project_license_file: projectLicense,
      notice_file: projectNotice,
      linkage_policy: {
        static: 'allowlisted SPDX expressions only; preserve license texts and NOTICE in every artifact',
        dynamic: 'record library version, load path, replacement/relink method, and platform packaging declaration',
        wasm: 'publish an accessible third-party notice page and retain bundle hash/source-map attribution',
      },
      fonts_icons: resources.length === 0 ? 'not_present' : 'review_required',
      third_party_report: {
        path: 'artifacts/reports/THIRD_PARTY_LICENSES.md',
        exists: false,
        bytes: 0,
        sha256: null,
      },
    },
    cryptography: {
      status: 'review_required',
      export_review_required: true,
      dependencies: cryptoPackages.map((entry) => `${entry.name}@${entry.version}`),
      required_actions: ['confirm applicable export classification and jurisdictional obligations', 'record legal owner and review date before commercial release'],
    },
    restrictions,
    release_blockers: releaseBlockers,
    summary: {
      dependency_count: dependencies.length,
      release_dependency_count: dependencies.filter((entry) => entry.scope === 'release_candidate').length,
      development_restriction_count: restrictions.filter((entry) => entry.scope === 'development_only' || entry.scope === 'transitive').length,
      release_blocker_count: releaseBlockers.length,
      resource_count: resources.length,
    },
    status: releaseBlockers.length === 0 ? 'pass_with_restrictions' : 'review_required',
  };
  return report;
}

function renderMarkdown(report) {
  const lines = [
    '# License compliance report',
    '',
    `Generated: ${report.generated_at_utc}`,
    `Status: **${report.status}**`,
    `Project license: **${report.project_license}**`,
    '',
    '## Summary',
    '',
    `- Dependencies: ${report.summary.dependency_count}; release-candidate: ${report.summary.release_dependency_count}; development/transitive restrictions: ${report.summary.development_restriction_count}.`,
    `- Release blockers: ${report.summary.release_blocker_count}.`,
    `- Fonts/icons/resources: ${report.resources.status}; files=${report.summary.resource_count}.`,
    `- Cryptography/export review: ${report.cryptography.status}; packages=${report.cryptography.dependencies.length}.`,
    '',
    '## Restrictions',
    '',
    '| Dependency | Scope | License | Classification | Reason |',
    '|---|---|---|---|---|',
    ...report.restrictions.map((entry) => `| ${entry.dependency} | ${entry.scope} | — | ${entry.type} | ${entry.reason} |`),
    '',
    '## Policy',
    '',
    '- Policy document: `docs/LICENSE_POLICY.md`.',
    '- Static/dynamic link mode remains `undetermined_until_packaging` until each target artifact is built and scanned.',
    '- `review_required` means evidence is missing or legal review is required; it is never treated as a release pass.',
    '',
  ];
  return `${lines.join('\n')}\n`;
}

function markdownCell(value) {
  return String(value ?? '').replaceAll('|', '\\|').replaceAll('\r', ' ').replaceAll('\n', ' ');
}

function renderThirdPartyMarkdown(report) {
  const lines = [
    '# Third-party license inventory',
    '',
    `Generated: ${report.generated_at_utc}`,
    `Project license: **${report.project_license}**`,
    `Dependencies: **${report.summary.dependency_count}**`,
    '',
    'Each row records the Cargo SPDX/license expression, dependency scope, source metadata, release eligibility, and whether a local license text/file could be located. `review_required=yes` is never a release approval.',
    '',
    '| Name | Version | SPDX/license expression | Scope | Source | Repository | Release eligible | License evidence | Review required |',
    '|---|---|---|---|---|---|---|---|---|',
    ...report.dependencies.map((entry) => {
      const evidence = entry.license_evidence?.files?.map((file) => file.path).join('; ') || 'not located';
      return `| ${markdownCell(entry.name)} | ${markdownCell(entry.version)} | ${markdownCell(entry.license)} | ${markdownCell(entry.scope)} | ${markdownCell(entry.source)} | ${markdownCell(entry.repository || 'not declared')} | ${entry.release_eligible ? 'yes' : 'no'} | ${markdownCell(entry.license_evidence?.status === 'located' ? evidence : 'not located')} | ${entry.license_evidence?.review_required ? 'yes' : 'no'} |`;
    }),
    '',
    '## Review notes',
    '',
    '- `not_located` means the package SPDX expression is present but a local license text/file could not be located from Cargo metadata or conventional package-root filenames.',
    '- `restricted_copyleft` and `review_required` dependencies must not enter a release artifact without an explicit policy/legal decision.',
    '- The exact third-party license texts and notices for a commercial artifact must be regenerated from the final packaging dependency/linkage set.',
    '',
  ];
  return `${lines.join('\n')}\n`;
}

const report = buildReport();
mkdirSync(dirname(OUT_JSON), { recursive: true });
writeFileSync(OUT_THIRD_PARTY, renderThirdPartyMarkdown(report), 'utf8');
report.distribution.third_party_report = fileEvidence('artifacts/reports/THIRD_PARTY_LICENSES.md');
writeFileSync(OUT_JSON, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
writeFileSync(OUT_MD, renderMarkdown(report), 'utf8');
console.log(`license report generated: status=${report.status}, dependencies=${report.summary.dependency_count}, release_blockers=${report.summary.release_blocker_count}, resources=${report.summary.resource_count}`);
