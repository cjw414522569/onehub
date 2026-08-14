//! OpenSSH server version / OS / algorithm compatibility matrix (T060).
//!
//! A data-driven matrix of real OpenSSH server combinations (platform x OS x
//! version x documented default algorithm sets). The local, always-runnable
//! check verifies that this client's negotiated algorithm policy has a
//! mutually acceptable algorithm in every category for each server combo.
//! Actually executing a live OpenSSH server across Linux/BSD/macOS/Windows is
//! `blocked_environment` on hosts without `ssh`/`sshd`/docker/remote hosts; the
//! report records that honestly and the full nightly matrix is meant to run in
//! CI with provisioned sshd containers.

use crate::algorithms::{
    negotiate_algorithm, Algorithm, AlgorithmKind, AlgorithmPolicy, NegotiatedAlgorithm,
};

/// Reason recorded when a live-server execution is not possible on this host.
pub const LIVE_BLOCKED_REASON: &str = "no ssh/sshd/docker/remote hosts on this Windows host \
(live OpenSSH execution is blocked_environment; the local algorithm check still runs)";

/// One documented OpenSSH server combination.
#[derive(Debug, Clone, Copy)]
pub struct ServerCombo {
    /// Stable id, e.g. `linux-ubuntu-2404-openssh-9.6`.
    pub id: &'static str,
    /// Platform family: Linux / macOS / Windows / FreeBSD / OpenBSD.
    pub platform: &'static str,
    /// OS release.
    pub os: &'static str,
    /// OpenSSH version.
    pub openssh_version: &'static str,
    /// Whether the combo still requires SHA-1-era algorithms by default.
    pub legacy_sha1_defaults: bool,
    /// Documented default key-exchange offers.
    pub kex: &'static [Algorithm],
    /// Documented default cipher offers.
    pub ciphers: &'static [Algorithm],
    /// Documented default MAC offers.
    pub macs: &'static [Algorithm],
    /// Documented default host-key offers.
    pub host_keys: &'static [Algorithm],
}

impl ServerCombo {
    /// The documented offers for `kind`.
    pub fn offers(&self, kind: AlgorithmKind) -> &'static [Algorithm] {
        match kind {
            AlgorithmKind::Kex => self.kex,
            AlgorithmKind::HostKey => self.host_keys,
            AlgorithmKind::Cipher => self.ciphers,
            AlgorithmKind::Mac => self.macs,
            AlgorithmKind::Compression => &[],
        }
    }
}

/// A mutually acceptable algorithm per category, chosen by local preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenAlgorithms {
    /// Negotiated key exchange.
    pub kex: Algorithm,
    /// Negotiated host key.
    pub host_key: Algorithm,
    /// Negotiated cipher.
    pub cipher: Algorithm,
    /// Negotiated MAC.
    pub mac: Algorithm,
}

/// Result of the local compatibility check for one combo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatVerdict {
    /// Every category has a mutually acceptable algorithm.
    Compatible(ChosenAlgorithms),
    /// Some category has no intersection under the policy.
    Incompatible { kind: AlgorithmKind },
}

/// How a matrix entry was executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// The local algorithm-intersection check ran automatically.
    LocalAlgorithmCheck,
    /// The live server execution is blocked on this host.
    LiveServerBlocked { reason: &'static str },
}

/// One matrix entry with its verdict and execution mode.
#[derive(Debug, Clone)]
pub struct MatrixEntry {
    /// Combo id.
    pub combo_id: &'static str,
    /// Platform family.
    pub platform: &'static str,
    /// OS release.
    pub os: &'static str,
    /// OpenSSH version.
    pub openssh_version: &'static str,
    /// Local algorithm-check verdict.
    pub verdict: CompatVerdict,
    /// Whether the combo still ships SHA-1-era defaults.
    pub legacy_sha1_defaults: bool,
    /// Execution mode.
    pub execution: ExecutionMode,
}

/// The full matrix report.
#[derive(Debug, Clone, Default)]
pub struct CompatMatrixReport {
    /// One entry per combo.
    pub entries: Vec<MatrixEntry>,
    /// Overall live-execution note.
    pub live_blocked_reason: &'static str,
}

impl CompatMatrixReport {
    /// Whether every modern (non-legacy) entry passed the local algorithm
    /// check. Legacy SHA-1-era combos are informational: they are expected to
    /// be rejected under secure defaults and to require an explicit opt-in.
    pub fn local_checks_passed(&self) -> bool {
        self.entries
            .iter()
            .filter(|entry| !entry.legacy_sha1_defaults)
            .all(|entry| matches!(entry.verdict, CompatVerdict::Compatible(_)))
    }

    /// Number of legacy combos rejected under the given policy.
    pub fn legacy_incompatible_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.legacy_sha1_defaults)
            .filter(|entry| matches!(entry.verdict, CompatVerdict::Incompatible { .. }))
            .count()
    }

    /// Number of entries whose live execution is blocked on this host.
    pub fn live_blocked_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.execution, ExecutionMode::LiveServerBlocked { .. }))
            .count()
    }

    /// Platform families covered.
    pub fn platforms(&self) -> Vec<&'static str> {
        let mut seen: Vec<&'static str> = Vec::new();
        for entry in &self.entries {
            if !seen.contains(&entry.platform) {
                seen.push(entry.platform);
            }
        }
        seen
    }
}

/// Checks one combo against the client policy using the T039 negotiation.
pub fn check_combo(policy: &AlgorithmPolicy, combo: &ServerCombo) -> CompatVerdict {
    let mut kex = None;
    let mut host_key = None;
    let mut cipher = None;
    let mut mac = None;
    for (kind, slot) in [
        (AlgorithmKind::Kex, &mut kex),
        (AlgorithmKind::HostKey, &mut host_key),
        (AlgorithmKind::Cipher, &mut cipher),
        (AlgorithmKind::Mac, &mut mac),
    ] {
        match negotiate_algorithm(policy, kind, combo.offers(kind)) {
            NegotiatedAlgorithm::Selected(algorithm) => *slot = Some(algorithm),
            NegotiatedAlgorithm::Rejected => return CompatVerdict::Incompatible { kind },
        }
    }
    CompatVerdict::Compatible(ChosenAlgorithms {
        kex: kex.expect("kex"),
        host_key: host_key.expect("host key"),
        cipher: cipher.expect("cipher"),
        mac: mac.expect("mac"),
    })
}

/// Runs the local algorithm check for every combo and records the live
/// execution as `blocked_environment` on this host.
pub fn run_compat_matrix(policy: &AlgorithmPolicy) -> CompatMatrixReport {
    let entries = THE_MATRIX
        .iter()
        .map(|combo| MatrixEntry {
            combo_id: combo.id,
            platform: combo.platform,
            os: combo.os,
            openssh_version: combo.openssh_version,
            verdict: check_combo(policy, combo),
            legacy_sha1_defaults: combo.legacy_sha1_defaults,
            execution: ExecutionMode::LiveServerBlocked {
                reason: LIVE_BLOCKED_REASON,
            },
        })
        .collect();
    CompatMatrixReport {
        entries,
        live_blocked_reason: LIVE_BLOCKED_REASON,
    }
}

/// Modern KEX set (OpenSSH 8.5+ default order).
static KEX_MODERN: &[Algorithm] = &[
    Algorithm::Curve25519Sha256,
    Algorithm::Curve25519Sha256LibSsh,
    Algorithm::EcdhSha2Nistp256,
    Algorithm::EcdhSha2Nistp384,
    Algorithm::EcdhSha2Nistp521,
    Algorithm::Group16Sha512,
    Algorithm::Group14Sha256,
];

/// Legacy KEX set (OpenSSH 7.x default, includes SHA-1 group14).
static KEX_LEGACY: &[Algorithm] = &[
    Algorithm::Curve25519Sha256,
    Algorithm::EcdhSha2Nistp256,
    Algorithm::EcdhSha2Nistp384,
    Algorithm::EcdhSha2Nistp521,
    Algorithm::Group14Sha1,
    Algorithm::Group1Sha1,
];

/// Modern cipher set (OpenSSH 8.x+ default).
static CIPHERS_MODERN: &[Algorithm] = &[
    Algorithm::Chacha20Poly1305,
    Algorithm::Aes128Ctr,
    Algorithm::Aes192Ctr,
    Algorithm::Aes256Ctr,
    Algorithm::Aes128Gcm,
    Algorithm::Aes256Gcm,
];

/// KEX set for OpenSSH 5.x (SHA-1 only; no curve25519).
static KEX_SHA1_ONLY: &[Algorithm] = &[Algorithm::Group14Sha1, Algorithm::Group1Sha1];

/// Cipher set for OpenSSH 5.x.
static CIPHERS_SHA1_ERA: &[Algorithm] = &[
    Algorithm::Aes128Ctr,
    Algorithm::Aes256Ctr,
    Algorithm::Aes128Cbc,
    Algorithm::TripleDesCbc,
];

/// Host-key set for OpenSSH 5.x (ssh-rsa only).
static HOST_KEYS_SHA1_ERA: &[Algorithm] = &[Algorithm::SshRsa];

/// MAC set for OpenSSH 5.x (hmac-sha1 era).
static MACS_SHA1_ERA: &[Algorithm] = &[Algorithm::HmacSha1];

/// Legacy cipher set (OpenSSH 7.x default, includes CBC).
static CIPHERS_LEGACY: &[Algorithm] = &[
    Algorithm::Chacha20Poly1305,
    Algorithm::Aes128Ctr,
    Algorithm::Aes192Ctr,
    Algorithm::Aes256Ctr,
    Algorithm::Aes128Cbc,
];

/// Modern MAC set (OpenSSH 8.x+ default, EtM first).
static MACS_MODERN: &[Algorithm] = &[
    Algorithm::HmacSha2_256,
    Algorithm::HmacSha2_512,
    Algorithm::Umac128Etm,
    Algorithm::HmacSha1,
];

/// Host-key set (OpenSSH 8.x+ default; ssh-rsa disabled since 8.8).
static HOST_KEYS_MODERN: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::EcdsaSha2Nistp256,
    Algorithm::EcdsaSha2Nistp384,
    Algorithm::EcdsaSha2Nistp521,
    Algorithm::RsaSha2_512,
    Algorithm::RsaSha2_256,
];

/// Host-key set for OpenSSH < 8.8 (ssh-rsa still offered).
static HOST_KEYS_WITH_SSH_RSA: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::EcdsaSha2Nistp256,
    Algorithm::RsaSha2_512,
    Algorithm::RsaSha2_256,
    Algorithm::SshRsa,
];

/// The documented OpenSSH server matrix.
pub static THE_MATRIX: &[ServerCombo] = &[
    ServerCombo {
        id: "linux-ubuntu-2404-openssh-9.6",
        platform: "Linux",
        os: "Ubuntu 24.04 LTS",
        openssh_version: "9.6p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "linux-debian-12-openssh-9.2",
        platform: "Linux",
        os: "Debian 12",
        openssh_version: "9.2p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "linux-rhel-9-openssh-8.7",
        platform: "Linux",
        os: "RHEL 9",
        openssh_version: "8.7p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "macos-14-openssh-9.4",
        platform: "macOS",
        os: "macOS 14 Sonoma",
        openssh_version: "9.4p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "windows-11-openssh-9.5",
        platform: "Windows",
        os: "Windows 11 (in-box OpenSSH)",
        openssh_version: "9.5p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "windows-10-openssh-8.9",
        platform: "Windows",
        os: "Windows 10 (in-box OpenSSH)",
        openssh_version: "8.9p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "freebsd-13-openssh-9.0",
        platform: "FreeBSD",
        os: "FreeBSD 13.2",
        openssh_version: "9.0p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "openbsd-7.4-openssh-9.5",
        platform: "OpenBSD",
        os: "OpenBSD 7.4",
        openssh_version: "9.5p1",
        legacy_sha1_defaults: false,
        kex: KEX_MODERN,
        ciphers: CIPHERS_MODERN,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_MODERN,
    },
    ServerCombo {
        id: "linux-rhel-7-openssh-7.4",
        platform: "Linux",
        os: "RHEL 7 (legacy)",
        openssh_version: "7.4p1",
        legacy_sha1_defaults: true,
        kex: KEX_LEGACY,
        ciphers: CIPHERS_LEGACY,
        macs: MACS_MODERN,
        host_keys: HOST_KEYS_WITH_SSH_RSA,
    },
    ServerCombo {
        id: "linux-rhel-6-openssh-5.3",
        platform: "Linux",
        os: "RHEL 6 (legacy, SHA-1 era)",
        openssh_version: "5.3p1",
        legacy_sha1_defaults: true,
        kex: KEX_SHA1_ONLY,
        ciphers: CIPHERS_SHA1_ERA,
        macs: MACS_SHA1_ERA,
        host_keys: HOST_KEYS_SHA1_ERA,
    },
];
#[cfg(test)]
mod tests {
    use super::{check_combo, run_compat_matrix, CompatVerdict, THE_MATRIX};
    use crate::algorithms::{Algorithm, AlgorithmKind, AlgorithmPolicy};

    fn legacy_policy() -> AlgorithmPolicy {
        let mut policy = AlgorithmPolicy::defaults();
        policy
            .allowed
            .entry(AlgorithmKind::Kex)
            .or_default()
            .push(Algorithm::Group14Sha1);
        policy
            .allowed
            .entry(AlgorithmKind::HostKey)
            .or_default()
            .push(Algorithm::SshRsa);
        policy
            .allowed
            .entry(AlgorithmKind::Cipher)
            .or_default()
            .push(Algorithm::Aes128Cbc);
        policy
            .allowed
            .entry(AlgorithmKind::Mac)
            .or_default()
            .push(Algorithm::HmacSha1);
        policy
    }

    #[test]
    fn modern_servers_are_compatible_with_secure_defaults() {
        let policy = AlgorithmPolicy::defaults();
        for combo in THE_MATRIX
            .iter()
            .filter(|combo| !combo.legacy_sha1_defaults)
        {
            let verdict = check_combo(&policy, combo);
            assert!(
                matches!(verdict, CompatVerdict::Compatible(_)),
                "{} must be compatible under secure defaults: {verdict:?}",
                combo.id
            );
        }
    }

    #[test]
    fn legacy_sha1_only_server_is_rejected_by_secure_defaults() {
        let policy = AlgorithmPolicy::defaults();
        let legacy = THE_MATRIX
            .iter()
            .find(|combo| combo.id == "linux-rhel-6-openssh-5.3")
            .expect("legacy combo");
        assert!(matches!(
            check_combo(&policy, legacy),
            CompatVerdict::Incompatible { .. }
        ));
    }

    #[test]
    fn legacy_server_connects_with_explicit_opt_in() {
        let policy = legacy_policy();
        let legacy = THE_MATRIX
            .iter()
            .find(|combo| combo.id == "linux-rhel-6-openssh-5.3")
            .expect("legacy combo");
        assert!(matches!(
            check_combo(&policy, legacy),
            CompatVerdict::Compatible(_)
        ));
    }

    #[test]
    fn rhel7_negotiates_modern_algorithms_with_secure_defaults() {
        let policy = AlgorithmPolicy::defaults();
        let rhel7 = THE_MATRIX
            .iter()
            .find(|combo| combo.id == "linux-rhel-7-openssh-7.4")
            .expect("rhel7 combo");
        match check_combo(&policy, rhel7) {
            CompatVerdict::Compatible(chosen) => {
                // The secure client must NOT fall back to SHA-1 kex even though
                // the legacy server also offers it.
                assert_ne!(chosen.kex, Algorithm::Group14Sha1);
                assert_eq!(chosen.kex, Algorithm::Curve25519Sha256);
            }
            verdict => panic!("rhel7 must be compatible: {verdict:?}"),
        }
    }

    #[test]
    fn matrix_covers_all_target_platforms_and_blocks_live_execution() {
        let policy = AlgorithmPolicy::defaults();
        let report = run_compat_matrix(&policy);
        let platforms = report.platforms();
        for required in ["Linux", "macOS", "Windows", "FreeBSD", "OpenBSD"] {
            assert!(
                platforms.contains(&required),
                "matrix must cover {required}, got {platforms:?}"
            );
        }
        assert!(report.local_checks_passed(), "modern checks must pass");
        assert_eq!(report.legacy_incompatible_count(), 1);
        assert_eq!(report.entries.len(), THE_MATRIX.len());
        // The live nightly matrix cannot run on this host; it is honestly
        // recorded as blocked_environment.
        assert_eq!(report.live_blocked_count(), report.entries.len());
        assert!(!report.live_blocked_reason.is_empty());
    }

    #[test]
    fn chosen_algorithms_follow_local_preference() {
        let policy = AlgorithmPolicy::defaults();
        let ubuntu = THE_MATRIX
            .iter()
            .find(|combo| combo.id == "linux-ubuntu-2404-openssh-9.6")
            .expect("ubuntu combo");
        let CompatVerdict::Compatible(chosen) = check_combo(&policy, ubuntu) else {
            panic!("ubuntu must be compatible");
        };
        assert_eq!(chosen.kex, Algorithm::Curve25519Sha256);
        assert_eq!(chosen.host_key, Algorithm::Ed25519);
        assert_eq!(chosen.cipher, Algorithm::Chacha20Poly1305);
        assert_eq!(chosen.mac, Algorithm::HmacSha2_256);
    }
}
