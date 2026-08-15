//! Gateway authentication, short-lived tokens, and session isolation (T137).
//!
//! The gateway never stores long-lived SSH credentials (keys stay
//! client-side, end-to-end). Clients authenticate with short-lived,
//! single-use, tenant-bound tokens; sessions are registered per tenant and
//! every access re-checks the tenant boundary so one tenant can never reach
//! another tenant's session.

use std::collections::HashMap;

/// A tenant identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantId(pub u64);

/// A short-lived, single-use authentication token bound to a tenant and a
/// session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthToken {
    /// Unique token identifier (also the replay nonce).
    pub token_id: u64,
    /// The tenant the token was issued to.
    pub tenant: TenantId,
    /// The session the token opens.
    pub session_id: u64,
    /// Issue time (unix seconds).
    pub issued_at: u64,
    /// Expiry time (unix seconds).
    pub expires_at: u64,
    /// Token secret material, held only in memory.
    pub secret: u64,
}

impl AuthToken {
    /// Whether the token has expired at `now`.
    pub fn is_expired(&self, now: u64) -> bool {
        now > self.expires_at
    }

    /// The token lifetime in seconds.
    pub fn ttl_secs(&self) -> u64 {
        self.expires_at.saturating_sub(self.issued_at)
    }
}

/// Issues short-lived tokens. Token secrets are derived in memory only and
/// never persisted.
#[derive(Debug, Clone)]
pub struct TokenIssuer {
    next_token: u64,
    next_session: u64,
    /// Default token TTL in seconds (short-lived).
    pub default_ttl_secs: u64,
}

impl Default for TokenIssuer {
    fn default() -> Self {
        Self {
            next_token: 1,
            next_session: 1,
            default_ttl_secs: 300,
        }
    }
}

impl TokenIssuer {
    /// A fresh issuer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Issues a token with the default TTL and creates a fresh session.
    pub fn issue(&mut self, tenant: TenantId, now: u64) -> AuthToken {
        self.issue_with_ttl(tenant, now, self.default_ttl_secs)
    }

    /// Issues a token with an explicit TTL and creates a fresh session.
    pub fn issue_with_ttl(&mut self, tenant: TenantId, now: u64, ttl_secs: u64) -> AuthToken {
        let token = AuthToken {
            token_id: self.next_token,
            tenant,
            session_id: self.next_session,
            issued_at: now,
            expires_at: now + ttl_secs,
            secret: self
                .next_token
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(now),
        };
        self.next_token += 1;
        self.next_session += 1;
        token
    }
}

/// A registered gateway session, owned by exactly one tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Session {
    tenant: TenantId,
    created_at: u64,
}

/// Session registry: owns every session and enforces tenant isolation and
/// single-use tokens. Consumed tokens are tracked with their expiry so a
/// long-running gateway can prune them (bounded memory over a 72-hour soak).
#[derive(Debug, Clone, Default)]
pub struct SessionRegistry {
    sessions: HashMap<u64, Session>,
    consumed_tokens: HashMap<u64, u64>,
}

impl SessionRegistry {
    /// A fresh registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new session for `tenant` and returns the token that opens
    /// it.
    pub fn create_session(
        &mut self,
        issuer: &mut TokenIssuer,
        tenant: TenantId,
        now: u64,
    ) -> AuthToken {
        let token = issuer.issue(tenant, now);
        self.sessions.insert(
            token.session_id,
            Session {
                tenant,
                created_at: now,
            },
        );
        token
    }

    /// Authenticates a presented token. Rejects expired tokens, replayed
    /// (already consumed) tokens, unknown sessions, and tokens whose tenant
    /// does not own the session. Returns the owning tenant and session id.
    pub fn authenticate(
        &mut self,
        token: &AuthToken,
        now: u64,
    ) -> Result<(TenantId, u64), AuthError> {
        if token.is_expired(now) {
            return Err(AuthError::TokenExpired);
        }
        if self.consumed_tokens.contains_key(&token.token_id) {
            return Err(AuthError::ReplayDetected);
        }
        let session = self
            .sessions
            .get(&token.session_id)
            .ok_or(AuthError::UnknownSession)?;
        if session.tenant != token.tenant {
            return Err(AuthError::TenantIsolationViolation);
        }
        self.consumed_tokens
            .insert(token.token_id, token.expires_at);
        Ok((session.tenant, token.session_id))
    }

    /// Prunes consumed tokens that have expired, keeping the replay window
    /// bounded over long soak runs. Returns the number of entries removed.
    pub fn prune_expired(&mut self, now: u64) -> usize {
        let before = self.consumed_tokens.len();
        self.consumed_tokens
            .retain(|_, expires_at| *expires_at >= now);
        before - self.consumed_tokens.len()
    }

    /// The number of consumed (replay-tracked) tokens; the bounded-memory
    /// soak metric.
    pub fn consumed_token_count(&self) -> usize {
        self.consumed_tokens.len()
    }

    /// Removes a session when it closes, so repeated connect/disconnect
    /// cycles return the registry to baseline (no leaked sessions). Returns
    /// whether a session was removed.
    pub fn close_session(&mut self, session_id: u64) -> bool {
        self.sessions.remove(&session_id).is_some()
    }

    /// The number of live (open) sessions.
    pub fn open_session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Enforces the tenant boundary for a session-bound operation: `tenant`
    /// must be the owner of `session_id`.
    pub fn access(&self, tenant: TenantId, session_id: u64) -> Result<(), AuthError> {
        match self.sessions.get(&session_id) {
            Some(session) if session.tenant == tenant => Ok(()),
            Some(_) => Err(AuthError::TenantIsolationViolation),
            None => Err(AuthError::UnknownSession),
        }
    }
}

/// Credential handling: SSH keys never touch the gateway disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialPolicy {
    /// Maximum lifetime for any gateway-held key material.
    pub max_key_lifetime_secs: u64,
}

impl Default for CredentialPolicy {
    fn default() -> Self {
        Self {
            max_key_lifetime_secs: 300,
        }
    }
}

impl CredentialPolicy {
    /// The gateway rejects persisting long-lived SSH keys; keys remain
    /// client-side.
    pub fn persist_long_term_key(&self) -> Result<(), AuthError> {
        Err(AuthError::LongTermKeyStorageForbidden)
    }

    /// A short-lived, in-memory session key within the lifetime budget is
    /// acceptable; anything longer is rejected.
    pub fn accept_short_lived_session_key(&self, lifetime_secs: u64) -> Result<(), AuthError> {
        if lifetime_secs <= self.max_key_lifetime_secs {
            Ok(())
        } else {
            Err(AuthError::LongTermKeyStorageForbidden)
        }
    }
}

/// Why an authentication or isolation operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// The presented token has expired.
    TokenExpired,
    /// The presented token was already consumed (replay).
    ReplayDetected,
    /// The token references an unknown session.
    UnknownSession,
    /// The caller's tenant does not own the session (cross-tenant access).
    TenantIsolationViolation,
    /// The gateway refuses to persist long-lived key material.
    LongTermKeyStorageForbidden,
}

#[cfg(test)]
mod tests {
    use super::{AuthError, CredentialPolicy, SessionRegistry, TenantId, TokenIssuer};

    #[test]
    fn short_lived_token_expires_after_ttl() {
        let mut issuer = TokenIssuer::new();
        let mut registry = SessionRegistry::new();
        let token = registry.create_session(&mut issuer, TenantId(1), 1000);
        assert_eq!(token.ttl_secs(), 300);
        assert!(registry.authenticate(&token, 1299).is_ok());
        // The token is consumed by the first authenticate; re-presentation at
        // any time is a replay. Use a fresh token for the expiry check.
        let token2 = registry.create_session(&mut issuer, TenantId(1), 1000);
        assert_eq!(
            registry.authenticate(&token2, 1301),
            Err(AuthError::TokenExpired)
        );
    }

    #[test]
    fn replay_of_consumed_token_rejected() {
        let mut issuer = TokenIssuer::new();
        let mut registry = SessionRegistry::new();
        let token = registry.create_session(&mut issuer, TenantId(1), 1000);
        assert!(registry.authenticate(&token, 1100).is_ok());
        assert_eq!(
            registry.authenticate(&token, 1100),
            Err(AuthError::ReplayDetected)
        );
        // Expired consumed tokens are pruned so the replay window stays
        // bounded over a long soak.
        assert_eq!(registry.consumed_token_count(), 1);
        assert_eq!(registry.prune_expired(1301), 1);
        assert_eq!(registry.consumed_token_count(), 0);
    }

    #[test]
    fn cross_tenant_access_denied() {
        let mut issuer = TokenIssuer::new();
        let mut registry = SessionRegistry::new();
        let token_a = registry.create_session(&mut issuer, TenantId(1), 1000);
        let (tenant, session) = registry.authenticate(&token_a, 1100).unwrap();
        assert_eq!(tenant, TenantId(1));
        // Tenant B must not access tenant A's session.
        assert_eq!(
            registry.access(TenantId(2), session),
            Err(AuthError::TenantIsolationViolation)
        );
        assert!(registry.access(TenantId(1), session).is_ok());
    }

    #[test]
    fn token_tenant_mismatch_rejected() {
        let mut issuer = TokenIssuer::new();
        let mut registry = SessionRegistry::new();
        let token_a = registry.create_session(&mut issuer, TenantId(1), 1000);
        // A forged token for tenant B claiming tenant A's session id.
        let forged = super::AuthToken {
            tenant: TenantId(2),
            ..token_a
        };
        assert_eq!(
            registry.authenticate(&forged, 1100),
            Err(AuthError::TenantIsolationViolation)
        );
    }

    #[test]
    fn tenant_sessions_are_isolated() {
        let mut issuer = TokenIssuer::new();
        let mut registry = SessionRegistry::new();
        let token_a = registry.create_session(&mut issuer, TenantId(1), 1000);
        let token_b = registry.create_session(&mut issuer, TenantId(2), 1000);
        let (_, session_a) = registry.authenticate(&token_a, 1100).unwrap();
        let (_, session_b) = registry.authenticate(&token_b, 1100).unwrap();
        assert!(registry.access(TenantId(1), session_a).is_ok());
        assert!(registry.access(TenantId(2), session_b).is_ok());
        assert_eq!(
            registry.access(TenantId(1), session_b),
            Err(AuthError::TenantIsolationViolation)
        );
        assert_eq!(
            registry.access(TenantId(2), session_a),
            Err(AuthError::TenantIsolationViolation)
        );
    }

    #[test]
    fn unknown_session_rejected() {
        let mut issuer = TokenIssuer::new();
        let mut registry = SessionRegistry::new();
        let token = registry.create_session(&mut issuer, TenantId(1), 1000);
        let unknown = super::AuthToken {
            session_id: 9999,
            token_id: 9999,
            ..token
        };
        assert_eq!(
            registry.authenticate(&unknown, 1100),
            Err(AuthError::UnknownSession)
        );
    }

    #[test]
    fn close_session_returns_registry_to_baseline() {
        let mut issuer = TokenIssuer::new();
        let mut registry = SessionRegistry::new();
        assert_eq!(registry.open_session_count(), 0);
        let token_a = registry.create_session(&mut issuer, TenantId(1), 1000);
        let token_b = registry.create_session(&mut issuer, TenantId(2), 1000);
        assert_eq!(registry.open_session_count(), 2);
        assert!(registry.close_session(token_a.session_id));
        assert!(!registry.close_session(token_a.session_id)); // idempotent
        assert_eq!(registry.open_session_count(), 1);
        assert!(registry.close_session(token_b.session_id));
        assert_eq!(registry.open_session_count(), 0);
    }

    #[test]
    fn long_term_key_persistence_rejected() {
        let policy = CredentialPolicy::default();
        assert_eq!(
            policy.persist_long_term_key(),
            Err(AuthError::LongTermKeyStorageForbidden)
        );
    }

    #[test]
    fn short_lived_session_key_within_budget() {
        let policy = CredentialPolicy::default();
        assert!(policy.accept_short_lived_session_key(300).is_ok());
        assert_eq!(
            policy.accept_short_lived_session_key(301),
            Err(AuthError::LongTermKeyStorageForbidden)
        );
    }

    #[test]
    fn custom_ttl_honored() {
        let mut issuer = TokenIssuer::new();
        let token = issuer.issue_with_ttl(TenantId(9), 5000, 60);
        assert_eq!(token.ttl_secs(), 60);
        assert!(!token.is_expired(5059));
        assert!(token.is_expired(5061));
    }
}
