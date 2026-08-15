// Test-account secrets provider (T153).
//
// Secrets NEVER live in the repository: they are loaded from environment
// variables. The provider rejects missing or placeholder values so a
// placeholder can never be used as a real credential.

/** Reads E2E secrets from the environment (never from the repo). */
export class SecretsProvider {
  private readonly env: Record<string, string | undefined>;

  constructor(env: Record<string, string | undefined> = process.env) {
    this.env = env;
  }

  /** The gateway session token (env E2E_GATEWAY_TOKEN). */
  gatewayToken(): string {
    const value = this.env.E2E_GATEWAY_TOKEN;
    if (!value || value.includes('<') || value.includes('replace-me')) {
      throw new Error('E2E_GATEWAY_TOKEN must be set in the environment; secrets never live in the repository');
    }
    return value;
  }

  /** The SSH test key material (env E2E_TEST_KEY), also env-only. */
  testKey(): string {
    const value = this.env.E2E_TEST_KEY;
    if (!value || value.includes('<')) {
      throw new Error('E2E_TEST_KEY must be set in the environment; secrets never live in the repository');
    }
    return value;
  }
}