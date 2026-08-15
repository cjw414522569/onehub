//! Mobile background-transfer system-allowed paths and user prompts (T131).
//!
//! Android and iOS expose different system-allowed background paths:
//! Android uses a foreground service for active sessions and WorkManager for
//! deferred work; iOS uses BGTaskScheduler and URLSession background
//! transfers. [`PlatformPaths`] makes the difference explicit, background
//! transfers always require a user-visible prompt, and
//! [`InterruptionRecovery`] resumes an interrupted transfer from a
//! checkpoint (no data lost). Real background time-limit and
//! system-termination tests run on mobile devices.

/// The mobile platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobilePlatform {
    /// Android.
    Android,
    /// iOS.
    Ios,
}

/// A system-allowed background path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundPath {
    /// Android foreground service (active sessions).
    ForegroundService,
    /// Android WorkManager (deferred / resumable work).
    WorkManager,
    /// iOS BGTaskScheduler (deferred work).
    BgTaskScheduler,
    /// iOS URLSession background transfer.
    BgUrlSession,
}

/// The platform's allowed background paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPaths {
    /// The platform.
    pub platform: MobilePlatform,
    /// The allowed paths.
    pub paths: Vec<BackgroundPath>,
}

impl PlatformPaths {
    /// Android: foreground service for active sessions, WorkManager for
    /// deferred work.
    pub fn android() -> Self {
        Self {
            platform: MobilePlatform::Android,
            paths: vec![
                BackgroundPath::ForegroundService,
                BackgroundPath::WorkManager,
            ],
        }
    }

    /// iOS: BGTaskScheduler for deferred work, URLSession background
    /// transfers for transfers.
    pub fn ios() -> Self {
        Self {
            platform: MobilePlatform::Ios,
            paths: vec![
                BackgroundPath::BgTaskScheduler,
                BackgroundPath::BgUrlSession,
            ],
        }
    }

    /// A human summary of the platform difference (visible to the user).
    pub fn difference_summary(&self) -> String {
        match self.platform {
            MobilePlatform::Android => {
                "Android keeps active sessions alive with a foreground service; deferred work runs via WorkManager.".to_owned()
            }
            MobilePlatform::Ios => {
                "iOS defers work to BGTaskScheduler and transfers to URLSession background transfers; the app is otherwise suspended.".to_owned()
            }
        }
    }
}

/// The background-transfer policy for a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundTransferPolicy {
    /// The platform.
    pub platform: MobilePlatform,
    /// Whether a user-visible prompt is required before a background
    /// transfer starts.
    pub prompt_required: bool,
    /// Whether the background window is time-limited by the system.
    pub time_limited: bool,
}

impl BackgroundTransferPolicy {
    /// The policy for a platform: both require a prompt and are time-limited.
    pub fn for_platform(platform: MobilePlatform) -> Self {
        Self {
            platform,
            prompt_required: true,
            time_limited: true,
        }
    }
}

/// Interruption recovery from a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptionRecovery {
    /// Bytes completed before the interruption.
    pub bytes_completed: u64,
    /// Total bytes.
    pub total: u64,
    /// The checkpoint (bytes durably written).
    pub checkpoint: u64,
}

impl InterruptionRecovery {
    /// The byte offset to resume from (the checkpoint; never past the bytes
    /// actually completed).
    pub fn resume_from(&self) -> u64 {
        self.checkpoint.min(self.bytes_completed)
    }

    /// Whether the transfer can be recovered without data loss.
    pub fn recovered(&self) -> bool {
        self.resume_from() <= self.total && self.checkpoint <= self.bytes_completed
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BackgroundPath, BackgroundTransferPolicy, InterruptionRecovery, MobilePlatform,
        PlatformPaths,
    };

    #[test]
    fn android_ios_background_paths_are_explicit_and_different() {
        let android = PlatformPaths::android();
        let ios = PlatformPaths::ios();
        assert_ne!(
            android.paths, ios.paths,
            "Android and iOS differ explicitly"
        );
        assert!(android.paths.contains(&BackgroundPath::ForegroundService));
        assert!(android.paths.contains(&BackgroundPath::WorkManager));
        assert!(ios.paths.contains(&BackgroundPath::BgTaskScheduler));
        assert!(ios.paths.contains(&BackgroundPath::BgUrlSession));
        assert!(!android.difference_summary().is_empty());
        assert!(!ios.difference_summary().is_empty());
        assert_ne!(android.difference_summary(), ios.difference_summary());
    }

    #[test]
    fn background_transfers_require_a_prompt_and_are_time_limited() {
        for platform in [MobilePlatform::Android, MobilePlatform::Ios] {
            let policy = BackgroundTransferPolicy::for_platform(platform);
            assert!(
                policy.prompt_required,
                "background transfers need a user prompt"
            );
            assert!(
                policy.time_limited,
                "background windows are system time-limited"
            );
        }
    }

    #[test]
    fn interruption_recovery_resumes_from_checkpoint() {
        // A transfer at 5000 bytes with a checkpoint at 4000 resumes at 4000.
        let recovery = InterruptionRecovery {
            bytes_completed: 5000,
            total: 10_000,
            checkpoint: 4000,
        };
        assert_eq!(recovery.resume_from(), 4000);
        assert!(recovery.recovered());
        // A checkpoint never exceeds the bytes actually completed.
        let bad = InterruptionRecovery {
            bytes_completed: 1000,
            total: 10_000,
            checkpoint: 5000,
        };
        assert_eq!(bad.resume_from(), 1000);
        assert!(!bad.recovered());
    }
}
