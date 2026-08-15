//! Update matrix (T166): upgrade / interrupt / tamper / rollback scenarios
//! against the real update coordinator, run 100 consecutive times and
//! printed as a stable report for `scripts/test-update-matrix.mjs`.

use update::{DigestVerifier, StagedRollout, UpdateCoordinator, UpdateManifest, Version};

/// Matrix repetitions.
const REPEATS: usize = 100;

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

/// Runs one full matrix pass; returns a deterministic outcome log.
fn run_matrix() -> Vec<(String, String)> {
    let mut log = Vec::new();
    let verifier = DigestVerifier;

    // 1. Upgrade: signed manifest applies.
    {
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.1.0"));
        let update = manifest("0.2.0", "0.1.0", 100, "abc123");
        let result = coordinator.apply(&update, &verifier, 42, true);
        log.push(("upgrade".to_owned(), format!("{result:?}")));
    }

    // 2. Interrupt: install fails mid-way; the version rolls back.
    {
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.1.0"));
        let update = manifest("0.2.0", "0.1.0", 100, "abc123");
        let result = coordinator.apply(&update, &verifier, 42, false);
        log.push((
            "interrupt".to_owned(),
            format!("{result:?} current={}", coordinator.current),
        ));
    }

    // 3. Tamper: stale signature over a changed digest is rejected.
    {
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.1.0"));
        let mut tampered = manifest("0.2.0", "0.1.0", 100, "abc123");
        tampered.sha256 = "evil".to_owned();
        let result = coordinator.apply(&tampered, &verifier, 42, true);
        log.push(("tamper".to_owned(), format!("{result:?}")));
    }

    // 4. Downgrade: an older target is rejected.
    {
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.2.0"));
        let older = manifest("0.1.0", "0.1.0", 100, "def456");
        let result = coordinator.apply(&older, &verifier, 42, true);
        log.push(("downgrade".to_owned(), format!("{result:?}")));
    }

    // 5. Staged rollout: 0% gates everyone; 100% offers everyone; 50% is
    //    a partial bucket.
    {
        let off = (0u64..1000)
            .filter(|id| StagedRollout::is_offered(*id, 0))
            .count();
        let all = (0u64..1000)
            .filter(|id| StagedRollout::is_offered(*id, 100))
            .count();
        let half = (0u64..1000)
            .filter(|id| StagedRollout::is_offered(*id, 50))
            .count();
        log.push((
            "staged".to_owned(),
            format!("off={off} all={all} half={half}"),
        ));
    }

    // 6. Rollback: after a successful upgrade, a later failure restores the
    //    last-known-good version.
    {
        let mut coordinator = UpdateCoordinator::new(Version::parse("0.1.0"));
        let update = manifest("0.2.0", "0.1.0", 100, "abc123");
        let _ = coordinator.apply(&update, &verifier, 42, true);
        let next = manifest("0.3.0", "0.2.0", 100, "next456");
        let result = coordinator.apply(&next, &verifier, 42, false);
        log.push((
            "rollback".to_owned(),
            format!("{result:?} current={}", coordinator.current),
        ));
    }

    log
}

fn main() {
    let canonical = run_matrix();
    let mut stable = true;
    for _ in 1..REPEATS {
        if run_matrix() != canonical {
            stable = false;
        }
    }
    println!(
        "UPDATE_MATRIX scenarios={} stable={stable}",
        canonical.len()
    );
    for (scenario, outcome) in &canonical {
        println!("UPDATE {scenario}={outcome}");
    }
    if !stable {
        std::process::exit(1);
    }
}
