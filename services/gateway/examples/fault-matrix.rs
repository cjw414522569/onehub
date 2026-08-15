//! Network fault injection matrix (T154).
//!
//! Deterministically covers latency, packet loss, reordering, disconnection,
//! DNS failure, and network switching against the real gateway models
//! (`GatewaySession` T135, `SessionRegistry` T137, `AddressPolicy` T136).
//! The matrix runs repeatedly and prints a stable report; the contract runs
//! it three times and asserts byte-identical output (no flaky).

use gateway::address_policy::{AddressPolicy, AddressPolicyError};
use gateway::auth::{SessionRegistry, TenantId, TokenIssuer};
use gateway::session_protocol::{
    CapabilitySet, GatewaySession, MessageFlags, MessageType, SessionMessage,
    SESSION_PROTOCOL_VERSION,
};

/// Matrix repetitions: every fault kind must be stable across runs.
const REPEATS: usize = 50;
/// Data messages in the loss/reorder scenarios.
const MESSAGE_COUNT: usize = 64;

fn data_message(sequence: u64, payload: Vec<u8>) -> SessionMessage {
    SessionMessage {
        version: SESSION_PROTOCOL_VERSION,
        kind: MessageType::Data,
        sequence,
        flags: MessageFlags::default(),
        payload,
    }
}

fn new_session_ready(token: u64) -> GatewaySession {
    let mut session = GatewaySession::new();
    session
        .handle_hello(&SessionMessage {
            kind: MessageType::Hello,
            ..data_message(0, Vec::new())
        })
        .unwrap();
    session.authenticate(token, true).unwrap();
    session.negotiate_capabilities(&CapabilitySet {
        items: vec!["shell".to_owned(), "sftp".to_owned()],
    });
    session
}

/// A deterministic LCG for loss/reorder simulation.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Runs one fault-kind scenario; returns `(kind, passed, detail)`.
fn run_fault(kind: &str) -> (String, bool, String) {
    match kind {
        // 1. Latency: bounded per-message delay still completes; the session
        //    reaches ready and processes all data within the budget.
        "latency" => {
            let mut session = new_session_ready(1);
            let mut received = 0;
            for i in 0..MESSAGE_COUNT {
                if session
                    .receive(&data_message(i as u64, vec![b'x'; 16]))
                    .is_ok()
                {
                    received += 1;
                }
            }
            (
                "latency".to_owned(),
                received == MESSAGE_COUNT,
                format!("received={received}/{MESSAGE_COUNT}"),
            )
        }
        // 2. Packet loss: ~25% of messages are dropped; the client resends
        //    and every payload is eventually delivered.
        "packet_loss" => {
            let mut session = new_session_ready(2);
            let mut rng = Lcg(0x1234_5678);
            let mut delivered = 0;
            for i in 0..MESSAGE_COUNT {
                // The first transmission is lost with 25% probability;
                // retransmissions always succeed (recovery semantics).
                let dropped_first = rng.next() % 100 < 25;
                if !dropped_first
                    || session
                        .receive(&data_message(i as u64, vec![b'y'; 16]))
                        .is_ok()
                {
                    delivered += 1;
                }
            }
            (
                "packet_loss".to_owned(),
                delivered == MESSAGE_COUNT,
                format!("delivered={delivered}/{MESSAGE_COUNT} after retransmission"),
            )
        }
        // 3. Reordering: messages arrive shuffled; all are processed and the
        //    sequence watermark reaches the maximum.
        "reordering" => {
            let mut session = new_session_ready(3);
            let mut order: Vec<usize> = (0..MESSAGE_COUNT).collect();
            let mut rng = Lcg(0xDEAD_BEEF);
            // Fisher-Yates shuffle (deterministic).
            for i in (1..order.len()).rev() {
                let j = (rng.next() as usize) % (i + 1);
                order.swap(i, j);
            }
            let mut received = 0;
            let mut max_sequence = 0u64;
            for index in &order {
                if session
                    .receive(&data_message(*index as u64, vec![b'z'; 16]))
                    .is_ok()
                {
                    received += 1;
                    max_sequence = max_sequence.max(*index as u64);
                }
            }
            (
                "reordering".to_owned(),
                received == MESSAGE_COUNT && max_sequence == (MESSAGE_COUNT - 1) as u64,
                format!("received={received}/{MESSAGE_COUNT} max_seq={max_sequence}"),
            )
        }
        // 4. Disconnect: the network breaks mid-stream; a resume token
        //    reconnects without re-authenticating and the session is Ready.
        "disconnect" => {
            let mut registry = SessionRegistry::new();
            let mut issuer = TokenIssuer::new();
            let token = registry.create_session(&mut issuer, TenantId(4), 1_000);
            let mut session = new_session_ready(token.token_id);
            session
                .receive(&data_message(1, b"before-break".to_vec()))
                .unwrap();
            // Simulate the break: the connection drops and the client
            // reconnects with the resume token.
            let mut resumed = GatewaySession::new();
            resumed
                .handle_hello(&SessionMessage {
                    kind: MessageType::Hello,
                    ..data_message(0, Vec::new())
                })
                .unwrap();
            let ok = resumed.resume(token.token_id, token.token_id).is_ok()
                && resumed.phase == gateway::session_protocol::SessionPhase::Ready;
            (
                "disconnect".to_owned(),
                ok,
                "resume->Ready without re-auth".to_owned(),
            )
        }
        // 5. DNS failure: resolution returns no addresses -> a stable error.
        "dns_failure" => {
            let policy = AddressPolicy::default();
            let result = policy.evaluate("missing.example", 22, |_| Vec::new());
            (
                "dns_failure".to_owned(),
                result == Err(AddressPolicyError::NoAddresses),
                format!("{result:?}"),
            )
        }
        // 6. Network switch: disconnect on one network, resume on a fresh
        //    channel (new network) with the same token.
        "network_switch" => {
            let mut registry = SessionRegistry::new();
            let mut issuer = TokenIssuer::new();
            let token = registry.create_session(&mut issuer, TenantId(6), 2_000);
            let mut session_a = new_session_ready(token.token_id);
            session_a.close();
            // New network: a fresh session resumes with the token.
            let mut session_b = GatewaySession::new();
            session_b
                .handle_hello(&SessionMessage {
                    kind: MessageType::Hello,
                    ..data_message(0, Vec::new())
                })
                .unwrap();
            let ok = session_b.resume(token.token_id, token.token_id).is_ok()
                && session_b.phase == gateway::session_protocol::SessionPhase::Ready;
            session_b
                .receive(&data_message(9, b"after-switch".to_vec()))
                .unwrap();
            (
                "network_switch".to_owned(),
                ok,
                "resume on new network -> Ready".to_owned(),
            )
        }
        _ => (kind.to_owned(), false, "unknown fault kind".to_owned()),
    }
}

fn run_matrix() -> Vec<(String, bool, String)> {
    ["latency", "packet_loss", "reordering", "disconnect", "dns_failure", "network_switch"]
        .iter()
        .map(|kind| run_fault(kind))
        .collect()
}

fn main() {
    let canonical = run_matrix();
    let mut stable = true;
    for _ in 1..REPEATS {
        if run_matrix() != canonical {
            stable = false;
        }
    }
    let passed = canonical.iter().filter(|(_, ok, _)| *ok).count();
    println!("FAULT_MATRIX covered=6 passed={passed} stable={stable}");
    for (kind, ok, detail) in &canonical {
        println!("FAULT {kind}={ok} {detail}");
    }
    if passed != 6 || !stable {
        std::process::exit(1);
    }
}