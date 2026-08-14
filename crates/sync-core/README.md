# sync-core

- Layer: L1 (core services)
- Dependencies: `core-domain`, `core-protocol`
- Scope: synchronization-agnostic merge and conflict contracts.
- T016 status: buildable workspace skeleton; behavior is specified by later control rows.


## T092: optional end-to-end encrypted sync protocol

`crates/sync-core/src/sync_protocol.rs`:

- `SyncEnvelope` - a versioned AEAD envelope (sender/recipient + nonce +
  ciphertext); the server never sees plaintext.
- `DeviceIdentity` - device id + public key (private keys never enter records).
- `RotateKey` - generation-tagged key rotation (old generations stay
  decryptable).
- `RevocationList` - device revocation (idempotent).
- `TestVector` / `ThreatModel` - deterministic protocol test vectors and a
  structured threat-model review.

The protocol is optional (disabled by default); when enabled, only encrypted
envelopes reach the sync server.

## T093: local sync CRDT / conflict-merge core

`crates/sync-core/src/crdt.rs`:

- `CrdtState` - a per-key last-writer-wins register CRDT with `LamportClock`
  versions and tombstones.
- `set` / `delete` (tombstone) / `get` / `is_tombstone` / `merge` /
  `converge` - offline concurrent edits deterministically converge (merge is
  commutative and idempotent), and deletes are recoverable (a newer set wins
  over a tombstone).

Random multi-replica property tests verify convergence in any merge order.


## T095: device lifecycle (pairing, recovery, revocation, rotation)

`crates/sync-core/src/device_lifecycle.rs`:

- `KeyManager` - the primary device's lifecycle manager: one-time `PairingCode`
  pairing (new devices receive only the current generation's wrapped key),
  `revoke_device` (lost devices are excluded from the next rotation),
  `rotate_keys` (fresh generation, wrapped only for non-revoked devices),
  and `recover_data_key` via the `RecoveryCode`.
- `Device` - one device's vault: its `DeviceKey` plus wrapped data keys; a
  device can only decrypt a generation it holds the wrapped key for.
- `wrap_data_key` / `unwrap_data_key` - AEAD key wrapping (random nonce +
  Poly1305 tag); wrong keys and tampering are rejected.
- Security model: a revoked or offline device never receives the new
  generation's wrapped key, so it cannot read new data (it may still read
  data it already held). The recovery code restores the current data key.

New dependency: `getrandom` (already in the workspace lock via storage-sqlite;
declared in architecture/dependency-rules.json sync-core external_imports).