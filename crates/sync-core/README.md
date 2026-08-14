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
