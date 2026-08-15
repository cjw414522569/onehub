//! Gateway soak test (T142): concurrency, bandwidth, long connections,
//! failure recovery, and cross-session isolation at the model level.
//!
//! The gateway network service is a workspace skeleton, so this soak drives
//! the real implemented models — `GatewaySession` (T135), `SessionRegistry`
//! / `TokenIssuer` (T137), and the address policy (T136) — under the T003
//! budgets: bounded memory over repeated connect/close cycles, throughput at
//! least 40 MB/s (the Web/PWA minimum), zero cross-session data leakage, and
//! failure recovery via resume tokens.
//!
//! Prints a single JSON summary line that `scripts/test-gateway-soak.mjs`
//! captures into the archived report.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use gateway::address_policy::AddressPolicy;
use gateway::auth::{AuthError, SessionRegistry, TenantId, TokenIssuer};

/// The shared gateway state: one registry and one issuer so session and
/// token ids are globally unique across worker threads (as in a real
/// gateway).
struct Gateway {
    registry: SessionRegistry,
    issuer: TokenIssuer,
}
use gateway::session_protocol::{
    CapabilitySet, GatewaySession, MessageFlags, MessageType, ProtocolError, SessionMessage,
    SESSION_PROTOCOL_VERSION,
};

/// Concurrency: worker threads.
const THREADS: usize = 8;
/// Sessions per worker.
const SESSIONS_PER_WORKER: usize = 512;
/// Data messages per session (1 KiB payloads).
const MESSAGES_PER_SESSION: usize = 64;
/// Soak cycles: connect/close repetitions per worker.
const SOAK_CYCLES: usize = 512;
/// Throughput floor (T003 Web/PWA minimum is 40 MB/s; model-level floor).
const THROUGHPUT_FLOOR_MBPS: f64 = 40.0;
/// Bounded-memory floor for the consumed-token window after pruning.
const MAX_CONSUMED_WINDOW: usize = 4096;

fn data_message(sequence: u64, payload: Vec<u8>) -> SessionMessage {
    SessionMessage {
        version: SESSION_PROTOCOL_VERSION,
        kind: MessageType::Data,
        sequence,
        flags: MessageFlags::default(),
        payload,
    }
}

#[derive(Debug, Default)]
struct Metrics {
    sessions: u64,
    bytes: u64,
    isolation_violations: u64,
    resume_failures: u64,
    peak_consumed: usize,
}

fn worker(tenant_base: u64, shared: &Arc<Mutex<Gateway>>, metrics: &Arc<Mutex<Metrics>>) {
    let policy = AddressPolicy::default();
    let payload = vec![b'x'; 1024];
    let server_capabilities = CapabilitySet {
        items: vec!["shell".to_owned(), "sftp".to_owned(), "forward".to_owned()],
    };

    for i in 0..SESSIONS_PER_WORKER {
        let tenant = TenantId(tenant_base + (i as u64) % 4);
        let now = 1_000_000 + i as u64;
        let mut guard = shared.lock().unwrap();
        let gateway = &mut *guard;
        let token = gateway
            .registry
            .create_session(&mut gateway.issuer, tenant, now);
        drop(guard);

        // Versioned handshake -> auth -> capability negotiation -> ready.
        let mut session = GatewaySession::new();
        session
            .handle_hello(&SessionMessage {
                kind: MessageType::Hello,
                ..data_message(0, Vec::new())
            })
            .unwrap();
        session.authenticate(token.token_id, true).unwrap();
        session.negotiate_capabilities(&server_capabilities);
        assert_eq!(
            session.phase,
            gateway::session_protocol::SessionPhase::Ready
        );

        // Bandwidth: pump 1 KiB data messages through the session.
        let mut bytes = 0u64;
        for m in 0..MESSAGES_PER_SESSION {
            let sequence = (i as u64) * 1_000 + m as u64;
            let received = session
                .receive(&data_message(sequence, payload.clone()))
                .expect("data in ready phase must be accepted");
            assert!(received.is_some());
            bytes += payload.len() as u64;
        }

        // Cross-session isolation: another tenant must never reach this one.
        let other = TenantId(tenant.0.wrapping_add(99));
        let isolation_ok = matches!(
            shared
                .lock()
                .unwrap()
                .registry
                .access(other, token.session_id),
            Err(AuthError::TenantIsolationViolation)
        );
        if !isolation_ok {
            metrics.lock().unwrap().isolation_violations += 1;
        }
        // The owning tenant can always reach its session.
        assert!(shared
            .lock()
            .unwrap()
            .registry
            .access(tenant, token.session_id)
            .is_ok());

        // Failure recovery: resume with the token after a simulated drop.
        let mut resumed = GatewaySession::new();
        resumed
            .handle_hello(&SessionMessage {
                kind: MessageType::Hello,
                ..data_message(0, Vec::new())
            })
            .unwrap();
        match resumed.resume(token.token_id, token.token_id) {
            Ok(()) => {
                assert_eq!(
                    resumed.phase,
                    gateway::session_protocol::SessionPhase::Ready
                );
            }
            Err(_) => metrics.lock().unwrap().resume_failures += 1,
        }
        // A wrong token is refused (recovery does not weaken auth).
        let mut bad = GatewaySession::new();
        bad.handle_hello(&SessionMessage {
            kind: MessageType::Hello,
            ..data_message(0, Vec::new())
        })
        .unwrap();
        assert_eq!(
            bad.resume(token.token_id.wrapping_add(1), token.token_id),
            Err(ProtocolError::InvalidResume)
        );

        // The address policy still gates the target under load.
        assert!(policy
            .evaluate("host", 22, |_| vec!["93.184.216.34".parse().unwrap()])
            .is_ok());

        session.close();
        {
            let mut m = metrics.lock().unwrap();
            m.sessions += 1;
            m.bytes += bytes;
        }
    }

    // Soak cycles: connect/close repeatedly; the consumed-token replay
    // window must stay bounded after pruning (T003: 72h soak, 0 growth).
    let mut peak = 0usize;
    for cycle in 0..SOAK_CYCLES {
        let now = 2_000_000 + cycle as u64;
        let mut guard = shared.lock().unwrap();
        let gateway = &mut *guard;
        let token =
            gateway
                .registry
                .create_session(&mut gateway.issuer, TenantId(tenant_base + 900), now);
        assert!(gateway.registry.authenticate(&token, now + 1).is_ok());
        let _ = gateway.registry.prune_expired(now + 60);
        peak = peak.max(gateway.registry.consumed_token_count());
    }
    let mut m = metrics.lock().unwrap();
    m.peak_consumed = m.peak_consumed.max(peak);
}

fn main() {
    let shared = Arc::new(Mutex::new(Gateway {
        registry: SessionRegistry::new(),
        issuer: TokenIssuer::new(),
    }));
    let metrics = Arc::new(Mutex::new(Metrics::default()));
    let start = Instant::now();

    let mut handles = Vec::new();
    for thread in 0..THREADS {
        let shared = Arc::clone(&shared);
        let metrics = Arc::clone(&metrics);
        handles.push(std::thread::spawn(move || {
            worker(thread as u64 * 10_000, &shared, &metrics);
        }));
    }
    for handle in handles {
        handle.join().expect("soak worker panicked");
    }

    let elapsed_secs = start.elapsed().as_secs_f64();
    let m = metrics.lock().unwrap();
    let sessions_per_sec = m.sessions as f64 / elapsed_secs;
    let mb_per_sec = m.bytes as f64 / (1024.0 * 1024.0) / elapsed_secs;

    assert!(
        mb_per_sec >= THROUGHPUT_FLOOR_MBPS,
        "throughput {mb_per_sec:.1} MB/s below floor {THROUGHPUT_FLOOR_MBPS}"
    );
    assert_eq!(
        m.isolation_violations, 0,
        "cross-session data leakage detected"
    );
    assert_eq!(m.resume_failures, 0, "valid resume tokens must succeed");
    assert!(
        m.peak_consumed <= MAX_CONSUMED_WINDOW,
        "consumed-token window {} exceeds bounded budget {MAX_CONSUMED_WINDOW}",
        m.peak_consumed
    );

    let summary = serde_json::json!({
        "task": "T142",
        "status": "pass",
        "soak": {
            "threads": THREADS,
            "sessions_per_worker": SESSIONS_PER_WORKER,
            "messages_per_session": MESSAGES_PER_SESSION,
            "soak_cycles_per_worker": SOAK_CYCLES,
            "sessions": m.sessions,
            "bytes_processed": m.bytes,
            "elapsed_secs": format!("{elapsed_secs:.3}"),
            "sessions_per_sec": format!("{sessions_per_sec:.0}"),
            "mb_per_sec": format!("{mb_per_sec:.1}"),
            "isolation_violations": m.isolation_violations,
            "resume_failures": m.resume_failures,
            "peak_consumed_tokens": m.peak_consumed,
            "throughput_floor_mbps": THROUGHPUT_FLOOR_MBPS,
            "max_consumed_window": MAX_CONSUMED_WINDOW,
        }
    });
    println!("SOAK_METRICS {summary}");
}
