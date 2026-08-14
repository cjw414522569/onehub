use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A stable protocol capability identifier.
///
/// String values are frozen and match `protocol/schema/domain-v1.json`
/// `capability_negotiation.feature_ids` exactly; new capabilities are
/// appended, never renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    #[serde(rename = "terminal.delta.v1")]
    TerminalDeltaV1,
    #[serde(rename = "terminal.snapshot.v1")]
    TerminalSnapshotV1,
    #[serde(rename = "session.cancel.v1")]
    SessionCancelV1,
    #[serde(rename = "flow.window.v1")]
    FlowWindowV1,
    #[serde(rename = "error.structured.v1")]
    ErrorStructuredV1,
    #[serde(rename = "sftp.transfer.v1")]
    SftpTransferV1,
    #[serde(rename = "forwarding.v1")]
    ForwardingV1,
}

impl Capability {
    /// Returns the frozen stable feature id.
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::TerminalDeltaV1 => "terminal.delta.v1",
            Capability::TerminalSnapshotV1 => "terminal.snapshot.v1",
            Capability::SessionCancelV1 => "session.cancel.v1",
            Capability::FlowWindowV1 => "flow.window.v1",
            Capability::ErrorStructuredV1 => "error.structured.v1",
            Capability::SftpTransferV1 => "sftp.transfer.v1",
            Capability::ForwardingV1 => "forwarding.v1",
        }
    }
}

/// All capabilities defined by schema v1.
pub const ALL_CAPABILITIES: [Capability; 7] = [
    Capability::TerminalDeltaV1,
    Capability::TerminalSnapshotV1,
    Capability::SessionCancelV1,
    Capability::FlowWindowV1,
    Capability::ErrorStructuredV1,
    Capability::SftpTransferV1,
    Capability::ForwardingV1,
];

/// An immutable set of capabilities, serialized as a sorted array of feature
/// ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet {
    inner: BTreeSet<Capability>,
}

impl CapabilitySet {
    /// An empty capability set.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Builds a set from a slice of capabilities.
    pub fn of(capabilities: &[Capability]) -> Self {
        Self {
            inner: capabilities.iter().copied().collect(),
        }
    }

    /// Inserts a capability; returns whether it was newly added.
    pub fn insert(&mut self, capability: Capability) -> bool {
        self.inner.insert(capability)
    }

    /// Removes a capability; returns whether it was present.
    pub fn remove(&mut self, capability: Capability) -> bool {
        self.inner.remove(&capability)
    }

    /// Returns whether the set contains the capability.
    pub fn contains(&self, capability: Capability) -> bool {
        self.inner.contains(&capability)
    }

    /// Iterates capabilities in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.inner.iter().copied()
    }

    /// Returns the number of capabilities.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns the union of two sets.
    pub fn union(&self, other: &Self) -> Self {
        Self {
            inner: self.inner.union(&other.inner).copied().collect(),
        }
    }

    /// Returns the intersection of two sets.
    pub fn intersection(&self, other: &Self) -> Self {
        Self {
            inner: self.inner.intersection(&other.inner).copied().collect(),
        }
    }

    /// Returns the difference `self \ other`.
    pub fn difference(&self, other: &Self) -> Self {
        Self {
            inner: self.inner.difference(&other.inner).copied().collect(),
        }
    }

    /// Returns whether every capability in `other` is also in `self`.
    pub fn is_superset_of(&self, other: &Self) -> bool {
        other.inner.is_subset(&self.inner)
    }

    /// Returns whether every capability in `self` is also in `other`.
    pub fn is_subset_of(&self, other: &Self) -> bool {
        self.inner.is_subset(&other.inner)
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

/// Result of negotiating requested capabilities against what is available.
///
/// Implements the schema's `intersection-with-explicit-rejection`: selected is
/// the intersection; rejected lists every requested capability that is not
/// available so callers (e.g. the UI) can hide those features explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegotiationResult {
    selected: CapabilitySet,
    rejected: CapabilitySet,
}

impl NegotiationResult {
    /// Returns the capabilities that are both requested and available.
    pub fn selected(&self) -> &CapabilitySet {
        &self.selected
    }

    /// Returns the requested capabilities that are not available.
    pub fn rejected(&self) -> &CapabilitySet {
        &self.rejected
    }

    /// Returns whether any requested capability was rejected.
    pub fn has_rejections(&self) -> bool {
        !self.rejected.is_empty()
    }
}

/// Negotiates requested capabilities against the available set.
///
/// The UI must only surface capabilities in [`NegotiationResult::selected`];
/// rejected capabilities are explicitly hidden, never silently assumed.
pub fn negotiate(requested: &CapabilitySet, available: &CapabilitySet) -> NegotiationResult {
    NegotiationResult {
        selected: requested.intersection(available),
        rejected: requested.difference(available),
    }
}

/// A platform identity used for capability profiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlatformId {
    Windows,
    MacOS,
    Linux,
    Ios,
    Android,
    Web,
    Cli,
}

impl PlatformId {
    /// Returns the stable platform string.
    pub const fn as_str(self) -> &'static str {
        match self {
            PlatformId::Windows => "windows",
            PlatformId::MacOS => "macos",
            PlatformId::Linux => "linux",
            PlatformId::Ios => "ios",
            PlatformId::Android => "android",
            PlatformId::Web => "web",
            PlatformId::Cli => "cli",
        }
    }
}

/// Runtime capability profile of a platform.
///
/// The profile is what the platform reports it can actually support in the
/// current runtime; the UI renders a feature only if it survives negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformProfile {
    platform: PlatformId,
    capabilities: CapabilitySet,
}

impl PlatformProfile {
    /// Creates a profile.
    pub fn new(platform: PlatformId, capabilities: CapabilitySet) -> Self {
        Self {
            platform,
            capabilities,
        }
    }

    /// Returns the platform.
    pub fn platform(&self) -> PlatformId {
        self.platform
    }

    /// Returns the runtime-confirmed capabilities.
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Desktop platforms (Windows/macOS/Linux) support the full feature set.
    pub fn desktop_default() -> Self {
        Self::new(PlatformId::Windows, CapabilitySet::of(&ALL_CAPABILITIES))
    }

    /// Mobile platforms (iOS/Android) exclude port forwarding.
    pub fn mobile_default() -> Self {
        Self::new(
            PlatformId::Android,
            CapabilitySet::of(&[
                Capability::TerminalDeltaV1,
                Capability::TerminalSnapshotV1,
                Capability::SessionCancelV1,
                Capability::FlowWindowV1,
                Capability::ErrorStructuredV1,
                Capability::SftpTransferV1,
            ]),
        )
    }

    /// Web/PWA supports the full feature set via the gateway.
    pub fn web_default() -> Self {
        Self::new(PlatformId::Web, CapabilitySet::of(&ALL_CAPABILITIES))
    }

    /// CLI supports the full feature set.
    pub fn cli_default() -> Self {
        Self::new(PlatformId::Cli, CapabilitySet::of(&ALL_CAPABILITIES))
    }

    /// Returns the default profile for a platform id.
    pub fn for_platform(platform: PlatformId) -> Self {
        match platform {
            PlatformId::Windows | PlatformId::MacOS | PlatformId::Linux => Self::desktop_default(),
            PlatformId::Ios | PlatformId::Android => Self::mobile_default(),
            PlatformId::Web => Self::web_default(),
            PlatformId::Cli => Self::cli_default(),
        }
    }
}

/// Negotiates requested capabilities against a platform profile.
pub fn negotiate_with_platform(
    requested: &CapabilitySet,
    profile: &PlatformProfile,
) -> NegotiationResult {
    negotiate(requested, profile.capabilities())
}

#[cfg(test)]
mod tests {
    use super::{
        negotiate, negotiate_with_platform, Capability, CapabilitySet, NegotiationResult,
        PlatformId, PlatformProfile, ALL_CAPABILITIES,
    };

    fn subsets(universe: &[Capability]) -> Vec<CapabilitySet> {
        let mut result = Vec::new();
        let count = universe.len();
        for mask in 0..(1usize << count) {
            let mut set = CapabilitySet::empty();
            for (index, capability) in universe.iter().enumerate() {
                if mask & (1 << index) != 0 {
                    set.insert(*capability);
                }
            }
            result.push(set);
        }
        result
    }

    #[test]
    fn schema_feature_ids_are_stable_and_covered() {
        let expected = [
            "terminal.delta.v1",
            "terminal.snapshot.v1",
            "session.cancel.v1",
            "flow.window.v1",
            "error.structured.v1",
            "sftp.transfer.v1",
            "forwarding.v1",
        ];
        let actual: Vec<&str> = ALL_CAPABILITIES.iter().map(|c| c.as_str()).collect();
        assert_eq!(actual, expected);
        assert_eq!(ALL_CAPABILITIES.len(), expected.len());
    }

    #[test]
    fn serde_uses_schema_feature_ids() {
        let set = CapabilitySet::of(&[Capability::TerminalDeltaV1, Capability::ForwardingV1]);
        let json = serde_json::to_string(&set).expect("serialize");
        assert_eq!(json, r#"["terminal.delta.v1","forwarding.v1"]"#);
        let decoded: CapabilitySet = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, set);
    }

    #[test]
    fn intersection_property_holds_for_all_subset_combinations() {
        let universe = [
            Capability::TerminalDeltaV1,
            Capability::TerminalSnapshotV1,
            Capability::SessionCancelV1,
            Capability::FlowWindowV1,
        ];
        let all_subsets = subsets(&universe);
        for requested in &all_subsets {
            for available in &all_subsets {
                let result = negotiate(requested, available);
                assert_eq!(
                    result.selected(),
                    &requested.intersection(available),
                    "selected must equal requested ∩ available"
                );
                assert_eq!(
                    result.rejected(),
                    &requested.difference(available),
                    "rejected must equal requested \\ available"
                );
                assert_eq!(
                    result.selected().union(result.rejected()),
                    *requested,
                    "selected ∪ rejected must equal requested"
                );
                assert_eq!(
                    result.selected().intersection(result.rejected()),
                    CapabilitySet::empty(),
                    "selected and rejected must be disjoint"
                );
                assert_eq!(
                    result.has_rejections(),
                    !requested.is_subset_of(available),
                    "rejections exist iff requested is not a subset of available"
                );
            }
        }
    }

    #[test]
    fn negotiation_is_idempotent_and_monotonic() {
        let available = CapabilitySet::of(&[
            Capability::TerminalDeltaV1,
            Capability::SftpTransferV1,
            Capability::ForwardingV1,
        ]);
        let requested = CapabilitySet::of(&[
            Capability::TerminalDeltaV1,
            Capability::SessionCancelV1,
            Capability::ForwardingV1,
        ]);
        let first = negotiate(&requested, &available);
        let second = negotiate(first.selected(), &available);
        assert_eq!(first.selected(), second.selected());

        let smaller = CapabilitySet::of(&[Capability::TerminalDeltaV1]);
        let bigger = CapabilitySet::of(&[Capability::TerminalDeltaV1, Capability::ForwardingV1]);
        let small_result = negotiate(&smaller, &available);
        let big_result = negotiate(&bigger, &available);
        assert!(small_result.selected().is_subset_of(big_result.selected()));
    }

    #[test]
    fn platform_profiles_differ_and_negotiation_respects_them() {
        let desktop = PlatformProfile::for_platform(PlatformId::Windows);
        assert_eq!(desktop.capabilities().len(), ALL_CAPABILITIES.len());

        let mobile = PlatformProfile::for_platform(PlatformId::Android);
        assert!(!mobile.capabilities().contains(Capability::ForwardingV1));
        assert!(mobile.capabilities().contains(Capability::TerminalDeltaV1));

        let web = PlatformProfile::for_platform(PlatformId::Web);
        assert_eq!(web.capabilities().len(), ALL_CAPABILITIES.len());

        let requested_all = CapabilitySet::of(&ALL_CAPABILITIES);
        let mobile_result = negotiate_with_platform(&requested_all, &mobile);
        assert!(mobile_result.has_rejections());
        assert_eq!(
            mobile_result.rejected(),
            &CapabilitySet::of(&[Capability::ForwardingV1])
        );
        assert!(!mobile_result.selected().contains(Capability::ForwardingV1));

        let desktop_result = negotiate_with_platform(&requested_all, &desktop);
        assert!(!desktop_result.has_rejections());
        assert_eq!(desktop_result.selected(), &requested_all);
    }

    #[test]
    fn negotiation_result_serde_round_trip() {
        let result = negotiate(
            &CapabilitySet::of(&[Capability::TerminalDeltaV1, Capability::ForwardingV1]),
            &CapabilitySet::of(&[Capability::TerminalDeltaV1]),
        );
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: NegotiationResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, result);
        assert!(decoded.has_rejections());
    }
}
