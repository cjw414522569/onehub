#![cfg(test)]
//! Deterministic, bounded fuzz targets for SSH decoders, auth messages and
//! channel data (T061). Every target uses a fixed seed and a fixed iteration
//! budget; any panic, out-of-bounds access, livelock or unbounded memory
//! growth fails the test. Crash inputs must be persisted back into the corpus
//! for regression (see `fuzz/smoke-corpus.json` and `run-fuzz-smoke.ps1`).

/// Small deterministic xorshift64 PRNG (mirrored from crates/fuzz-targets).
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    pub fn next_u8(&mut self) -> u8 {
        self.next_u64() as u8
    }

    pub fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    pub fn fill(&mut self, buffer: &mut [u8]) {
        for byte in buffer.iter_mut() {
            *byte = self.next_u8();
        }
    }
}

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

mod tests {
    use super::{fake_bytes, fake_string, XorShift64};
    use crate::agent::{parse_frame, AGENT_MAX_MESSAGE_LEN};
    use crate::agent_forwarding::decode_agent_channel_open;
    use crate::channel_qos::{Scheduler, SchedulerConfig, TrafficClass};
    use crate::known_hosts::{hashed_host_matches, host_field_matches, hostname_matches_pattern};
    use crate::session_channel::{ExecCommand, PtyConfig};
    use base64::Engine as _;

    #[test]
    fn fuzz_agent_frame_parse() {
        let mut rng = XorShift64::new(21);
        for _ in 0..20_000 {
            let bytes = fake_bytes(&mut rng, 2 * AGENT_MAX_MESSAGE_LEN as usize);
            // Must never panic; errors and oversized frames are rejected.
            if let Ok(Some((payload, consumed))) = parse_frame(&bytes) {
                assert!(consumed <= bytes.len());
                assert!(payload.len() <= AGENT_MAX_MESSAGE_LEN as usize);
            }
        }
    }

    #[test]
    fn fuzz_agent_forwarding_channel_open() {
        let mut rng = XorShift64::new(22);
        for _ in 0..20_000 {
            let bytes = fake_bytes(&mut rng, 256);
            // Must never panic; malformed channel opens are rejected.
            let _ = decode_agent_channel_open(&bytes);
        }
    }

    #[test]
    fn fuzz_known_hosts_matching() {
        let mut rng = XorShift64::new(23);
        for _ in 0..20_000 {
            let field = fake_string(&mut rng, 96);
            let host = fake_string(&mut rng, 32);
            let port = (rng.next_u64() % 65536) as u16;
            // All three matchers must be total (never panic) and
            // deterministic.
            let a = host_field_matches(&field, &host, port);
            let b = host_field_matches(&field, &host, port);
            assert_eq!(a, b, "matching must be deterministic");
            let _ = hashed_host_matches(&field, &host);
            let _ = hostname_matches_pattern(&field, &host);
        }
    }

    #[test]
    fn fuzz_qos_scheduler_bounded() {
        // Random register/set_class/enqueue/window_adjust/drain sequences must
        // never panic, never livelock, and never grow queued memory
        // unboundedly.
        let mut rng = XorShift64::new(25);
        for _ in 0..5_000 {
            let config = SchedulerConfig::default();
            let mut scheduler = Scheduler::new(config).expect("scheduler");
            let mut enqueued: u64 = 0;
            let mut drained_total: u64 = 0;
            for _ in 0..256 {
                let op = rng.next_below(6);
                let id = (rng.next_below(8) + 1) as u32;
                match op {
                    0 => {
                        let class = match rng.next_below(3) {
                            0 => TrafficClass::Control,
                            1 => TrafficClass::Interactive,
                            _ => TrafficClass::Bulk,
                        };
                        let _ = scheduler.register(id, class);
                    }
                    1 => {
                        let _ = scheduler.unregister(id);
                    }
                    2 => {
                        let class = match rng.next_below(3) {
                            0 => TrafficClass::Control,
                            1 => TrafficClass::Interactive,
                            _ => TrafficClass::Bulk,
                        };
                        let _ = scheduler.set_class(id, class);
                    }
                    3 => {
                        if scheduler.enqueue(id, rng.next_below(64 * 1024) + 1).is_ok() {
                            enqueued += 1;
                        }
                    }
                    4 => {
                        let _ = scheduler.window_adjust(id, (rng.next_u64() % 1_000_000) as u32);
                    }
                    _ => {
                        let sends = scheduler.drain(None);
                        drained_total += sends.len() as u64;
                    }
                }
            }
            // Drain must terminate (bounded by round budget per call) and the
            // scheduler must never report more drained chunks than enqueued.
            let mut guard = 0;
            loop {
                let sends = scheduler.drain(None);
                if sends.is_empty() {
                    break;
                }
                drained_total += sends.len() as u64;
                guard += 1;
                assert!(guard < 100_000, "drain must terminate (livelock guard)");
            }
            assert!(drained_total <= enqueued + 256, "drained exceeds enqueued");
            for queued in scheduler.snapshot().channels.iter() {
                assert!(
                    queued.queued_bytes <= 64 * 1024 * 256,
                    "unbounded queue growth"
                );
            }
        }
    }

    #[test]
    fn fuzz_session_channel_validation() {
        let mut rng = XorShift64::new(26);
        for _ in 0..20_000 {
            let term = fake_string(&mut rng, 64);
            let command = fake_string(&mut rng, 128);
            // Validators must never panic; they return Result.
            let _ = PtyConfig::new(
                &term,
                (rng.next_u64() % 4096) as u16,
                (rng.next_u64() % 4096) as u16,
                0,
                0,
            );
            let _ = ExecCommand::new(&command, vec![]);
        }
    }

    #[test]
    fn fuzz_hashed_host_matching() {
        let mut rng = XorShift64::new(27);
        for _ in 0..20_000 {
            // Random |1|salt|hash| fields must never panic.
            let salt = fake_bytes(&mut rng, 24);
            let hash = fake_bytes(&mut rng, 24);
            let field = format!(
                "|1|{}|{}|",
                base64::engine::general_purpose::STANDARD.encode(&salt),
                base64::engine::general_purpose::STANDARD.encode(&hash)
            );
            let host = fake_string(&mut rng, 32);
            let _ = hashed_host_matches(&field, &host);
            let _ = host_field_matches(&field, &host, 22);
        }
    }
}
