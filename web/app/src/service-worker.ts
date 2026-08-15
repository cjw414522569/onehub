// PWA service-worker model (T140): versioned app-shell caching, update /
// activation flow, stale-cache purge, and session-scoped cache separation.
// The session cache is memory-only by policy (never written to disk), so
// offline mode can never leak session secrets.

/** A semantic version. */
export interface SwVersion {
  major: number;
  minor: number;
  patch: number;
}

/** Parses a "x.y.z" version string. */
export function swVersion(text: string): SwVersion {
  const [major = 0, minor = 0, patch = 0] = text.split('.').map((part) => Number(part));
  return { major, minor, patch };
}

/** Compares two versions: negative when a < b, zero when equal, positive otherwise. */
export function compareVersions(a: SwVersion, b: SwVersion): number {
  return a.major - b.major || a.minor - b.minor || a.patch - b.patch;
}

/** The app-shell cache prefix. */
export const APP_SHELL_CACHE_PREFIX = 'ssh-shell-';
/** The session cache name (memory-only; never persisted by policy). */
export const SESSION_CACHE_NAME = 'ssh-session-v0';

/** A versioned app-shell cache name, e.g. `ssh-shell-1.0.0`. */
export function appShellCacheName(version: SwVersion): string {
  return `${APP_SHELL_CACHE_PREFIX}${version.major}.${version.minor}.${version.patch}`;
}

/** Result of detecting a newer service worker. */
export interface UpdateResult {
  /** The currently active version. */
  active: SwVersion;
  /** The detected version. */
  next: SwVersion;
  /** Whether an update is waiting to activate. */
  waiting: boolean;
}

/** The service-worker lifecycle model. */
export class ServiceWorkerModel {
  /** The active version. */
  active: SwVersion;
  /** The pending (waiting) version, if an update was found. */
  pending: SwVersion | null = null;
  /** The cache names currently registered in the browser. */
  knownCaches: string[] = [];

  constructor(version: string) {
    this.active = swVersion(version);
  }

  /** Registers the cache names currently present in the browser. */
  registerCaches(names: string[]): void {
    this.knownCaches = [...names];
  }

  /** The app-shell cache name for the active version. */
  activeCacheName(): string {
    return appShellCacheName(this.active);
  }

  /** The session cache name (memory-only by policy). */
  sessionCacheName(): string {
    return SESSION_CACHE_NAME;
  }

  /** Detects a newer service worker and enters the waiting state. */
  onUpdateFound(version: string): UpdateResult {
    const next = swVersion(version);
    const waiting = compareVersions(next, this.active) > 0;
    if (waiting) this.pending = next;
    return { active: this.active, next, waiting };
  }

  /**
   * Activates the pending update (skipWaiting + clients.claim semantics):
   * the pending version becomes active and the function returns every stale
   * app-shell cache name that must be purged.
   */
  activate(): string[] {
    if (!this.pending) return [];
    const stale = this.staleCacheNames(this.pending, this.knownCaches);
    this.active = this.pending;
    this.pending = null;
    return stale;
  }

  /**
   * App-shell caches whose version differs from `version` (candidates for
   * deletion after activation).
   */
  staleCacheNames(version: SwVersion, existing: string[] = []): string[] {
    const current = appShellCacheName(version);
    return existing.filter((name) => name.startsWith(APP_SHELL_CACHE_PREFIX) && name !== current);
  }
}