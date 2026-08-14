use core_protocol::capabilities::PlatformId;

/// Hardware key technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareKeyKind {
    /// FIDO2/CTAP2 (security keys, platform authenticators).
    Fido2,
    /// PKCS#11 tokens (smart cards, HSM-backed keys).
    Pkcs11,
}

impl HardwareKeyKind {
    /// Stable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            HardwareKeyKind::Fido2 => "fido2",
            HardwareKeyKind::Pkcs11 => "pkcs11",
        }
    }
}

/// Outcome of the hardware key capability gate for a platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareKeyGate {
    /// The technology is supported and enabled on this platform.
    Supported { kind: HardwareKeyKind },
    /// The technology is explicitly disabled on this platform with a reason.
    Disabled {
        /// The technology.
        kind: HardwareKeyKind,
        /// Stable reason code.
        reason: &'static str,
    },
}

impl HardwareKeyGate {
    /// Whether the gate allows the technology.
    pub fn is_supported(&self) -> bool {
        matches!(self, HardwareKeyGate::Supported { .. })
    }
}

/// The default hardware key support matrix.
///
/// Unsupported platforms are explicitly disabled with a stable reason so the
/// UI can explain exactly why a hardware key feature is unavailable.
pub fn hardware_key_gate(platform: PlatformId, kind: HardwareKeyKind) -> HardwareKeyGate {
    use HardwareKeyKind::*;
    use PlatformId::*;
    match (platform, kind) {
        // FIDO2 is available on desktop and mobile platforms via CTAP2.
        (Windows, Fido2) | (MacOS, Fido2) | (Linux, Fido2) | (Android, Fido2) | (Ios, Fido2) => {
            HardwareKeyGate::Supported { kind }
        }
        // PKCS#11 is a desktop-token technology.
        (Windows, Pkcs11) | (MacOS, Pkcs11) | (Linux, Pkcs11) => {
            HardwareKeyGate::Supported { kind }
        }
        // Mobile platforms have no generic PKCS#11 middleware.
        (Android, Pkcs11) => HardwareKeyGate::Disabled {
            kind,
            reason: "no-pkcs11-middleware",
        },
        (Ios, Pkcs11) => HardwareKeyGate::Disabled {
            kind,
            reason: "no-pkcs11-middleware",
        },
        // Web/PWA cannot reach raw USB/NFC hardware.
        (Web, Fido2) | (Web, Pkcs11) => HardwareKeyGate::Disabled {
            kind,
            reason: "browser-cannot-reach-raw-hardware",
        },
        // The CLI can use FIDO2 via the ssh agent only if the host exposes it.
        (Cli, Fido2) | (Cli, Pkcs11) => HardwareKeyGate::Disabled {
            kind,
            reason: "requires-agent-or-host-middleware",
        },
    }
}

/// A backend that performs a hardware key operation (soft-token simulation
/// implements this for CI; a real CTAP2/PKCS#11 backend implements it for
/// hardware-verified runs).
#[allow(async_fn_in_trait)]
pub trait HardwareKeyBackend: Send + Sync {
    /// Whether the backend is present and usable right now.
    fn is_available(&self) -> bool;
    /// Stable backend name.
    fn name(&self) -> &'static str;
}

/// Resolves the effective gate for a platform, combining the static matrix
/// with a runtime backend probe.
pub fn effective_gate(
    platform: PlatformId,
    kind: HardwareKeyKind,
    backend: &dyn HardwareKeyBackend,
) -> HardwareKeyGate {
    match hardware_key_gate(platform, kind) {
        HardwareKeyGate::Supported { kind } if backend.is_available() => {
            HardwareKeyGate::Supported { kind }
        }
        HardwareKeyGate::Supported { kind } => HardwareKeyGate::Disabled {
            kind,
            reason: "no-backend-present",
        },
        disabled => disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        effective_gate, hardware_key_gate, HardwareKeyBackend, HardwareKeyGate, HardwareKeyKind,
    };
    use core_protocol::capabilities::PlatformId;

    struct SoftToken;
    impl HardwareKeyBackend for SoftToken {
        fn is_available(&self) -> bool {
            true
        }
        fn name(&self) -> &'static str {
            "soft-token"
        }
    }

    struct Absent;
    impl HardwareKeyBackend for Absent {
        fn is_available(&self) -> bool {
            false
        }
        fn name(&self) -> &'static str {
            "absent"
        }
    }

    #[test]
    fn fido2_support_matrix() {
        for platform in [
            PlatformId::Windows,
            PlatformId::MacOS,
            PlatformId::Linux,
            PlatformId::Android,
            PlatformId::Ios,
        ] {
            assert!(
                hardware_key_gate(platform, HardwareKeyKind::Fido2).is_supported(),
                "{platform:?} should support FIDO2"
            );
        }
        // Web and CLI explicitly disable FIDO2 with a reason.
        assert_eq!(
            hardware_key_gate(PlatformId::Web, HardwareKeyKind::Fido2),
            HardwareKeyGate::Disabled {
                kind: HardwareKeyKind::Fido2,
                reason: "browser-cannot-reach-raw-hardware"
            }
        );
        assert_eq!(
            hardware_key_gate(PlatformId::Cli, HardwareKeyKind::Fido2),
            HardwareKeyGate::Disabled {
                kind: HardwareKeyKind::Fido2,
                reason: "requires-agent-or-host-middleware"
            }
        );
    }

    #[test]
    fn pkcs11_support_matrix() {
        for platform in [PlatformId::Windows, PlatformId::MacOS, PlatformId::Linux] {
            assert!(
                hardware_key_gate(platform, HardwareKeyKind::Pkcs11).is_supported(),
                "{platform:?} should support PKCS#11"
            );
        }
        // Mobile and web explicitly disable PKCS#11 with reasons.
        assert_eq!(
            hardware_key_gate(PlatformId::Android, HardwareKeyKind::Pkcs11),
            HardwareKeyGate::Disabled {
                kind: HardwareKeyKind::Pkcs11,
                reason: "no-pkcs11-middleware"
            }
        );
        assert_eq!(
            hardware_key_gate(PlatformId::Ios, HardwareKeyKind::Pkcs11),
            HardwareKeyGate::Disabled {
                kind: HardwareKeyKind::Pkcs11,
                reason: "no-pkcs11-middleware"
            }
        );
        assert_eq!(
            hardware_key_gate(PlatformId::Web, HardwareKeyKind::Pkcs11),
            HardwareKeyGate::Disabled {
                kind: HardwareKeyKind::Pkcs11,
                reason: "browser-cannot-reach-raw-hardware"
            }
        );
    }

    #[test]
    fn every_platform_kind_pair_has_a_defined_gate() {
        // The gate is total: no pair is undefined.
        use PlatformId::*;
        for platform in [Windows, MacOS, Linux, Android, Ios, Web, Cli] {
            for kind in [HardwareKeyKind::Fido2, HardwareKeyKind::Pkcs11] {
                let gate = hardware_key_gate(platform, kind);
                match gate {
                    HardwareKeyGate::Supported { .. } => {}
                    HardwareKeyGate::Disabled { reason, .. } => {
                        assert!(!reason.is_empty(), "disabled gate needs a reason");
                    }
                }
            }
        }
    }

    #[test]
    fn effective_gate_requires_runtime_backend() {
        // Soft token CI: the platform supports it and the backend is present.
        let soft = SoftToken;
        assert!(effective_gate(PlatformId::Windows, HardwareKeyKind::Fido2, &soft).is_supported());
        // No backend present -> disabled with "no-backend-present".
        let absent = Absent;
        assert_eq!(
            effective_gate(PlatformId::Windows, HardwareKeyKind::Fido2, &absent),
            HardwareKeyGate::Disabled {
                kind: HardwareKeyKind::Fido2,
                reason: "no-backend-present"
            }
        );
        // Disabled-by-matrix platforms stay disabled regardless of backend.
        assert_eq!(
            effective_gate(PlatformId::Web, HardwareKeyKind::Fido2, &soft),
            HardwareKeyGate::Disabled {
                kind: HardwareKeyKind::Fido2,
                reason: "browser-cannot-reach-raw-hardware"
            }
        );
    }
}
