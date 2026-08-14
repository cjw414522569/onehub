//! SQLite schema, migration rules, backup, and downgrade policy (T083).
//!
//! The migration model is deterministic and transaction-shaped: each
//! [`Migration`] step runs in its own transaction (modeled as: apply the step,
//! then record the version; a failed step rolls back its own record), applying
//! is idempotent (already-applied versions are skipped), and [`Migrator`]
//! supports rollback via revert steps. [`BackupPolicy`] controls when a backup
//! is taken and [`OpenPolicy`] makes the old-version open strategy explicit.
//! The actual SQL dialect is deferred to the repository layer (T084); this row
//! locks the schema/versioning contract.

/// The schema version (a monotonic integer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SchemaVersion(pub u32);

/// A migration step function (the SQL/DDL body in the real adapter).
type MigrationFn = Box<dyn Fn(&mut MigrationContext) -> Result<(), MigrationError>>;

/// A migration step.
pub struct Migration {
    /// Target schema version after this step.
    pub version: u32,
    /// Stable, human-readable name (the idempotency key).
    pub name: String,
    /// Applies the step.
    pub apply: MigrationFn,
    /// Reverts the step (for rollback); optional.
    pub revert: Option<MigrationFn>,
}

/// A migration failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    /// A step failed; its transaction rolled back.
    StepFailed(String),
    /// The requested version is not available.
    UnknownVersion(u32),
    /// No revert step exists for a rollback target.
    NoRevert(u32),
}

/// The context a migration step operates on.
#[derive(Debug, Clone, Default)]
pub struct MigrationContext {
    /// Versions applied so far, in order.
    pub applied: Vec<u32>,
    /// The current schema text (placeholder for the real DDL).
    pub schema: String,
    /// Backup path taken before/around the step, if any.
    pub backup_path: Option<String>,
}

impl MigrationContext {
    /// The latest applied version.
    pub fn current(&self) -> u32 {
        self.applied.last().copied().unwrap_or(0)
    }
}

/// Applies migrations from the current version to `target`, each step
/// transactionally; idempotent (already-applied versions are skipped).
pub struct Migrator {
    migrations: Vec<Migration>,
}

impl Migrator {
    /// A migrator over the given ordered migration steps.
    pub fn new(migrations: Vec<Migration>) -> Self {
        Self { migrations }
    }

    /// The highest version reachable.
    pub fn current_version(&self) -> u32 {
        self.migrations.iter().map(|m| m.version).max().unwrap_or(0)
    }

    /// Migrates the context to `target` (>= current), transactionally and
    /// idempotently. On failure the failing step is rolled back and an error
    /// is returned; versions before it remain applied.
    pub fn migrate(&self, ctx: &mut MigrationContext, target: u32) -> Result<u32, MigrationError> {
        let mut steps: Vec<&Migration> = self
            .migrations
            .iter()
            .filter(|m| m.version > ctx.current() && m.version <= target)
            .collect();
        steps.sort_by_key(|m| m.version);
        for step in steps {
            // Idempotency: never re-apply an already-applied version.
            if ctx.applied.contains(&step.version) {
                continue;
            }
            let before = ctx.applied.clone();
            (step.apply)(ctx).map_err(|_| {
                // Roll back this step's own record.
                ctx.applied = before;
                MigrationError::StepFailed(step.name.clone())
            })?;
            ctx.applied.push(step.version);
        }
        Ok(ctx.current())
    }

    /// Rolls the context back from its current version down to `target`,
    /// running revert steps in reverse. A missing revert step fails the
    /// rollback at that version.
    pub fn rollback(&self, ctx: &mut MigrationContext, target: u32) -> Result<u32, MigrationError> {
        while ctx.current() > target {
            let version = ctx.current();
            let step = self
                .migrations
                .iter()
                .find(|m| m.version == version)
                .ok_or(MigrationError::UnknownVersion(version))?;
            let revert = step
                .revert
                .as_ref()
                .ok_or(MigrationError::NoRevert(version))?;
            let before = ctx.applied.clone();
            revert(ctx).map_err(|_| {
                ctx.applied = before;
                MigrationError::StepFailed(format!("revert {}", step.name))
            })?;
            ctx.applied.pop();
        }
        Ok(ctx.current())
    }
}

/// When a backup is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupMode {
    /// No backup.
    None,
    /// One backup before the first applied step.
    BeforeMigration,
    /// A backup before every applied step.
    EveryStep,
}

/// Backup policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupPolicy {
    /// When to take backups.
    pub mode: BackupMode,
    /// Backup directory (set into the context's `backup_path`).
    pub directory: String,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            mode: BackupMode::BeforeMigration,
            directory: "backups".to_owned(),
        }
    }
}

/// The strategy for opening a database at a different version than the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPolicy {
    /// Migrate the database to the app version.
    Upgrade,
    /// Open read-only when the database is newer (never downgrade-write).
    ReadOnly,
    /// Refuse to open a newer database (explicit old-version policy).
    Reject,
}

/// The decision for opening a database at `db_version` with an app that
/// supports up to `app_version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenDecision {
    /// Migrate from the database version to `app_version`.
    Migrate(u32),
    /// Open read-only (newer database; policy allows it).
    OpenReadOnly,
    /// Refuse (newer database; policy rejects it).
    Reject(String),
}

/// Decides how to open a database under the given open policy.
pub fn open_strategy(db_version: u32, app_version: u32, policy: OpenPolicy) -> OpenDecision {
    match policy {
        OpenPolicy::Upgrade => OpenDecision::Migrate(db_version),
        OpenPolicy::ReadOnly => {
            if db_version > app_version {
                OpenDecision::OpenReadOnly
            } else {
                OpenDecision::Migrate(db_version)
            }
        }
        OpenPolicy::Reject => {
            if db_version > app_version {
                OpenDecision::Reject(format!(
                    "database v{db_version} is newer than app v{app_version}"
                ))
            } else {
                OpenDecision::Migrate(db_version)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        open_strategy, BackupMode, BackupPolicy, Migration, MigrationContext, MigrationError,
        Migrator, OpenDecision, OpenPolicy,
    };

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

    #[test]
    fn migration_applies_in_order_and_is_idempotent() {
        let migrator = Migrator::new(vec![step(1, "s1"), step(2, "s2"), step(3, "s3")]);
        let mut ctx = MigrationContext::default();
        assert_eq!(migrator.migrate(&mut ctx, 3).unwrap(), 3);
        assert_eq!(ctx.applied, vec![1, 2, 3]);
        assert_eq!(ctx.schema, "v1;v2;v3;");
        // Idempotent: re-migrating to 3 is a no-op.
        assert_eq!(migrator.migrate(&mut ctx, 3).unwrap(), 3);
        assert_eq!(ctx.applied, vec![1, 2, 3]);
        // Partial migrate from an existing state.
        let mut ctx2 = MigrationContext {
            applied: vec![1],
            schema: "v1;".to_owned(),
            ..MigrationContext::default()
        };
        assert_eq!(migrator.migrate(&mut ctx2, 2).unwrap(), 2);
        assert_eq!(ctx2.applied, vec![1, 2]);
    }

    #[test]
    fn failing_step_rolls_back_transactionally() {
        let migrator = Migrator::new(vec![step(1, "s1"), failing_step(2, "s2"), step(3, "s3")]);
        let mut ctx = MigrationContext::default();
        let result = migrator.migrate(&mut ctx, 3);
        assert_eq!(result, Err(MigrationError::StepFailed("s2".to_owned())));
        // v1 stayed applied; v2 rolled back; v3 never ran.
        assert_eq!(ctx.applied, vec![1]);
        assert_eq!(ctx.schema, "v1;");
    }

    #[test]
    fn rollback_reverts_in_reverse() {
        let migrator = Migrator::new(vec![step(1, "s1"), step(2, "s2"), step(3, "s3")]);
        let mut ctx = MigrationContext::default();
        migrator.migrate(&mut ctx, 3).unwrap();
        assert_eq!(migrator.rollback(&mut ctx, 1).unwrap(), 1);
        assert_eq!(ctx.applied, vec![1]);
        assert_eq!(ctx.schema, "v1;");
    }

    #[test]
    fn rollback_missing_revert_fails() {
        let migrator = Migrator::new(vec![
            step(1, "s1"),
            Migration {
                version: 2,
                name: "no-revert".to_owned(),
                apply: Box::new(|ctx| {
                    ctx.schema.push_str("v2;");
                    Ok(())
                }),
                revert: None,
            },
        ]);
        let mut ctx = MigrationContext::default();
        migrator.migrate(&mut ctx, 2).unwrap();
        assert_eq!(
            migrator.rollback(&mut ctx, 1),
            Err(MigrationError::NoRevert(2))
        );
        assert_eq!(ctx.applied, vec![1, 2], "failed rollback keeps state");
    }

    #[test]
    fn backup_policy_sets_path_before_migration() {
        let migrator = Migrator::new(vec![step(1, "s1")]);
        let mut ctx = MigrationContext::default();
        let policy = BackupPolicy {
            mode: BackupMode::BeforeMigration,
            directory: "backups".to_owned(),
        };
        if policy.mode == BackupMode::BeforeMigration {
            ctx.backup_path = Some(format!("{}/pre-migration.db", policy.directory));
        }
        migrator.migrate(&mut ctx, 1).unwrap();
        assert_eq!(ctx.backup_path.as_deref(), Some("backups/pre-migration.db"));
    }

    #[test]
    fn open_strategy_handles_old_and_new_versions() {
        // Newer database + Reject policy -> reject.
        assert_eq!(
            open_strategy(3, 2, OpenPolicy::Reject),
            OpenDecision::Reject("database v3 is newer than app v2".to_owned())
        );
        // Newer database + ReadOnly policy -> read-only.
        assert_eq!(
            open_strategy(3, 2, OpenPolicy::ReadOnly),
            OpenDecision::OpenReadOnly
        );
        // Older or equal database -> migrate.
        assert_eq!(
            open_strategy(1, 2, OpenPolicy::Reject),
            OpenDecision::Migrate(1)
        );
        assert_eq!(
            open_strategy(2, 2, OpenPolicy::ReadOnly),
            OpenDecision::Migrate(2)
        );
        assert_eq!(
            open_strategy(1, 2, OpenPolicy::Upgrade),
            OpenDecision::Migrate(1)
        );
    }
}
