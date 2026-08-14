#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # fuzz-targets
//!
//! Deterministic, bounded fuzz-style smoke targets for the core models.
//! Each target uses a fixed seed from `fuzz/smoke-corpus.json` and a fixed
//! iteration budget; any panic or invariant violation fails the test and the
//! failing seed/input must be persisted back into the corpus for regression.
//!
//! The crate is test infrastructure only and ships no runtime behavior.

/// A small deterministic xorshift64 PRNG (no external dependency).
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// Creates a PRNG with the given seed (0 is remapped).
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    /// Returns the next u64.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Returns the next u8.
    pub fn next_u8(&mut self) -> u8 {
        self.next_u64() as u8
    }

    /// Returns a value in `0..bound`.
    pub fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    /// Fills a byte buffer with pseudo-random bytes.
    pub fn fill(&mut self, buffer: &mut [u8]) {
        for byte in buffer.iter_mut() {
            *byte = self.next_u8();
        }
    }
}

/// Number of fuzz targets (matches `fuzz/smoke-corpus.json`).
pub const FUZZ_TARGET_COUNT: usize = 7;

#[cfg(test)]
mod tests {
    use super::XorShift64;
    use core_domain::{
        migrate_settings, resolve_command, CommandSnippet, EnvVar, Environment, ForwardingEndpoint,
        ForwardingFamily, ForwardingKind, ForwardingSpec, ForwardingTable, HostId, KnownHostsEntry,
        LocalSettings, Macro, PlaceholderDef, ProxyChain, ProxyHop, ProxyKind, SettingsDocument,
        SETTINGS_SCHEMA_VERSION,
    };
    use core_protocol::terminal::TerminalSnapshot;
    use session_orchestrator::{apply, SessionEvent, SessionState, SessionTransitionResult};

    fn fake_bytes(rng: &mut XorShift64, max_len: usize) -> Vec<u8> {
        let len = rng.next_below(max_len + 1);
        let mut buffer = vec![0u8; len];
        rng.fill(&mut buffer);
        buffer
    }

    fn fake_string(rng: &mut XorShift64, max_len: usize) -> String {
        let bytes = fake_bytes(rng, max_len);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn fuzz_known_hosts_parse() {
        let mut rng = XorShift64::new(11);
        for _ in 0..20_000 {
            let line = fake_string(&mut rng, 128);
            // Must never panic; parse returns Option.
            let _ = KnownHostsEntry::parse(&line);
        }
    }

    #[test]
    fn fuzz_terminal_snapshot_deserialize() {
        let mut rng = XorShift64::new(12);
        for _ in 0..20_000 {
            let bytes = fake_bytes(&mut rng, 512);
            // Arbitrary JSON must not panic; serde errors are returned.
            let _ = serde_json::from_slice::<TerminalSnapshot>(&bytes);
        }
    }

    #[test]
    fn fuzz_proxy_chain_validation() {
        let mut rng = XorShift64::new(13);
        for _ in 0..20_000 {
            let count = rng.next_below(6);
            let mut hops = Vec::new();
            for _ in 0..count {
                let host = HostId::new(format!("host-{}", rng.next_below(3))).expect("host id");
                let hop = match rng.next_below(3) {
                    0 => ProxyHop::new(ProxyKind::JumpHost { host_id: host }),
                    1 => ProxyHop::new(ProxyKind::Socks5 { host_id: host }),
                    _ => ProxyHop::new(ProxyKind::HttpConnect { host_id: host }),
                };
                hops.push(hop);
            }
            let chain = ProxyChain::from_hops(hops);
            let first = chain.validate();
            let second = chain.validate();
            assert_eq!(first, second, "validation must be deterministic");
        }
    }

    #[test]
    fn fuzz_session_state_machine() {
        let mut rng = XorShift64::new(14);
        for _ in 0..20_000 {
            let mut state = SessionState::Disconnected;
            for _ in 0..64 {
                let event = match rng.next_below(6) {
                    0 => SessionEvent::ConnectRequested,
                    1 => SessionEvent::Connected,
                    2 => SessionEvent::Authenticated,
                    3 => SessionEvent::Disconnected,
                    4 => SessionEvent::CloseRequested,
                    _ => SessionEvent::Closed,
                };
                match apply(state, event) {
                    SessionTransitionResult::Accepted(transition) => state = transition.state,
                    SessionTransitionResult::Rejected { .. } => {}
                }
            }
            if state == SessionState::Closed {
                for _ in 0..8 {
                    let event = match rng.next_below(6) {
                        0 => SessionEvent::ConnectRequested,
                        1 => SessionEvent::Connected,
                        2 => SessionEvent::Authenticated,
                        3 => SessionEvent::Disconnected,
                        4 => SessionEvent::CloseRequested,
                        _ => SessionEvent::Closed,
                    };
                    assert!(
                        apply(state, event).is_rejected(),
                        "Closed must reject every event"
                    );
                }
            }
        }
    }

    #[test]
    fn fuzz_forwarding_table() {
        let mut rng = XorShift64::new(15);
        for _ in 0..20_000 {
            let mut table = ForwardingTable::new();
            for _ in 0..16 {
                let host = fake_string(&mut rng, 16);
                let port = (rng.next_u64() % 65536) as u16;
                let Ok(listen) = ForwardingEndpoint::new(&host, port) else {
                    continue;
                };
                let Ok(target) = ForwardingEndpoint::new("t.internal", 22) else {
                    continue;
                };
                let Ok(spec) = ForwardingSpec::new(
                    ForwardingKind::Local,
                    listen,
                    Some(target),
                    ForwardingFamily::Any,
                ) else {
                    continue;
                };
                table.add(spec);
            }
            let _ = table.entries().len();
        }
    }

    #[test]
    fn fuzz_settings_migration() {
        let mut rng = XorShift64::new(16);
        for _ in 0..20_000 {
            let doc = SettingsDocument {
                schema_version: (rng.next_u64() % 4) as u32,
                settings: LocalSettings::default(),
            };
            let first = migrate_settings(doc);
            let second = migrate_settings(first.clone());
            assert_eq!(first, second, "migration must be idempotent");
            assert_eq!(first.schema_version, SETTINGS_SCHEMA_VERSION);
        }
    }

    #[test]
    fn fuzz_command_resolution() {
        let mut rng = XorShift64::new(17);
        for _ in 0..20_000 {
            let snippet = CommandSnippet::new(
                "fuzz",
                "fuzz",
                "ssh {user}@{host} $TOKEN",
                vec![
                    PlaceholderDef {
                        name: "user".to_owned(),
                        sensitive: rng.next_u8().is_multiple_of(2),
                    },
                    PlaceholderDef {
                        name: "host".to_owned(),
                        sensitive: false,
                    },
                ],
            )
            .expect("snippet");
            let env = Environment::from_vars(vec![EnvVar::new(
                "TOKEN",
                fake_string(&mut rng, 16),
                rng.next_u8().is_multiple_of(2),
            )
            .expect("var")]);
            let command = resolve_command(
                &snippet,
                &[
                    ("user".to_owned(), fake_string(&mut rng, 8)),
                    ("host".to_owned(), "h".to_owned()),
                ],
                &env,
                &[Macro {
                    name: "TOKEN".to_owned(),
                    expansion: "x".to_owned(),
                    sensitive: false,
                }],
            );
            assert_eq!(command.history_allowed(), !command.sensitive);
            assert_eq!(command.telemetry_allowed(), !command.sensitive);
        }
    }
}
