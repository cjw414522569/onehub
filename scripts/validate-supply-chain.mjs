#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const BLOCKING_SEVERITIES = new Set(['high', 'critical']);
const KNOWN_LOCKFILE_KINDS = new Set(['cargo', 'toolchain']);
const TODAY_UTC = new Date().toISOString().slice(0, 10);

function readJson(root, relativePath) {
  const absolute = resolve(root, relativePath);
  if (!existsSync(absolute)) return null;
  const text = readFileSync(absolute, 'utf8').replace(/^\uFEFF/, '');
  try {
    return JSON.parse(text);
  } catch (error) {
    return { __invalid_json__: error.message };
  }
}

function isValidDate(value) {
  return typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value) && !Number.isNaN(Date.parse(`${value}T00:00:00Z`));
}

function validateExceptions(root, relativePath, kind, errors) {
  const data = readJson(root, relativePath);
  if (data === null) {
    errors.push(`missing supply-chain policy: ${relativePath}`);
    return [];
  }
  if (data.__invalid_json__) {
    errors.push(`${relativePath} is not valid JSON: ${data.__invalid_json__}`);
    return [];
  }
  if (data.schema_version !== 1) errors.push(`${relativePath} must declare schema_version=1.`);
  if (!Array.isArray(data.exceptions)) errors.push(`${relativePath} must contain an exceptions array.`);
  const exceptions = Array.isArray(data.exceptions) ? data.exceptions : [];
  for (const exception of exceptions) {
    const label = `${kind} exception match=${exception?.match ?? '(missing)'}`;
    if (typeof exception?.match !== 'string' || exception.match.trim().length < 4) {
      errors.push(`${relativePath} ${label} must declare a match of at least 4 characters.`);
    }
    if (typeof exception?.owner !== 'string' || !exception.owner.trim()) {
      errors.push(`${relativePath} ${label} must declare an owner.`);
    }
    if (typeof exception?.reason !== 'string' || !exception.reason.trim()) {
      errors.push(`${relativePath} ${label} must declare a reason.`);
    }
    if (!isValidDate(exception?.expires_at)) {
      errors.push(`${relativePath} ${label} must declare expires_at as YYYY-MM-DD.`);
    } else if (exception.expires_at < TODAY_UTC) {
      errors.push(`${relativePath} ${label} is expired (expires_at=${exception.expires_at}, today=${TODAY_UTC}).`);
    }
  }
  return exceptions;
}

function exceptionMatches(exception, key) {
  return typeof exception?.match === 'string' && exception.match.length > 0 && key.includes(exception.match);
}

export function validateSupplyChain(rootDir) {
  const root = resolve(rootDir);
  const errors = [];
  const summary = { lockfiles: 0, sbom_components: 0, license_status: null, release_blockers: null, vuln_scan: null, vuln_findings: 0, secret_scan: null, secret_findings: 0, vuln_exceptions: 0, secret_exceptions: 0 };

  // 1. Lockfiles
  const lockfilesConfig = readJson(root, 'supply-chain/lockfiles.json');
  if (lockfilesConfig === null) {
    errors.push('missing supply-chain policy: supply-chain/lockfiles.json');
  } else if (lockfilesConfig.__invalid_json__) {
    errors.push(`supply-chain/lockfiles.json is not valid JSON: ${lockfilesConfig.__invalid_json__}`);
  } else {
    if (lockfilesConfig.schema_version !== 1) errors.push('supply-chain/lockfiles.json must declare schema_version=1.');
    if (!Array.isArray(lockfilesConfig.lockfiles) || lockfilesConfig.lockfiles.length === 0) {
      errors.push('supply-chain/lockfiles.json must declare at least one lockfile.');
    } else {
      for (const lockfile of lockfilesConfig.lockfiles) {
        if (typeof lockfile?.path !== 'string' || !lockfile.path.trim()) {
          errors.push('supply-chain/lockfiles.json contains a lockfile without a path.');
          continue;
        }
        if (typeof lockfile?.kind !== 'string' || !KNOWN_LOCKFILE_KINDS.has(lockfile.kind)) {
          errors.push(`lockfile ${lockfile.path} must declare a known kind (${[...KNOWN_LOCKFILE_KINDS].join(', ')}).`);
        }
        if (typeof lockfile?.purpose !== 'string' || !lockfile.purpose.trim()) {
          errors.push(`lockfile ${lockfile.path} must declare a purpose.`);
        }
        if (!existsSync(resolve(root, lockfile.path))) {
          errors.push(`required lockfile is missing: ${lockfile.path}`);
        }
        summary.lockfiles += 1;
      }
    }
  }

  // 2. SBOM
  const sbom = readJson(root, 'artifacts/reports/SBOM_CDX.json');
  if (sbom === null) {
    errors.push('missing supply-chain evidence: artifacts/reports/SBOM_CDX.json');
  } else if (sbom.__invalid_json__) {
    errors.push(`artifacts/reports/SBOM_CDX.json is not valid JSON: ${sbom.__invalid_json__}`);
  } else {
    if (sbom.bomFormat !== 'CycloneDX') errors.push('SBOM must declare bomFormat=CycloneDX.');
    if (typeof sbom.specVersion !== 'string' || !/^1\./.test(sbom.specVersion)) errors.push('SBOM must declare a CycloneDX 1.x specVersion.');
    if (!Array.isArray(sbom.components) || sbom.components.length === 0) errors.push('SBOM must declare at least one component.');
    if (!sbom.metadata?.component?.name || !sbom.metadata?.timestamp) errors.push('SBOM metadata must declare component name and timestamp.');
    summary.sbom_components = Array.isArray(sbom.components) ? sbom.components.length : 0;
  }

  // 3. License report
  const license = readJson(root, 'artifacts/reports/LICENSE_COMPLIANCE.json');
  if (license === null) {
    errors.push('missing supply-chain evidence: artifacts/reports/LICENSE_COMPLIANCE.json');
  } else if (license.__invalid_json__) {
    errors.push(`artifacts/reports/LICENSE_COMPLIANCE.json is not valid JSON: ${license.__invalid_json__}`);
  } else {
    const allowed = new Set(['pass', 'pass_with_restrictions']);
    if (!allowed.has(license.status)) errors.push(`license report status must be one of ${[...allowed].join(', ')}; got ${license.status}.`);
    if (!Array.isArray(license.release_blockers) || license.release_blockers.length !== 0) errors.push(`license report must have an empty release_blockers array; got ${JSON.stringify(license.release_blockers)}.`);
    summary.license_status = license.status;
    summary.release_blockers = license.release_blockers;
  }

  // 4. Vulnerability scan + exceptions
  const vulnExceptions = validateExceptions(root, 'supply-chain/vulnerability-exceptions.json', 'vulnerability', errors);
  summary.vuln_exceptions = vulnExceptions.length;
  const vulnScan = readJson(root, 'artifacts/reports/VULNERABILITY_SCAN.json');
  if (vulnScan === null) {
    errors.push('missing supply-chain evidence: artifacts/reports/VULNERABILITY_SCAN.json');
  } else if (vulnScan.__invalid_json__) {
    errors.push(`artifacts/reports/VULNERABILITY_SCAN.json is not valid JSON: ${vulnScan.__invalid_json__}`);
  } else {
    summary.vuln_scan = vulnScan.status;
    const findings = Array.isArray(vulnScan.findings) ? vulnScan.findings : [];
    summary.vuln_findings = findings.length;
    if (vulnScan.status === 'available') {
      for (const finding of findings) {
        if (!BLOCKING_SEVERITIES.has(finding?.severity)) continue;
        const key = `${finding.id} ${finding.package}@${finding.version}`;
        const matched = vulnExceptions.some((exception) => exceptionMatches(exception, key));
        if (!matched) {
          errors.push(`blocking vulnerability without exception: ${key} (severity=${finding.severity}). Add an exception with owner, reason, and future expires_at.`);
        }
      }
    } else if (vulnScan.status === 'blocked_environment') {
      // Recorded environment limitation; never presented as a passing advisory scan.
    } else if (vulnScan.status === 'error') {
      errors.push(`vulnerability scan failed: ${vulnScan.reason ?? 'unknown error'}`);
    } else {
      errors.push(`vulnerability scan has unknown status: ${vulnScan.status}`);
    }
  }

  // 5. Secret scan + exceptions
  const secretExceptions = validateExceptions(root, 'supply-chain/secret-exceptions.json', 'secret', errors);
  summary.secret_exceptions = secretExceptions.length;
  const secretScan = readJson(root, 'artifacts/reports/SECRET_SCAN.json');
  if (secretScan === null) {
    errors.push('missing supply-chain evidence: artifacts/reports/SECRET_SCAN.json');
  } else if (secretScan.__invalid_json__) {
    errors.push(`artifacts/reports/SECRET_SCAN.json is not valid JSON: ${secretScan.__invalid_json__}`);
  } else {
    summary.secret_scan = secretScan.status;
    const findings = Array.isArray(secretScan.findings) ? secretScan.findings : [];
    summary.secret_findings = findings.length;
    if (secretScan.status === 'findings') {
      for (const finding of findings) {
        const key = `${finding.rule}:${finding.path}`;
        const matched = secretExceptions.some((exception) => exceptionMatches(exception, key));
        if (!matched) {
          errors.push(`secret finding without exception: ${key} (line ${finding.line}). Add an exception with owner, reason, and future expires_at.`);
        }
      }
    } else if (secretScan.status !== 'pass') {
      errors.push(`secret scan has unknown status: ${secretScan.status}`);
    }
  }

  return { errors, summary };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const root = process.argv[2] ?? process.cwd();
  const { errors, summary } = validateSupplyChain(root);
  if (errors.length > 0) {
    console.error(`Supply chain gate failed with ${errors.length} error(s):`);
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
  } else {
    console.log(`Supply chain gate valid: lockfiles=${summary.lockfiles}, sbom_components=${summary.sbom_components}, license=${summary.license_status} (release_blockers=${Array.isArray(summary.release_blockers) ? summary.release_blockers.length : summary.release_blockers}), vuln_scan=${summary.vuln_scan} (findings=${summary.vuln_findings}), secret_scan=${summary.secret_scan} (findings=${summary.secret_findings}), vuln_exceptions=${summary.vuln_exceptions}, secret_exceptions=${summary.secret_exceptions}.`);
  }
}