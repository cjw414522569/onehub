//! macOS sandbox, hardened runtime, and minimal entitlement set (T124).
//!
//! [`EntitlementSet::minimal`] is the baseline (sandbox, network client,
//! user-selected files); on-demand entitlements (network server for
//! forwarding, keychain access group) may be added explicitly. The
//! [`NotarizationAudit`] runs the pre-notarization security checks: hardened
//! runtime enabled, sandbox enabled, no `get-task-allow` in release, and no
//! extra entitlements. Real `codesign` / `spctl` / entitlement audits run on
//! macOS hosts; this module locks the deterministic model.

/// A macOS entitlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entitlement {
    /// App Sandbox.
    Sandbox,
    /// Outgoing network (client).
    NetworkClient,
    /// Incoming network (server, for port forwarding) - on demand.
    NetworkServer,
    /// User-selected file read/write.
    UserSelectedFiles,
    /// Keychain access group - on demand.
    KeychainAccessGroup,
}

/// An entitlement set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementSet {
    /// The entitlements.
    pub entitlements: Vec<Entitlement>,
}

impl EntitlementSet {
    /// The minimal release set: sandbox, network client, user-selected files.
    pub fn minimal() -> Self {
        Self {
            entitlements: vec![
                Entitlement::Sandbox,
                Entitlement::NetworkClient,
                Entitlement::UserSelectedFiles,
            ],
        }
    }

    /// Adds an entitlement (on-demand).
    pub fn with(mut self, entitlement: Entitlement) -> Self {
        if !self.entitlements.contains(&entitlement) {
            self.entitlements.push(entitlement);
        }
        self
    }

    /// Whether an entitlement is present.
    pub fn contains(&self, entitlement: Entitlement) -> bool {
        self.entitlements.contains(&entitlement)
    }

    /// Whether the set is exactly the minimal baseline.
    pub fn is_minimal(&self) -> bool {
        *self == Self::minimal()
    }

    /// On-demand entitlements that may be added without failing the audit.
    pub fn allowed_on_demand() -> &'static [Entitlement] {
        &[Entitlement::NetworkServer, Entitlement::KeychainAccessGroup]
    }
}

/// A pre-notarization audit issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditIssue {
    /// A stable code.
    pub code: &'static str,
    /// A human message.
    pub message: String,
}

/// The pre-notarization security audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotarizationAudit {
    /// Whether the hardened runtime is enabled.
    pub hardened_runtime: bool,
    /// Whether the App Sandbox is enabled.
    pub sandbox: bool,
    /// The entitlement set.
    pub entitlements: EntitlementSet,
    /// Whether `get-task-allow` is set (forbidden in release).
    pub get_task_allow: bool,
    /// Extra raw entitlements beyond the model (e.g. custom ones).
    pub extra_entitlements: Vec<&'static str>,
}

impl NotarizationAudit {
    /// Runs the pre-notarization checks.
    pub fn check(&self) -> Vec<AuditIssue> {
        let mut issues = Vec::new();
        if !self.hardened_runtime {
            issues.push(AuditIssue {
                code: "HARDENED_RUNTIME_REQUIRED",
                message: "the hardened runtime must be enabled before notarization".to_owned(),
            });
        }
        if !self.sandbox {
            issues.push(AuditIssue {
                code: "SANDBOX_REQUIRED",
                message: "the app sandbox must be enabled".to_owned(),
            });
        }
        if self.get_task_allow {
            issues.push(AuditIssue {
                code: "GET_TASK_ALLOW_FORBIDDEN",
                message: "get-task-allow must be false for release builds".to_owned(),
            });
        }
        for entitlement in &self.entitlements.entitlements {
            let baseline = EntitlementSet::minimal().entitlements;
            let on_demand = EntitlementSet::allowed_on_demand();
            if !baseline.contains(entitlement) && !on_demand.contains(entitlement) {
                issues.push(AuditIssue {
                    code: "EXTRA_ENTITLEMENT",
                    message: format!("unexpected entitlement {:?}", entitlement),
                });
            }
        }
        for extra in &self.extra_entitlements {
            issues.push(AuditIssue {
                code: "EXTRA_ENTITLEMENT",
                message: format!("unexpected entitlement {extra}"),
            });
        }
        issues
    }

    /// Whether all pre-notarization checks pass.
    pub fn passes(&self) -> bool {
        self.check().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Entitlement, EntitlementSet, NotarizationAudit};

    fn release_audit() -> NotarizationAudit {
        NotarizationAudit {
            hardened_runtime: true,
            sandbox: true,
            entitlements: EntitlementSet::minimal(),
            get_task_allow: false,
            extra_entitlements: Vec::new(),
        }
    }

    #[test]
    fn minimal_release_audit_passes() {
        let audit = release_audit();
        assert!(EntitlementSet::minimal().is_minimal());
        assert!(audit.passes(), "minimal + hardened + sandbox must pass");
    }

    #[test]
    fn hardened_runtime_and_sandbox_are_required() {
        let audit = NotarizationAudit {
            hardened_runtime: false,
            sandbox: true,
            ..release_audit()
        };
        let issues = audit.check();
        assert!(issues.iter().any(|i| i.code == "HARDENED_RUNTIME_REQUIRED"));
        let audit = NotarizationAudit {
            sandbox: false,
            ..release_audit()
        };
        assert!(audit.check().iter().any(|i| i.code == "SANDBOX_REQUIRED"));
    }

    #[test]
    fn get_task_allow_is_forbidden_in_release() {
        let audit = NotarizationAudit {
            get_task_allow: true,
            ..release_audit()
        };
        assert!(!audit.passes());
        assert!(audit
            .check()
            .iter()
            .any(|i| i.code == "GET_TASK_ALLOW_FORBIDDEN"));
    }

    #[test]
    fn on_demand_entitlements_are_allowed_but_flagged_extra() {
        // Network server (port forwarding) is on-demand and allowed.
        let audit = NotarizationAudit {
            entitlements: EntitlementSet::minimal().with(Entitlement::NetworkServer),
            ..release_audit()
        };
        assert!(!audit.entitlements.is_minimal());
        assert!(audit.passes(), "on-demand entitlement is allowed");
        // A raw extra entitlement is flagged.
        let audit = NotarizationAudit {
            extra_entitlements: vec!["com.apple.security.cs.allow-jit"],
            ..release_audit()
        };
        let issues = audit.check();
        assert!(!audit.passes());
        assert!(issues.iter().any(|i| i.code == "EXTRA_ENTITLEMENT"));
    }
}
