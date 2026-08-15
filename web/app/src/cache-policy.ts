// Cache policy and audit for the PWA (T140): the app-shell is cacheable;
// session traffic, credentials, and terminal data are never cached. The
// audit guarantees offline caches contain no session secrets, and
// clear-session-data purges every session-scoped entry while keeping the
// app shell.

/** Cache kinds. */
export type CacheKind = 'app-shell' | 'session' | 'forbidden';

/** A cache entry seen by the audit. */
export interface CacheEntry {
  url: string;
  kind: CacheKind;
  sensitive: boolean;
  bytes: number;
}

/** App-shell asset patterns (cacheable). */
const APP_SHELL_PATTERNS = [
  /\.(js|mjs|css|wasm|woff2?|ttf|svg|png|ico|webmanifest)$/i,
  /^\/static\//,
];

/** Sensitive patterns: never cacheable, never persisted. */
const SENSITIVE_PATTERNS = [
  /token/i,
  /auth/i,
  /key/i,
  /secret/i,
  /session/i,
  /credential/i,
  /terminal/i,
  /host/i,
  /history/i,
  /transfer/i,
];

/** Whether a URL is an app-shell asset. */
export function isAppShellUrl(url: string): boolean {
  return APP_SHELL_PATTERNS.some((pattern) => pattern.test(url));
}

/** Whether a URL is sensitive (session data, credentials, terminal content). */
export function isSensitiveUrl(url: string): boolean {
  return SENSITIVE_PATTERNS.some((pattern) => pattern.test(url));
}

/** The cache policy. */
export class CachePolicy {
  /** Classifies a URL into a cache kind. */
  classify(url: string): CacheKind {
    if (isSensitiveUrl(url)) return 'forbidden';
    if (isAppShellUrl(url)) return 'app-shell';
    return 'session';
  }

  /** Whether a URL may be written to the offline cache. */
  shouldCache(url: string): boolean {
    return this.classify(url) === 'app-shell';
  }

  /** Audits cache entries; returns every violation (sensitive data cached). */
  audit(entries: CacheEntry[]): string[] {
    const violations: string[] = [];
    for (const entry of entries) {
      if (entry.kind === 'forbidden' || entry.sensitive) {
        violations.push(`sensitive data cached: ${entry.url}`);
      }
      if (entry.kind === 'session') {
        violations.push(`session data persisted: ${entry.url}`);
      }
    }
    return violations;
  }

  /**
   * Clears session data: removes every session-scoped / forbidden entry and
   * returns the entries that remain (the app shell). Never removes
   * app-shell assets.
   */
  clearSessionData(entries: CacheEntry[]): CacheEntry[] {
    return entries.filter(
      (entry) => entry.kind === 'app-shell' && !entry.sensitive,
    );
  }
}