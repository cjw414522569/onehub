//! Deterministic fake gateway server (T152).
//!
//! Implements the versioned session protocol (T135) with the auth registry
//! (T137) and address policy (T136) as a fake server, runs the full
//! scenario suite — handshake, auth, capabilities, data, cancel,
//! backpressure, resume, error paths, address-policy denial, close — 100
//! consecutive times, and prints a stable summary. The contract runs this
//! example repeatedly and asserts the output is byte-identical (no flaky).

use gateway::address_policy::AddressPolicy;
use gateway::auth::{SessionRegistry, TenantId, TokenIssuer};
use gateway::session_protocol::{
    CapabilitySet, GatewaySession, MessageFlags, MessageType, SessionMessage,
    SESSION_PROTOCOL_VERSION,
};

/// How many consecutive scenario runs the harness performs.
const RUNS: usize = 100;

fn data_message(sequence: u64, payload: Vec<u8>) -> SessionMessage {
    SessionMessage {
        version: SESSION_PROTOCOL_VERSION,
        kind: MessageType::Data,
        sequence,
        flags: MessageFlags::default(),
        payload,
    }
}

/// One deterministic scenario through the fake server; returns the outcome
/// log. Every run must produce the identical log.
fn run_scenario() -> Vec<String> {
    let mut log = Vec::new();
    let mut registry = SessionRegistry::new();
    let mut issuer = TokenIssuer::new();
    let policy = AddressPolicy::default();
    let server_capabilities = CapabilitySet {
        items: vec!["shell".to_owned(), "sftp".to_owned(), "forward".to_owned()],
    };
    let now = 1_000_000u64;

    // Handshake: version check.
    let token = registry.create_session(&mut issuer, TenantId(1), now);
    let mut session = GatewaySession::new();
    session
        .handle_hello(&SessionMessage {
            kind: MessageType::Hello,
            ..data_message(0, Vec::new())
        })
        .unwrap();
    log.push("hello=accepted".to_owned());

    // Data before auth is refused (error path).
    log.push(format!(
        "data_before_auth={:?}",
        session
            .receive(&data_message(1, b"early".to_vec()))
            .err()
            .unwrap()
    ));

    // Auth with a valid token; bad token refused.
    session.authenticate(token.token_id, true).unwrap();
    log.push("auth=accepted".to_owned());
    let mut bad = GatewaySession::new();
    bad.handle_hello(&SessionMessage {
        kind: MessageType::Hello,
        ..data_message(0, Vec::new())
    })
    .unwrap();
    log.push(format!(
        "bad_token={:?}",
        bad.authenticate(token.token_id.wrapping_add(1), false)
            .err()
            .unwrap()
    ));

    // Capability negotiation -> ready.
    let negotiated = session.negotiate_capabilities(&server_capabilities);
    log.push(format!("negotiated={}", negotiated.join(",")));

    // Data flow with cancellation and backpressure flags.
    session
        .receive(&data_message(2, b"payload-1".to_vec()))
        .unwrap();
    session
        .receive(&SessionMessage {
            kind: MessageType::Backpressure,
            flags: MessageFlags {
                backpressure: true,
                cancel: false,
            },
            ..data_message(3, Vec::new())
        })
        .unwrap();
    log.push(format!("backpressure={}", session.backpressure));
    session
        .receive(&data_message(4, b"payload-2".to_vec()))
        .unwrap();
    session
        .receive(&SessionMessage {
            kind: MessageType::Cancel,
            ..data_message(5, Vec::new())
        })
        .unwrap();
    log.push("cancel=handled".to_owned());

    // Version mismatch rejected (error path).
    let mut stale = GatewaySession::new();
    let old_hello = SessionMessage {
        version: SESSION_PROTOCOL_VERSION - 1,
        kind: MessageType::Hello,
        ..data_message(0, Vec::new())
    };
    log.push(format!(
        "version_mismatch={:?}",
        stale.handle_hello(&old_hello).err().unwrap()
    ));

    // Resume after a simulated network failure; wrong token refused.
    let mut resumed = GatewaySession::new();
    resumed
        .handle_hello(&SessionMessage {
            kind: MessageType::Hello,
            ..data_message(0, Vec::new())
        })
        .unwrap();
    resumed.resume(token.token_id, token.token_id).unwrap();
    log.push(format!("resume_phase={:?}", resumed.phase));
    let mut bad_resume = GatewaySession::new();
    bad_resume
        .handle_hello(&SessionMessage {
            kind: MessageType::Hello,
            ..data_message(0, Vec::new())
        })
        .unwrap();
    log.push(format!(
        "bad_resume={:?}",
        bad_resume
            .resume(token.token_id.wrapping_add(1), token.token_id)
            .err()
            .unwrap()
    ));

    // Address-policy denial (SSRF): private target rejected deterministically.
    log.push(format!(
        "private_target={:?}",
        policy
            .evaluate("host", 22, |_| vec!["10.0.0.5".parse().unwrap()])
            .err()
            .unwrap()
    ));
    log.push(format!(
        "public_target={:?}",
        policy
            .evaluate("host", 22, |_| vec!["93.184.216.34".parse().unwrap()])
            .map(|target| target.ips.len())
    ));

    // Lifecycle: close terminates the session; the registry still isolates.
    session.close();
    log.push(format!("phase_after_close={:?}", session.phase));
    log.push(format!(
        "isolation={:?}",
        registry
            .access(TenantId(2), token.session_id)
            .err()
            .unwrap()
    ));
    log.push(format!(
        "tenant_ok={:?}",
        registry.access(TenantId(1), token.session_id).map(|_| "ok")
    ));

    log
}

fn main() {
    let canonical = run_scenario();
    let mut stable = true;
    for _run in 1..RUNS {
        if run_scenario() != canonical {
            stable = false;
        }
    }
    println!("HARNESS runs={RUNS} stable={stable}");
    for line in &canonical {
        println!("HARNESS {line}");
    }
    if !stable {
        std::process::exit(1);
    }
}
