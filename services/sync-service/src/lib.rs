#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

//! # sync-service
//!
//! The minimal trusted backend for the self-hosted sync service (T094).
//!
//! The backend is *ciphertext-only*: it stores [`SyncEnvelope`]s (versioned
//! AEAD ciphertext plus routing metadata) and never sees or stores plaintext.
//! Every device has a storage **quota** (bytes of ciphertext it uploads) and a
//! per-device **rate limit** (token bucket). The **audit log** is
//! content-free: it records only device id, envelope id, action, byte length,
//! and a timestamp — never envelope contents.
//!
//! Delivery model: a `put` stores the envelope in the sender's *and* the
//! recipient's mailboxes (quota is charged to the sender once). A device may
//! only write envelopes it sends (sender id must match) and can only ever see
//! envelopes addressed to it, so unauthorized devices structurally never get
//! the data.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use storage_sqlite::{AtomicStore, ConfigRepository};
use sync_core::{SyncEnvelope, SYNC_PROTOCOL_VERSION};

/// Module identity used by diagnostics and architecture tooling.
pub const MODULE_ID: &str = "sync-service";

/// A clock abstraction so rate limiting is deterministic in tests.
pub trait Clock: std::fmt::Debug + Send + Sync {
    /// Current time in milliseconds since the Unix epoch.
    fn now_epoch_ms(&self) -> u64;
}

/// The real system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_epoch_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Service configuration: per-device quota and rate limiting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServiceConfig {
    /// Max bytes of ciphertext a single device may store.
    pub quota_bytes: u64,
    /// Token-bucket capacity (max burst of requests before limiting).
    pub rate_capacity: u32,
    /// Tokens refilled per second.
    pub rate_refill_per_sec: f64,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            quota_bytes: 1024 * 1024,
            rate_capacity: 60,
            rate_refill_per_sec: 10.0,
        }
    }
}

/// Why the backend refused an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceError {
    /// The device tried to write an envelope it does not send (forged sender
    /// identity).
    Forbidden,
    /// The envelope uses a protocol version this backend does not serve.
    UnsupportedVersion,
    /// The device's ciphertext quota would be exceeded.
    QuotaExceeded {
        /// Device id.
        device: u64,
        /// Bytes currently used (before this write).
        used: u64,
        /// The configured quota.
        quota: u64,
    },
    /// The per-device rate limit was exceeded.
    RateLimited {
        /// Device id.
        device: u64,
        /// Seconds until the device may retry.
        retry_after_secs: u64,
    },
}

/// A content-free audit record. Never contains envelope contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    /// When the action happened (epoch ms).
    pub at_epoch_ms: u64,
    /// Device that performed the action.
    pub device: u64,
    /// Action: "put", "get", "delete", or "list".
    pub action: &'static str,
    /// Envelope id (routing metadata only).
    pub envelope_id: String,
    /// Stored byte length (ciphertext + routing metadata), or 0 for list.
    pub byte_len: u64,
}

/// Content-free envelope metadata returned by [`SyncBackend::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeMeta {
    /// Envelope id.
    pub id: String,
    /// Sender device.
    pub sender: u64,
    /// Recipient device.
    pub recipient: u64,
    /// Protocol version.
    pub version: u8,
    /// Stored byte length (ciphertext + routing metadata).
    pub byte_len: u64,
}

/// A per-device token bucket for rate limiting.
#[derive(Debug, Clone)]
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last_refill_ms: u64,
}

impl TokenBucket {
    fn new(capacity: u32, refill_per_sec: f64, now_ms: u64) -> Self {
        Self {
            capacity: capacity as f64,
            tokens: capacity as f64,
            refill_per_sec,
            last_refill_ms: now_ms,
        }
    }

    /// Refills lazily and tries to spend one token.
    fn try_acquire(&mut self, now_ms: u64) -> bool {
        if now_ms > self.last_refill_ms {
            let elapsed_secs = (now_ms - self.last_refill_ms) as f64 / 1000.0;
            self.tokens = (self.tokens + elapsed_secs * self.refill_per_sec).min(self.capacity);
            self.last_refill_ms = now_ms;
        }
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Whole seconds until the bucket is full enough for one request.
    fn retry_after_secs(&self) -> u64 {
        let deficit = (self.capacity - self.tokens).max(0.0);
        if self.refill_per_sec <= 0.0 {
            u64::MAX
        } else {
            (deficit / self.refill_per_sec).ceil() as u64
        }
    }
}

/// The minimal trusted sync backend.
#[derive(Debug, Clone)]
pub struct SyncBackend {
    config: ServiceConfig,
    store: AtomicStore,
    usage: Arc<Mutex<HashMap<u64, u64>>>,
    buckets: Arc<Mutex<HashMap<u64, TokenBucket>>>,
    audit: Arc<Mutex<Vec<AuditRecord>>>,
    locks: Arc<Mutex<HashMap<u64, Arc<Mutex<()>>>>>,
    clock: Arc<dyn Clock>,
}

const MAILBOX_PREFIX: &str = "mailbox:";
const INDEX_PREFIX: &str = "index:";

fn mailbox_key(device: u64, id: &str) -> String {
    format!("{MAILBOX_PREFIX}{device}:{id}")
}

fn index_key(device: u64) -> String {
    format!("{INDEX_PREFIX}{device}")
}

/// Server-side envelope storage encoding (version + routing + ciphertext).
fn encode_envelope(envelope: &SyncEnvelope) -> Vec<u8> {
    let mut out = Vec::with_capacity(33 + envelope.ciphertext.len());
    out.push(envelope.version);
    out.extend_from_slice(&envelope.sender.to_le_bytes());
    out.extend_from_slice(&envelope.recipient.to_le_bytes());
    out.extend_from_slice(&envelope.nonce);
    out.extend_from_slice(&(envelope.ciphertext.len() as u32).to_le_bytes());
    out.extend_from_slice(&envelope.ciphertext);
    out
}

/// Decodes a stored envelope; `None` on any malformed input.
fn decode_envelope(bytes: &[u8]) -> Option<SyncEnvelope> {
    if bytes.len() < 33 {
        return None;
    }
    let version = bytes[0];
    let sender = u64::from_le_bytes(bytes[1..9].try_into().ok()?);
    let recipient = u64::from_le_bytes(bytes[9..17].try_into().ok()?);
    let nonce: [u8; 12] = bytes[17..29].try_into().ok()?;
    let cipher_len = u32::from_le_bytes(bytes[29..33].try_into().ok()?) as usize;
    if bytes.len() != 33 + cipher_len {
        return None;
    }
    Some(SyncEnvelope {
        version,
        sender,
        recipient,
        nonce,
        ciphertext: bytes[33..].to_vec(),
    })
}

fn parse_index(bytes: Option<Vec<u8>>) -> Vec<String> {
    bytes
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .split('\n')
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn add_index_id(store: &AtomicStore, device: u64, id: &str) {
    let mut ids = parse_index(store.get(&index_key(device)));
    if !ids.iter().any(|existing| existing == id) {
        ids.push(id.to_owned());
        ids.sort();
        store.set(&index_key(device), ids.join("\n").into_bytes());
    }
}

fn remove_index_id(store: &AtomicStore, device: u64, id: &str) {
    let mut ids = parse_index(store.get(&index_key(device)));
    ids.retain(|existing| existing != id);
    store.set(&index_key(device), ids.join("\n").into_bytes());
}

impl SyncBackend {
    /// Creates a backend with the given configuration and clock.
    pub fn new(config: ServiceConfig, clock: Arc<dyn Clock>) -> Self {
        Self {
            config,
            store: AtomicStore::new(),
            usage: Arc::new(Mutex::new(HashMap::new())),
            buckets: Arc::new(Mutex::new(HashMap::new())),
            audit: Arc::new(Mutex::new(Vec::new())),
            locks: Arc::new(Mutex::new(HashMap::new())),
            clock,
        }
    }

    /// The per-device lock; all mutations for one device are serialized so
    /// quota + mailbox stay consistent even under concurrent writers.
    fn lock_for(&self, device: u64) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().expect("locks lock");
        locks
            .entry(device)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Spends one rate-limit token for the device, or returns
    /// [`ServiceError::RateLimited`].
    fn acquire_token(&self, device: u64) -> Result<(), ServiceError> {
        let now = self.clock.now_epoch_ms();
        let mut buckets = self.buckets.lock().expect("buckets lock");
        let bucket = buckets.entry(device).or_insert_with(|| {
            TokenBucket::new(
                self.config.rate_capacity,
                self.config.rate_refill_per_sec,
                now,
            )
        });
        if bucket.try_acquire(now) {
            Ok(())
        } else {
            Err(ServiceError::RateLimited {
                device,
                retry_after_secs: bucket.retry_after_secs(),
            })
        }
    }

    fn record_audit(&self, device: u64, action: &'static str, id: &str, byte_len: u64) {
        self.audit.lock().expect("audit lock").push(AuditRecord {
            at_epoch_ms: self.clock.now_epoch_ms(),
            device,
            action,
            envelope_id: id.to_owned(),
            byte_len,
        });
    }

    /// Stores an envelope addressed from `device` to its recipient.
    ///
    /// The envelope is placed in both the sender's and the recipient's
    /// mailboxes; quota is charged to the sender once. Refused with
    /// [`ServiceError::Forbidden`] when `device` is not the envelope sender,
    /// [`ServiceError::UnsupportedVersion`] for unknown protocol versions,
    /// [`ServiceError::RateLimited`] when the device exceeds its burst, and
    /// [`ServiceError::QuotaExceeded`] when the write would exceed the
    /// device's ciphertext quota.
    pub fn put(&self, device: u64, id: &str, envelope: SyncEnvelope) -> Result<(), ServiceError> {
        self.acquire_token(device)?;
        if device != envelope.sender {
            return Err(ServiceError::Forbidden);
        }
        if envelope.version != SYNC_PROTOCOL_VERSION {
            return Err(ServiceError::UnsupportedVersion);
        }
        let lock = self.lock_for(device);
        let _guard = lock.lock().expect("device lock");
        let bytes = encode_envelope(&envelope);
        let size = bytes.len() as u64;
        // Overwriting your own id frees the previous copy's bytes first.
        let old_size = self
            .store
            .get(&mailbox_key(device, id))
            .as_deref()
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(0);
        let mut used = self.usage.lock().expect("usage lock");
        let current = used.get(&device).copied().unwrap_or(0);
        let new_used = current.saturating_sub(old_size).saturating_add(size);
        if new_used > self.config.quota_bytes {
            return Err(ServiceError::QuotaExceeded {
                device,
                used: current,
                quota: self.config.quota_bytes,
            });
        }
        // Sender's mailbox (and the recipient's, when different).
        self.store.set(&mailbox_key(device, id), bytes.clone());
        add_index_id(&self.store, device, id);
        if envelope.recipient != device {
            self.store.set(&mailbox_key(envelope.recipient, id), bytes);
            add_index_id(&self.store, envelope.recipient, id);
        }
        used.insert(device, new_used);
        drop(used);
        self.record_audit(device, "put", id, size);
        Ok(())
    }

    /// Fetches an envelope from the device's own mailbox. Envelopes are only
    /// ever placed in mailboxes they are addressed to, so a device can never
    /// read another device's data; a missing envelope is `Ok(None)`.
    pub fn get(&self, device: u64, id: &str) -> Result<Option<SyncEnvelope>, ServiceError> {
        self.acquire_token(device)?;
        let Some(bytes) = self.store.get(&mailbox_key(device, id)) else {
            self.record_audit(device, "get", id, 0);
            return Ok(None);
        };
        let size = bytes.len() as u64;
        // Corrupted storage is treated as absent (defensive).
        if decode_envelope(&bytes).is_none() {
            self.record_audit(device, "get", id, 0);
            return Ok(None);
        }
        self.record_audit(device, "get", id, size);
        Ok(Some(decode_envelope(&bytes).expect("decoded above")))
    }

    /// Removes an envelope from the device's own mailbox. Idempotent:
    /// `Ok(true)` when it existed, `Ok(false)` when it did not. Deleting as
    /// the sender frees the device's quota; the other device keeps its copy.
    pub fn delete(&self, device: u64, id: &str) -> Result<bool, ServiceError> {
        self.acquire_token(device)?;
        let lock = self.lock_for(device);
        let _guard = lock.lock().expect("device lock");
        let key = mailbox_key(device, id);
        let Some(bytes) = self.store.get(&key) else {
            self.record_audit(device, "delete", id, 0);
            return Ok(false);
        };
        let Some(envelope) = decode_envelope(&bytes) else {
            self.record_audit(device, "delete", id, 0);
            return Ok(false);
        };
        let size = bytes.len() as u64;
        self.store.delete(&key);
        remove_index_id(&self.store, device, id);
        // Only the sender's quota is charged, so only the sender frees it.
        if device == envelope.sender {
            let mut used = self.usage.lock().expect("usage lock");
            let current = used.get(&device).copied().unwrap_or(0);
            used.insert(device, current.saturating_sub(size));
        }
        self.record_audit(device, "delete", id, size);
        Ok(true)
    }

    /// Lists content-free metadata for the device's own mailbox.
    pub fn list(&self, device: u64) -> Result<Vec<EnvelopeMeta>, ServiceError> {
        self.acquire_token(device)?;
        let ids = parse_index(self.store.get(&index_key(device)));
        let mut metas = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(bytes) = self.store.get(&mailbox_key(device, &id)) {
                if let Some(envelope) = decode_envelope(&bytes) {
                    metas.push(EnvelopeMeta {
                        id,
                        sender: envelope.sender,
                        recipient: envelope.recipient,
                        version: envelope.version,
                        byte_len: bytes.len() as u64,
                    });
                }
            }
        }
        self.record_audit(device, "list", "", 0);
        Ok(metas)
    }

    /// Current ciphertext bytes charged to a device.
    pub fn usage(&self, device: u64) -> u64 {
        self.usage
            .lock()
            .expect("usage lock")
            .get(&device)
            .copied()
            .unwrap_or(0)
    }

    /// The content-free audit log (metadata only).
    pub fn audit_log(&self) -> Vec<AuditRecord> {
        self.audit.lock().expect("audit lock").clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::{Clock, ServiceConfig, ServiceError, SyncBackend, TokenBucket};
    use sync_core::{SyncEnvelope, SYNC_PROTOCOL_VERSION};

    /// A deterministic, advanceable clock for tests.
    #[derive(Debug)]
    struct TestClock {
        now_ms: std::sync::Mutex<u64>,
    }

    impl TestClock {
        fn new(now_ms: u64) -> Self {
            Self {
                now_ms: std::sync::Mutex::new(now_ms),
            }
        }

        fn advance(&self, ms: u64) {
            *self.now_ms.lock().unwrap() += ms;
        }
    }

    impl Clock for TestClock {
        fn now_epoch_ms(&self) -> u64 {
            *self.now_ms.lock().unwrap()
        }
    }

    fn envelope(sender: u64, recipient: u64, ciphertext: Vec<u8>) -> SyncEnvelope {
        SyncEnvelope {
            version: SYNC_PROTOCOL_VERSION,
            sender,
            recipient,
            nonce: [0u8; 12],
            ciphertext,
        }
    }

    fn backend(config: ServiceConfig) -> (SyncBackend, Arc<TestClock>) {
        let clock = Arc::new(TestClock::new(1_000_000));
        let backend = SyncBackend::new(config, clock.clone());
        (backend, clock)
    }

    fn lenient_config() -> ServiceConfig {
        ServiceConfig {
            quota_bytes: 1 << 20,
            rate_capacity: 1000,
            rate_refill_per_sec: 1000.0,
        }
    }

    #[test]
    fn token_bucket_refills_and_limits() {
        let mut bucket = TokenBucket::new(2, 1.0, 0);
        assert!(bucket.try_acquire(0));
        assert!(bucket.try_acquire(0));
        assert!(!bucket.try_acquire(0));
        assert!(bucket.retry_after_secs() >= 1);
        // One second later the bucket has one token again.
        assert!(bucket.try_acquire(1_000));
        assert!(!bucket.try_acquire(1_000));
    }

    #[test]
    fn api_contract_put_get_list_delete_round_trip() {
        let (backend, _clock) = backend(lenient_config());
        backend
            .put(1, "a", envelope(1, 2, b"encrypted".to_vec()))
            .unwrap();
        // Sender and recipient can both read their own copies.
        let fetched = backend.get(1, "a").unwrap().unwrap();
        assert_eq!(fetched.sender, 1);
        assert_eq!(fetched.recipient, 2);
        assert_eq!(fetched.ciphertext, b"encrypted");
        assert_eq!(
            backend.get(2, "a").unwrap().unwrap().ciphertext,
            b"encrypted"
        );
        // Each mailbox lists the envelope; metadata is content-free.
        let sender_metas = backend.list(1).unwrap();
        assert_eq!(sender_metas.len(), 1);
        assert_eq!(sender_metas[0].id, "a");
        assert_eq!(sender_metas[0].sender, 1);
        assert_eq!(sender_metas[0].recipient, 2);
        assert_eq!(sender_metas[0].byte_len, 42); // 33 + 9 ciphertext bytes
        assert_eq!(backend.list(2).unwrap().len(), 1);
        // Quota is charged to the sender once (logical bytes).
        assert_eq!(backend.usage(1), 42);
        assert_eq!(backend.usage(2), 0);
        // Sender delete frees quota and its own copy; the recipient keeps its.
        assert!(backend.delete(1, "a").unwrap());
        assert!(!backend.delete(1, "a").unwrap());
        assert_eq!(backend.get(1, "a").unwrap(), None);
        assert_eq!(backend.usage(1), 0);
        assert!(backend.list(1).unwrap().is_empty());
        assert!(backend.get(2, "a").unwrap().is_some());
        // The recipient removes its own copy.
        assert!(backend.delete(2, "a").unwrap());
        assert_eq!(backend.get(2, "a").unwrap(), None);
    }

    #[test]
    fn unauthorized_device_cannot_read_or_write() {
        let (backend, _clock) = backend(lenient_config());
        backend
            .put(1, "a", envelope(1, 2, b"secret".to_vec()))
            .unwrap();
        // A third device is structurally isolated: no data, no existence leak.
        assert_eq!(backend.get(3, "a"), Ok(None));
        assert_eq!(backend.delete(3, "a"), Ok(false));
        assert!(backend.list(3).unwrap().is_empty());
        // Forging the sender identity is refused outright.
        assert_eq!(
            backend.put(3, "x", envelope(1, 2, b"forged".to_vec())),
            Err(ServiceError::Forbidden)
        );
        // The recipient can read; the sender can delete.
        assert!(backend.get(2, "a").unwrap().is_some());
        assert!(backend.delete(1, "a").unwrap());
        // Unknown protocol versions are refused.
        let mut bad = envelope(1, 2, b"v9".to_vec());
        bad.version = 9;
        assert_eq!(
            backend.put(1, "v9", bad),
            Err(ServiceError::UnsupportedVersion)
        );
    }

    #[test]
    fn quota_is_enforced_and_frees_on_delete() {
        let (backend, _clock) = backend(ServiceConfig {
            quota_bytes: 100,
            rate_capacity: 1000,
            rate_refill_per_sec: 1000.0,
        });
        backend.put(1, "a", envelope(1, 2, vec![0u8; 9])).unwrap(); // 42 bytes
        backend.put(1, "b", envelope(1, 2, vec![0u8; 9])).unwrap(); // 84 bytes
        assert_eq!(backend.usage(1), 84);
        // The third write would reach 126 > 100.
        assert_eq!(
            backend.put(1, "c", envelope(1, 2, vec![0u8; 9])),
            Err(ServiceError::QuotaExceeded {
                device: 1,
                used: 84,
                quota: 100
            })
        );
        assert_eq!(backend.usage(1), 84);
        assert_eq!(backend.list(1).unwrap().len(), 2);
        // Deleting frees quota; the write now fits.
        assert!(backend.delete(1, "a").unwrap());
        assert_eq!(backend.usage(1), 42);
        backend.put(1, "c", envelope(1, 2, vec![0u8; 9])).unwrap();
        assert_eq!(backend.usage(1), 84);
    }

    #[test]
    fn rate_limit_denies_bursts_and_recovers() {
        let (backend, clock) = backend(ServiceConfig {
            quota_bytes: 1 << 20,
            rate_capacity: 2,
            rate_refill_per_sec: 1.0,
        });
        backend.put(1, "a", envelope(1, 2, vec![0u8; 8])).unwrap();
        backend.put(1, "b", envelope(1, 2, vec![0u8; 8])).unwrap();
        // Burst exhausted: the third request is rate limited with retry-after.
        let Err(ServiceError::RateLimited {
            device,
            retry_after_secs,
        }) = backend.put(1, "c", envelope(1, 2, vec![0u8; 8]))
        else {
            panic!("expected rate limited");
        };
        assert_eq!(device, 1);
        assert!(retry_after_secs >= 1);
        // After one second a token refills and the write succeeds.
        clock.advance(1_000);
        backend.put(1, "c", envelope(1, 2, vec![0u8; 8])).unwrap();
        // Reads are rate limited too: wait for another token before the get.
        clock.advance(1_000);
        assert!(backend.get(1, "c").unwrap().is_some());
        // Rate limiting is per device: device 2 is unaffected.
        backend.put(2, "x", envelope(2, 1, vec![0u8; 8])).unwrap();
        backend.put(2, "y", envelope(2, 1, vec![0u8; 8])).unwrap();
    }

    #[test]
    fn audit_log_is_content_free() {
        let (backend, _clock) = backend(lenient_config());
        backend
            .put(1, "a", envelope(1, 2, b"PLAINTEXT_MARKER_1234".to_vec()))
            .unwrap();
        backend.get(2, "a").unwrap();
        backend.list(1).unwrap();
        backend.delete(1, "a").unwrap();
        let audit = backend.audit_log();
        let actions: Vec<&str> = audit.iter().map(|record| record.action).collect();
        assert_eq!(actions, vec!["put", "get", "list", "delete"]);
        let serialized = audit
            .iter()
            .flat_map(|record| [record.action, record.envelope_id.as_str()])
            .collect::<Vec<_>>()
            .join("|");
        // Content (plaintext marker or ciphertext) never enters the audit log.
        assert!(!serialized.contains("PLAINTEXT_MARKER_1234"));
        assert!(!serialized.contains("secret"));
        // Every record has only metadata fields (21-byte marker => 54 bytes).
        for record in &audit {
            let expected = match record.action {
                "put" | "get" | "delete" => 54,
                _ => 0,
            };
            assert_eq!(record.byte_len, expected);
            assert!(record.at_epoch_ms >= 1_000_000);
        }
    }

    #[test]
    fn concurrent_load_preserves_all_writes_and_quota() {
        // 4 devices x 40 writes each: every write must be stored, usage must
        // equal the sum of stored sizes, and the audit must be complete.
        let (backend, _clock) = backend(lenient_config());
        let backend = Arc::new(backend);
        let threads: Vec<_> = (0..4u64)
            .map(|device| {
                let backend = Arc::clone(&backend);
                thread::spawn(move || {
                    for index in 0..40 {
                        let id = format!("t{device}-{index}");
                        let ciphertext = vec![device as u8; 10 + index];
                        backend
                            .put(device, &id, envelope(device, (device + 1) % 4, ciphertext))
                            .unwrap();
                    }
                })
            })
            .collect();
        for handle in threads {
            handle.join().expect("thread");
        }
        for device in 0..4u64 {
            // Dual mailboxes: 40 sent + 40 received (each device is a
            // recipient of exactly one other device) = 80 envelopes.
            let metas = backend.list(device).unwrap();
            assert_eq!(metas.len(), 80, "device {device} must keep all writes");
            let expected: u64 = (0..40).map(|index| (33 + 10 + index) as u64).sum();
            assert_eq!(backend.usage(device), expected);
            assert_eq!(
                backend
                    .get(device, &format!("t{device}-1"))
                    .unwrap()
                    .unwrap()
                    .sender,
                device
            );
        }
        let audit = backend.audit_log();
        assert_eq!(audit.len(), 4 * 40 + 8); // 160 puts + 4 lists + 4 gets
        assert_eq!(
            audit.iter().filter(|record| record.action == "put").count(),
            4 * 40
        );
        assert_eq!(
            audit
                .iter()
                .filter(|record| record.action == "list")
                .count(),
            4
        );
        assert_eq!(
            audit.iter().filter(|record| record.action == "get").count(),
            4
        );
    }

    #[test]
    fn same_device_concurrent_puts_do_not_lose_writes() {
        // Two threads writing distinct ids to the SAME device: quota and the
        // mailbox stay consistent (no lost writes, no under-counted quota).
        let (backend, _clock) = backend(lenient_config());
        let backend = Arc::new(backend);
        let handles: Vec<_> = (0..2)
            .map(|thread_id| {
                let backend = Arc::clone(&backend);
                thread::spawn(move || {
                    for index in 0..25 {
                        let id = format!("w{thread_id}-{index}");
                        let ciphertext = vec![0u8; 12];
                        backend.put(1, &id, envelope(1, 2, ciphertext)).unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(backend.list(1).unwrap().len(), 50);
        assert_eq!(backend.usage(1), 50 * 45); // 33 + 12
    }
}
