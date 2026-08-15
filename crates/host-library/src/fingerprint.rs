//! First-connection host fingerprint review (T104).
//!
//! When connecting to a host for the first time (or when the presented key
//! differs from the known key), the UI must show enough to make a safe
//! decision: the key **algorithm**, the **SHA-256 fingerprint**, the
//! **source** (known_hosts / server / user-provided), and a **risk** level.
//! [`FingerprintReview`] classifies the situation (match / new / changed),
//! drives approve / reject decisions, and renders a review view.

use sha2::{Digest, Sha256};

/// The SHA-256 digest length.
pub const SHA256_FINGERPRINT_LEN: usize = 32;

/// The host key algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAlgorithm {
    /// ed25519.
    Ed25519,
    /// ecdsa-sha2-nistp256.
    EcdsaP256,
    /// rsa-sha2-256.
    RsaSha2_256,
    /// ssh-dss (legacy, weak).
    Dss,
}

impl KeyAlgorithm {
    /// The human label shown in the review.
    pub fn label(&self) -> &'static str {
        match self {
            KeyAlgorithm::Ed25519 => "ssh-ed25519",
            KeyAlgorithm::EcdsaP256 => "ecdsa-sha2-nistp256",
            KeyAlgorithm::RsaSha2_256 => "rsa-sha2-256",
            KeyAlgorithm::Dss => "ssh-dss",
        }
    }

    /// The intrinsic risk of the algorithm.
    pub fn risk(&self) -> RiskLevel {
        match self {
            KeyAlgorithm::Dss => RiskLevel::High,
            KeyAlgorithm::RsaSha2_256 => RiskLevel::Medium,
            KeyAlgorithm::Ed25519 | KeyAlgorithm::EcdsaP256 => RiskLevel::Low,
        }
    }
}

/// The fingerprint source shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerprintSource {
    /// From the local known_hosts file.
    KnownHosts,
    /// Presented by the server during the handshake.
    Server,
    /// Provided by the user.
    UserProvided,
}

impl FingerprintSource {
    /// The human label.
    pub fn label(&self) -> &'static str {
        match self {
            FingerprintSource::KnownHosts => "known_hosts",
            FingerprintSource::Server => "server",
            FingerprintSource::UserProvided => "user",
        }
    }
}

/// The displayed risk level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    /// Low risk.
    Low,
    /// Medium risk.
    Medium,
    /// High risk.
    High,
}

impl RiskLevel {
    /// The human label.
    pub fn label(&self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
        }
    }
}

/// A host key fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyFingerprint {
    /// Key algorithm.
    pub algorithm: KeyAlgorithm,
    /// The SHA-256 digest bytes.
    pub digest: [u8; SHA256_FINGERPRINT_LEN],
    /// Where this fingerprint came from.
    pub source: FingerprintSource,
}

impl HostKeyFingerprint {
    /// Computes the SHA-256 fingerprint of raw key bytes.
    pub fn from_key_bytes(
        algorithm: KeyAlgorithm,
        key_bytes: &[u8],
        source: FingerprintSource,
    ) -> Self {
        let digest: [u8; SHA256_FINGERPRINT_LEN] = Sha256::digest(key_bytes).into();
        Self {
            algorithm,
            digest,
            source,
        }
    }

    /// The colon-grouped hex fingerprint (`aa:bb:...`) shown to the user.
    pub fn formatted(&self) -> String {
        let hex: String = self
            .digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        hex.as_bytes()
            .chunks(2)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<Vec<_>>()
            .join(":")
    }

    /// The full hex digest (no separators).
    pub fn hex(&self) -> String {
        self.digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Whether two fingerprints are identical.
    pub fn matches(&self, other: &HostKeyFingerprint) -> bool {
        self.digest == other.digest
    }
}

/// The review state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// Waiting for the user's decision.
    Pending,
    /// The user approved the host.
    Approved,
    /// The user rejected the host.
    Rejected,
}

/// The user's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    /// Trust and continue.
    Approve,
    /// Refuse the connection.
    Reject,
}

/// A change notice shown when the presented key differs from the known key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeNotice {
    /// Human-readable warning.
    pub message: String,
}

/// The review view (what the UI displays).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewView {
    /// Algorithm label.
    pub algorithm: &'static str,
    /// Colon-grouped SHA-256 fingerprint.
    pub sha256: String,
    /// Source label.
    pub source: &'static str,
    /// Risk level.
    pub risk: RiskLevel,
    /// Change notice when the presented key differs from the known key.
    pub change: Option<ChangeNotice>,
}

/// A first-connection fingerprint review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FingerprintReview {
    /// The known fingerprint, if any.
    pub known: Option<HostKeyFingerprint>,
    /// The fingerprint the server presented.
    pub presented: HostKeyFingerprint,
    /// The review state.
    pub state: ReviewState,
}

impl FingerprintReview {
    /// Classifies a presented fingerprint against a known one (if any).
    pub fn classify(known: Option<HostKeyFingerprint>, presented: HostKeyFingerprint) -> Self {
        Self {
            known,
            presented,
            state: ReviewState::Pending,
        }
    }

    /// Whether the presented key differs from the known key.
    pub fn change_detected(&self) -> bool {
        match &self.known {
            Some(known) => !known.matches(&self.presented),
            None => false,
        }
    }

    /// The risk level shown to the user.
    pub fn risk(&self) -> RiskLevel {
        let algorithm_risk = self.presented.algorithm.risk();
        if self.change_detected() {
            return RiskLevel::High;
        }
        match &self.known {
            // New host: medium, unless the algorithm itself is weak.
            None => algorithm_risk.max(RiskLevel::Medium),
            Some(_) => algorithm_risk,
        }
    }

    /// Applies the user's decision.
    pub fn decide(&mut self, decision: ReviewDecision) {
        self.state = match decision {
            ReviewDecision::Approve => ReviewState::Approved,
            ReviewDecision::Reject => ReviewState::Rejected,
        };
    }

    /// Renders the review view.
    pub fn view(&self) -> ReviewView {
        ReviewView {
            algorithm: self.presented.algorithm.label(),
            sha256: self.presented.formatted(),
            source: self.presented.source.label(),
            risk: self.risk(),
            change: if self.change_detected() {
                Some(ChangeNotice {
                    message: "The host key has changed since the last connection; this may indicate a man-in-the-middle attack.".to_owned(),
                })
            } else {
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FingerprintReview, FingerprintSource, HostKeyFingerprint, KeyAlgorithm, ReviewDecision,
        ReviewState, RiskLevel,
    };

    fn fingerprint(algorithm: KeyAlgorithm, seed: u8) -> HostKeyFingerprint {
        HostKeyFingerprint::from_key_bytes(algorithm, &[seed; 32], FingerprintSource::Server)
    }

    fn known(algorithm: KeyAlgorithm, seed: u8) -> HostKeyFingerprint {
        HostKeyFingerprint::from_key_bytes(algorithm, &[seed; 32], FingerprintSource::KnownHosts)
    }

    #[test]
    fn fingerprint_is_deterministic_sha256() {
        let a = fingerprint(KeyAlgorithm::Ed25519, 1);
        let b = fingerprint(KeyAlgorithm::Ed25519, 1);
        let c = fingerprint(KeyAlgorithm::Ed25519, 2);
        assert_eq!(a, b, "same key bytes -> same fingerprint");
        assert_ne!(a, c, "different key bytes -> different fingerprint");
        assert_eq!(a.digest.len(), 32);
    }

    #[test]
    fn formatted_fingerprint_is_colon_grouped() {
        let fp = fingerprint(KeyAlgorithm::Ed25519, 7);
        let formatted = fp.formatted();
        // 32 bytes -> 64 hex chars grouped in pairs: "aa:bb:...".
        assert_eq!(formatted.len(), 64 + 31);
        assert!(formatted.contains(':'));
        assert_eq!(formatted.split(':').count(), 32);
        assert_eq!(fp.hex().len(), 64);
    }

    #[test]
    fn new_host_is_medium_risk_and_approvable() {
        let mut review = FingerprintReview::classify(None, fingerprint(KeyAlgorithm::Ed25519, 1));
        assert!(!review.change_detected());
        assert_eq!(review.risk(), RiskLevel::Medium);
        assert_eq!(review.state, ReviewState::Pending);
        review.decide(ReviewDecision::Approve);
        assert_eq!(review.state, ReviewState::Approved);
    }

    #[test]
    fn matching_known_fingerprint_is_low_risk() {
        let review = FingerprintReview::classify(
            Some(known(KeyAlgorithm::Ed25519, 1)),
            fingerprint(KeyAlgorithm::Ed25519, 1),
        );
        assert!(!review.change_detected());
        assert_eq!(review.risk(), RiskLevel::Low);
    }

    #[test]
    fn changed_fingerprint_is_high_risk_and_rejectable() {
        let mut review = FingerprintReview::classify(
            Some(known(KeyAlgorithm::Ed25519, 1)),
            fingerprint(KeyAlgorithm::Ed25519, 2),
        );
        assert!(review.change_detected());
        assert_eq!(review.risk(), RiskLevel::High);
        assert!(review.view().change.is_some());
        review.decide(ReviewDecision::Reject);
        assert_eq!(review.state, ReviewState::Rejected);
    }

    #[test]
    fn weak_algorithm_raises_risk_even_when_matching() {
        let review = FingerprintReview::classify(
            Some(known(KeyAlgorithm::Dss, 1)),
            fingerprint(KeyAlgorithm::Dss, 1),
        );
        assert!(!review.change_detected());
        assert_eq!(
            review.risk(),
            RiskLevel::High,
            "ssh-dss is intrinsically weak"
        );
    }

    #[test]
    fn view_shows_algorithm_fingerprint_source_and_risk() {
        let mut review = FingerprintReview::classify(
            Some(known(KeyAlgorithm::RsaSha2_256, 1)),
            fingerprint(KeyAlgorithm::RsaSha2_256, 2),
        );
        let view = review.view();
        assert_eq!(view.algorithm, "rsa-sha2-256");
        assert!(view.sha256.contains(':'));
        assert_eq!(view.source, "server");
        assert_eq!(view.risk, RiskLevel::High);
        assert!(view.change.is_some());
        // Approving moves the state forward and the view stays renderable.
        review.decide(ReviewDecision::Approve);
        assert_eq!(review.state, ReviewState::Approved);
        assert_eq!(review.view().algorithm, "rsa-sha2-256");
    }
}
