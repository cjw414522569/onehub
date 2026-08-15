//! App startup, database unlock, and failure-recovery flow (T101).
//!
//! [`StartupFlow::run`] produces a structured [`StartupOutcome`] with
//! **actionable prompts** for every failure mode a user can hit at launch:
//! corrupted database, failed migration, a rejected/read-only open policy,
//! and a locked OS secure store. Each prompt carries a stable machine code,
//! a severity, a human title/message, and the concrete action the user
//! should take — so the app can show a "what happened / what to do" dialog
//! instead of a bare error.

use secure_store::SecureStore;
use storage_sqlite::migration::open_strategy;
use storage_sqlite::{MigrationError, Migrator, OpenDecision, OpenPolicy};

/// How severe a startup prompt is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptSeverity {
    /// Informational.
    Info,
    /// The app started but with degraded capabilities.
    Warning,
    /// The app could not start normally.
    Error,
}

/// A user-actionable startup prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionablePrompt {
    /// Severity.
    pub severity: PromptSeverity,
    /// Stable machine code (e.g. "DB_MIGRATION_FAILED").
    pub code: &'static str,
    /// Short human title.
    pub title: String,
    /// What happened.
    pub message: String,
    /// What the user should do.
    pub action: String,
}

/// The health of the local database file before startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseHealth {
    /// The file opens; it is at the given schema version.
    Healthy {
        /// Current schema version.
        current_version: u32,
    },
    /// The file is corrupted/unreadable.
    Corrupted {
        /// Human-readable reason.
        reason: String,
    },
}

/// Startup configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupConfig {
    /// How to treat a database with a mismatched version.
    pub open_policy: OpenPolicy,
    /// The app's target schema version.
    pub target_version: u32,
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            open_policy: OpenPolicy::Upgrade,
            target_version: 3,
        }
    }
}

/// The result of a startup attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupOutcome {
    /// Whether the database opened (read-write or read-only).
    pub opened: bool,
    /// The schema version the database ended at (0 when not opened).
    pub schema_version: u32,
    /// Whether the OS secure store is unlocked and usable.
    pub secure_store_available: bool,
    /// All prompts the UI should show, in order.
    pub prompts: Vec<ActionablePrompt>,
}

impl StartupOutcome {
    /// Prompts at or above a severity.
    pub fn prompts_at_least(&self, severity: PromptSeverity) -> Vec<&ActionablePrompt> {
        let threshold = severity as u8;
        self.prompts
            .iter()
            .filter(|prompt| (prompt.severity as u8) >= threshold)
            .collect()
    }

    /// Whether the startup has any error prompt.
    pub fn has_errors(&self) -> bool {
        self.prompts
            .iter()
            .any(|prompt| prompt.severity == PromptSeverity::Error)
    }
}

/// The startup orchestration flow.
pub struct StartupFlow;

impl StartupFlow {
    /// Runs the full startup: open/migrate the database, unlock the secure
    /// store, and collect actionable prompts for every failure mode.
    pub fn run(
        config: &StartupConfig,
        health: &DatabaseHealth,
        migrator: &Migrator,
        context: &mut storage_sqlite::MigrationContext,
        store: &dyn SecureStore,
    ) -> StartupOutcome {
        let mut outcome = StartupOutcome {
            opened: false,
            schema_version: context.current(),
            secure_store_available: store.is_available(),
            prompts: Vec::new(),
        };

        // 1. Corruption: the file cannot be read at all.
        if let DatabaseHealth::Corrupted { reason } = health {
            outcome.prompts.push(ActionablePrompt {
                severity: PromptSeverity::Error,
                code: "DB_CORRUPTED",
                title: "Database is corrupted".to_owned(),
                message: reason.clone(),
                action: "Restore the latest backup, or start fresh (losing the corrupted copy)."
                    .to_owned(),
            });
            return outcome;
        }

        // 2. Open strategy (version policy).
        let current = match health {
            DatabaseHealth::Healthy { current_version } => *current_version,
            DatabaseHealth::Corrupted { .. } => unreachable!("handled above"),
        };
        outcome.schema_version = current;
        match open_strategy(current, config.target_version, config.open_policy) {
            OpenDecision::Migrate(from) => {
                match migrator.migrate(context, config.target_version) {
                    Ok(version) if version < config.target_version => {
                        // The database is older, but the app has no migration
                        // path from it up to the target version.
                        outcome.prompts.push(ActionablePrompt {
                            severity: PromptSeverity::Error,
                            code: "DB_UNKNOWN_VERSION",
                            title: "Unknown database version".to_owned(),
                            message: format!(
                                "No migration path exists from v{from} to v{target}.",
                                target = config.target_version
                            ),
                            action: "Install the app version that wrote this database.".to_owned(),
                        });
                    }
                    Ok(version) => {
                        outcome.opened = true;
                        outcome.schema_version = version;
                        outcome.prompts.push(ActionablePrompt {
                            severity: PromptSeverity::Info,
                            code: "DB_OPENED",
                            title: "Database opened".to_owned(),
                            message: format!("Migrated the database from v{from} to v{version}."),
                            action: "Continue to the main window.".to_owned(),
                        });
                    }
                    Err(MigrationError::StepFailed(step)) => {
                        outcome.schema_version = context.current();
                        outcome.prompts.push(ActionablePrompt {
                            severity: PromptSeverity::Error,
                            code: "DB_MIGRATION_FAILED",
                            title: "Database migration failed".to_owned(),
                            message: format!(
                                "The migration step '{step}' failed; your data is still at v{current}."
                            ),
                            action: "Restore the pre-migration backup, or contact support with the step name."
                                .to_owned(),
                        });
                    }
                    Err(MigrationError::UnknownVersion(version)) => {
                        outcome.prompts.push(ActionablePrompt {
                            severity: PromptSeverity::Error,
                            code: "DB_UNKNOWN_VERSION",
                            title: "Unknown database version".to_owned(),
                            message: format!("No migration path exists to v{version}."),
                            action: "Install the app version that wrote this database.".to_owned(),
                        });
                    }
                    Err(MigrationError::NoRevert(version)) => {
                        outcome.prompts.push(ActionablePrompt {
                            severity: PromptSeverity::Error,
                            code: "DB_NO_REVERT",
                            title: "Cannot roll back".to_owned(),
                            message: format!("Version v{version} has no revert step."),
                            action: "Contact support; keep the current data untouched.".to_owned(),
                        });
                    }
                }
            }
            OpenDecision::Reject(reason) => {
                outcome.prompts.push(ActionablePrompt {
                    severity: PromptSeverity::Error,
                    code: "DB_REJECTED",
                    title: "Database version rejected".to_owned(),
                    message: reason,
                    action: "Install the app version that matches your database.".to_owned(),
                });
            }
            OpenDecision::OpenReadOnly => {
                outcome.opened = true;
                outcome.prompts.push(ActionablePrompt {
                    severity: PromptSeverity::Warning,
                    code: "DB_READ_ONLY",
                    title: "Database opened read-only".to_owned(),
                    message: "The database is newer than this app; changes are disabled."
                        .to_owned(),
                    action: "Update the app to edit your data.".to_owned(),
                });
            }
        }

        // 3. Secure-store unlock.
        if !outcome.secure_store_available {
            outcome.prompts.push(ActionablePrompt {
                severity: PromptSeverity::Error,
                code: "SECURE_STORE_LOCKED",
                title: "Secure storage is locked".to_owned(),
                message: "The OS keychain / credential store is locked or unavailable.".to_owned(),
                action:
                    "Unlock your operating system (sign in / dismiss the lock screen) and restart."
                        .to_owned(),
            });
        }

        outcome
    }
}

#[cfg(test)]
mod tests {
    use secure_store::MemorySecureStore;
    use storage_sqlite::{Migration, MigrationContext, MigrationError, Migrator, OpenPolicy};

    use super::{ActionablePrompt, DatabaseHealth, PromptSeverity, StartupConfig, StartupFlow};

    fn step(version: u32, name: &str) -> Migration {
        Migration {
            version,
            name: name.to_owned(),
            apply: Box::new(move |ctx| {
                ctx.schema.push_str(&format!("v{version};"));
                Ok(())
            }),
            revert: Some(Box::new(move |ctx| {
                let marker = format!("v{version};");
                if ctx.schema.ends_with(&marker) {
                    ctx.schema.truncate(ctx.schema.len() - marker.len());
                }
                Ok(())
            })),
        }
    }

    fn failing_step(version: u32, name: &str) -> Migration {
        Migration {
            version,
            name: name.to_owned(),
            apply: Box::new(|_| Err(MigrationError::StepFailed("boom".to_owned()))),
            revert: None,
        }
    }

    fn healthy(current: u32) -> DatabaseHealth {
        DatabaseHealth::Healthy {
            current_version: current,
        }
    }

    fn prompt<'a>(outcome: &'a super::StartupOutcome, code: &str) -> Option<&'a ActionablePrompt> {
        outcome.prompts.iter().find(|prompt| prompt.code == code)
    }

    #[test]
    fn healthy_startup_migrates_and_prompts_info() {
        let migrator = Migrator::new(vec![step(1, "s1"), step(2, "s2"), step(3, "s3")]);
        let mut context = MigrationContext::default();
        let store = MemorySecureStore::new();
        let outcome = StartupFlow::run(
            &StartupConfig::default(),
            &healthy(0),
            &migrator,
            &mut context,
            &store,
        );
        assert!(outcome.opened);
        assert_eq!(outcome.schema_version, 3);
        assert!(outcome.secure_store_available);
        assert!(!outcome.has_errors());
        let opened = prompt(&outcome, "DB_OPENED").expect("info prompt");
        assert_eq!(opened.severity, PromptSeverity::Info);
        assert!(!opened.action.is_empty());
        assert_eq!(context.applied, vec![1, 2, 3]);
    }

    #[test]
    fn corruption_yields_actionable_prompt() {
        let migrator = Migrator::new(vec![step(1, "s1")]);
        let mut context = MigrationContext::default();
        let store = MemorySecureStore::new();
        let outcome = StartupFlow::run(
            &StartupConfig::default(),
            &DatabaseHealth::Corrupted {
                reason: "header checksum mismatch".to_owned(),
            },
            &migrator,
            &mut context,
            &store,
        );
        assert!(!outcome.opened);
        assert!(outcome.has_errors());
        let corrupted = prompt(&outcome, "DB_CORRUPTED").expect("corruption prompt");
        assert_eq!(corrupted.severity, PromptSeverity::Error);
        assert!(corrupted.message.contains("checksum"));
        assert!(corrupted.action.contains("backup"));
    }

    #[test]
    fn migration_failure_yields_actionable_prompt_and_keeps_data() {
        let migrator = Migrator::new(vec![step(1, "s1"), failing_step(2, "s2"), step(3, "s3")]);
        let mut context = MigrationContext::default();
        let store = MemorySecureStore::new();
        let outcome = StartupFlow::run(
            &StartupConfig::default(),
            &healthy(0),
            &migrator,
            &mut context,
            &store,
        );
        assert!(!outcome.opened);
        assert!(outcome.has_errors());
        let failed = prompt(&outcome, "DB_MIGRATION_FAILED").expect("migration prompt");
        assert_eq!(failed.severity, PromptSeverity::Error);
        assert!(failed.message.contains("s2"));
        assert!(failed.action.contains("backup"));
        // Data before the failing step is preserved.
        assert_eq!(context.applied, vec![1]);
        assert_eq!(outcome.schema_version, 1);
    }

    #[test]
    fn secure_store_lock_yields_actionable_prompt() {
        let migrator = Migrator::new(vec![step(1, "s1")]);
        let mut context = MigrationContext::default();
        let mut store = MemorySecureStore::new();
        store.set_available(false);
        let outcome = StartupFlow::run(
            &StartupConfig::default(),
            &healthy(0),
            &migrator,
            &mut context,
            &store,
        );
        assert!(!outcome.secure_store_available);
        let locked = prompt(&outcome, "SECURE_STORE_LOCKED").expect("lock prompt");
        assert_eq!(locked.severity, PromptSeverity::Error);
        assert!(locked.action.to_lowercase().contains("unlock"));
        // An unlocked store produces no lock prompt.
        let mut unlocked = MemorySecureStore::new();
        unlocked.set_available(true);
        let outcome2 = StartupFlow::run(
            &StartupConfig::default(),
            &healthy(0),
            &migrator,
            &mut context,
            &unlocked,
        );
        assert!(prompt(&outcome2, "SECURE_STORE_LOCKED").is_none());
    }

    #[test]
    fn reject_and_read_only_open_policies_prompt() {
        let migrator = Migrator::new(vec![step(1, "s1")]);
        let mut context = MigrationContext::default();
        let store = MemorySecureStore::new();

        // Reject policy with a newer database.
        let outcome = StartupFlow::run(
            &StartupConfig {
                open_policy: OpenPolicy::Reject,
                target_version: 2,
            },
            &healthy(3),
            &migrator,
            &mut context,
            &store,
        );
        assert!(!outcome.opened);
        let rejected = prompt(&outcome, "DB_REJECTED").expect("reject prompt");
        assert_eq!(rejected.severity, PromptSeverity::Error);

        // Read-only policy with a newer database.
        let outcome = StartupFlow::run(
            &StartupConfig {
                open_policy: OpenPolicy::ReadOnly,
                target_version: 2,
            },
            &healthy(3),
            &migrator,
            &mut context,
            &store,
        );
        assert!(outcome.opened);
        let read_only = prompt(&outcome, "DB_READ_ONLY").expect("read-only prompt");
        assert_eq!(read_only.severity, PromptSeverity::Warning);
    }

    #[test]
    fn unknown_version_yields_actionable_prompt() {
        // No migration step reaches target_version 5.
        let migrator = Migrator::new(vec![step(1, "s1"), step(2, "s2")]);
        let mut context = MigrationContext::default();
        let store = MemorySecureStore::new();
        let outcome = StartupFlow::run(
            &StartupConfig {
                open_policy: OpenPolicy::Upgrade,
                target_version: 5,
            },
            &healthy(0),
            &migrator,
            &mut context,
            &store,
        );
        assert!(!outcome.opened);
        let unknown = prompt(&outcome, "DB_UNKNOWN_VERSION").expect("unknown prompt");
        assert_eq!(unknown.severity, PromptSeverity::Error);
        assert!(unknown.action.to_lowercase().contains("install"));
    }
}
