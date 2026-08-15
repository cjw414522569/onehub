//! Automatic update and rollback (T166): signed update metadata,
//! anti-downgrade, staged rollout, and failure rollback for
//! Windows / macOS / Linux.
//!
//! [`UpdateManifest`] carries a version, channel, staged-rollout percentage,
//! and a minimum supported version. [`UpdateCoordinator::apply`] verifies
//! the manifest signature, rejects downgrades, honors the staged rollout,
//! and — when an update fails or is interrupted mid-apply — rolls back to
//! the last-known-good version.

/// A simple semantic version (major.minor.patch) with total ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Major.
    pub major: u16,
    /// Minor.
    pub minor: u16,
    /// Patch.
    pub patch: u16,
}

impl Version {
    /// Parses a `major.minor.patch` version string.
    pub fn parse(text: &str) -> Version {
        let parts: Vec<&str> = text.split('.').collect();
        Version {
            major: parts.first().and_then(|p| p.parse().ok()).unwrap_or(0),
            minor: parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0),
            patch: parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0),
        }
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Why an update was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateError {
    /// The manifest signature is invalid (tampered metadata).
    InvalidSignature,
    /// The target version is older than the current one (anti-downgrade).
    DowngradeRejected,
    /// The target version is below the minimum supported version.
    BelowMinimum,
    /// The client is not part of the current staged rollout.
    NotInRollout,
    /// The download/verify/install step failed (rollback triggered).
    ApplyFailed,
}

/// The signed update manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateManifest {
    /// Target version.
    pub version: Version,
    /// Release channel (stable / beta).
    pub channel: String,
    /// Rollout percentage (0-100); 100 = everyone.
    pub rollout_pct: u8,
    /// Minimum supported version (anti-downgrade floor).
    pub min_version: Version,
    /// Artifact SHA-256 (hex).
    pub sha256: String,
    /// Opaque signature over the metadata.
    pub signature: String,
}

/// A signature verifier (the real implementation binds the platform
/// signature service; the model is deterministic).
pub trait SignatureVerifier {
    /// Whether `signature` is valid for `manifest`.
    fn verify(&self, manifest: &UpdateManifest) -> bool;
}

/// A deterministic verifier for tests: the signature must equal
/// `sha256:<sha256>`.
pub struct DigestVerifier;

impl SignatureVerifier for DigestVerifier {
    fn verify(&self, manifest: &UpdateManifest) -> bool {
        manifest.signature == format!("sha256:{}", manifest.sha256)
    }
}

/// Staged rollout: the client bucket decides whether the update is offered.
pub struct StagedRollout;

impl StagedRollout {
    /// Whether `client_id` falls inside the first `rollout_pct` percent of
    /// the deterministic client space.
    pub fn is_offered(client_id: u64, rollout_pct: u8) -> bool {
        let bucket = (client_id.wrapping_mul(0x9E37_79B9_7F4A_7C15) % 100) as u8;
        bucket < rollout_pct
    }
}

/// The coordinator: apply with verification, staged rollout, anti-downgrade,
/// and rollback on failure.
#[derive(Debug, Clone)]
pub struct UpdateCoordinator {
    /// The current installed version.
    pub current: Version,
    /// The last-known-good version kept for rollback.
    last_known_good: Version,
}

impl UpdateCoordinator {
    /// A coordinator at `current`.
    pub fn new(current: Version) -> Self {
        Self {
            current,
            last_known_good: current,
        }
    }

    /// Applies an update. On any failure the current version is rolled back
    /// to the last-known-good.
    pub fn apply(
        &mut self,
        manifest: &UpdateManifest,
        verifier: &dyn SignatureVerifier,
        client_id: u64,
        install_ok: bool,
    ) -> Result<Version, UpdateError> {
        // 1. Signature (tamper).
        if !verifier.verify(manifest) {
            return Err(UpdateError::InvalidSignature);
        }
        // 2. Anti-downgrade.
        if manifest.version < self.current {
            return Err(UpdateError::DowngradeRejected);
        }
        if manifest.version < manifest.min_version {
            return Err(UpdateError::BelowMinimum);
        }
        // 3. Staged rollout.
        if !StagedRollout::is_offered(client_id, manifest.rollout_pct) {
            return Err(UpdateError::NotInRollout);
        }
        // 4. Install (may fail / be interrupted).
        if !install_ok {
            self.rollback();
            return Err(UpdateError::ApplyFailed);
        }
        self.last_known_good = self.current;
        self.current = manifest.version;
        Ok(self.current)
    }

    /// Rolls back to the last-known-good version.
    pub fn rollback(&mut self) {
        self.current = self.last_known_good;
    }

    /// The last-known-good version (what a rollback restores).
    pub fn last_known_good(&self) -> Version {
        self.last_known_good
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DigestVerifier, StagedRollout, UpdateCoordinator, UpdateError, UpdateManifest, Version,
    };

    fn manifest(version: &str, min: &str, rollout: u8, sha256: &str) -> UpdateManifest {
        UpdateManifest {
            version: Version::parse(version),
            channel: "stable".to_owned(),
            rollout_pct: rollout,
            min_version: Version::parse(min),
            sha256: sha256.to_owned(),
            signature: format!("sha256:{sha256}"),
        }
    }

    #[test]
    fn signed_upgrade_applies() {
        let verifier = DigestVerifier;
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.1.0"));
        let update = manifest("0.2.0", "0.1.0", 100, "abc123");
        assert_eq!(
            coordinator.apply(&update, &verifier, 42, true),
            Ok(Version::parse("0.2.0"))
        );
        assert_eq!(coordinator.current, Version::parse("0.2.0"));
    }

    #[test]
    fn tampered_metadata_is_rejected() {
        let verifier = DigestVerifier;
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.1.0"));
        let mut tampered = manifest("0.2.0", "0.1.0", 100, "abc123");
        tampered.sha256 = "tampered".to_owned(); // digest changed, signature stale
        assert_eq!(
            coordinator.apply(&tampered, &verifier, 42, true),
            Err(UpdateError::InvalidSignature)
        );
        assert_eq!(coordinator.current, Version::parse("0.1.0"));
    }

    #[test]
    fn downgrade_is_rejected() {
        let verifier = DigestVerifier;
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.2.0"));
        let older = manifest("0.1.0", "0.1.0", 100, "def456");
        assert_eq!(
            coordinator.apply(&older, &verifier, 42, true),
            Err(UpdateError::DowngradeRejected)
        );
        // Below the minimum floor is also rejected (target 0.1.5 is not a
        // downgrade from 0.1.0 but is under the 0.2.0 floor).
        let below = manifest("0.1.5", "0.2.0", 100, "def456");
        let mut coordinator2 = UpdateCoordinator::new(Version::parse("0.1.0"));
        assert_eq!(
            coordinator2.apply(&below, &verifier, 42, true),
            Err(UpdateError::BelowMinimum)
        );
    }

    #[test]
    fn staged_rollout_gates_clients() {
        // With 0% rollout nobody is offered the update; with 100% everyone.
        assert!(!StagedRollout::is_offered(7, 0));
        assert!(StagedRollout::is_offered(7, 100));
        // A 50% rollout offers roughly half of the deterministic bucket space.
        let offered = (0u64..1000)
            .filter(|id| StagedRollout::is_offered(*id, 50))
            .count();
        assert!(
            (400..600).contains(&offered),
            "50% rollout offered {offered}/1000"
        );
    }

    #[test]
    fn failed_install_rolls_back() {
        let verifier = DigestVerifier;
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.1.0"));
        let update = manifest("0.2.0", "0.1.0", 100, "abc123");
        // Interrupted install: apply fails and the version rolls back.
        assert_eq!(
            coordinator.apply(&update, &verifier, 42, false),
            Err(UpdateError::ApplyFailed)
        );
        assert_eq!(coordinator.current, Version::parse("0.1.0"));
        assert_eq!(coordinator.last_known_good(), Version::parse("0.1.0"));
        // A subsequent valid update still works after rollback.
        assert_eq!(
            coordinator.apply(&update, &verifier, 42, true),
            Ok(Version::parse("0.2.0"))
        );
    }
}
