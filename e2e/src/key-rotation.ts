// Test-key rotation policy (T153).
//
// E2E credentials are short-lived and rotated; the policy derives the active
// key generation from a base loaded from the environment, so no key material
// is ever committed.

/** A rotation policy for E2E secrets. */
export class KeyRotationPolicy {
  /** Rotation interval in days. */
  readonly intervalDays: number;
  /** The current generation. */
  readonly generation: number;

  constructor(intervalDays: number, generation: number) {
    this.intervalDays = intervalDays;
    this.generation = generation;
  }

  /** Rotates: returns the next generation's policy. */
  rotate(): KeyRotationPolicy {
    return new KeyRotationPolicy(this.intervalDays, this.generation + 1);
  }

  /** The active key for a base secret (base-gen<generation>). */
  activeKey(base: string): string {
    return `${base}-gen${this.generation}`;
  }

  /** Whether the key is still valid at `ageDays`. */
  isValid(ageDays: number): boolean {
    return ageDays < this.intervalDays;
  }
}