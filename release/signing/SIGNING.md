# Release signing, timestamping, notarization, and key rotation (T165)

Version: 1.0.0.

## Policy

- Every release artifact is signed; signatures are timestamped (RFC 3161)
  and platform-required notarization is applied (Apple notary for macOS,
  Authenticode for Windows, app-store signing for mobile).
- Signing keys are least-privilege: short-lived (<= 30 days),
  per-platform, usage-limited to signing, stored air-gapped/off-CI, rotated
  on a schedule, and every signing operation is audited (no secrets in
  audit logs).
- A recovery drill (key compromise -> rotate -> revoke -> re-sign) is
  completed and recorded.

## Signature chain

`scripts/test-signing-pipeline.mjs` builds the artifact manifest (SHA-256
hashes of the real build artifacts), signs each digest, timestamps and
marks notarization per platform, then verifies the full chain (manifest
integrity, signature, timestamp, notarization status) and runs the
recovery drill.