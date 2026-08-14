# core-domain

- Layer: L0 (pure types)
- Dependencies: `secret`, `serde` (derive); no UI, database, or SSH implementation types.
- Scope: immutable domain values and identifiers only.

## Models (T021)

`HostId`, `HostAddress`, `Host`, `CredentialKind`, `CredentialRef`, `SessionProfile`, `DomainError`.

## T028: host key policy and known_hosts

`HostKeyPolicy`, `HostKeyStatus`, `HostKeyDecision`, `verify_host_key`, `KnownHostsEntry`, `HostKeyIdentity`.

## T029: credential provider and unlock interaction

`CredentialValue`, `AgentHandle` / `HardwareKeyHandle`, `UnlockInteraction`, `ProviderError`, `CredentialProvider`.

## T030: proxy chain and jump-host graph

`ProxyKind`, `HopPolicy`, `ProxyHop`, `ProxyChain`, `proxy_jump`, `proxy_jump_multi`.

## T031: port forwarding model

`ForwardingKind`, `ForwardingFamily`, `ForwardingEndpoint`, `ForwardingSpec`, `ForwardingTable`.

## T032: file transfer and remote file operations

`TransferDirection`, `TransferMode`, `TransferSpec`, `TransferStatus`, `TransferProgress`, `RemoteFileOp`, `TransferError`.

## T034: command snippets, macros, environment variables, sensitive fields

`EnvVar` / `Environment`, `PlaceholderDef` / `CommandSnippet`, `Macro`, `ResolvedCommand`, `resolve_command` with monotonic sensitivity propagation.

## T035: local settings, workspace, tabs, layout

| Model | Purpose |
|---|---|
| `SettingsScope` | Global / Account / Window with explicit precedence (window > account > global). |
| `SettingsValue` / `SettingsEntry` / `LocalSettings` | Typed settings store with `effective(key)` resolution. |
| `SettingsDocument` / `migrate_settings` | Versioned document (schema v2); migration applies defaults for missing global keys and is idempotent. |
| `TabId` / `Tab` | Tab identity, title, optional session profile. |
| `LayoutNode` / `SplitDirection` / `WindowLayout` | Split-tree layout with tab leaves; `tabs()` traversal. |
| `Workspace` | Collection of windows. |

Default and migration tests cover defaults, scope precedence (window > account > global), migration applying defaults while preserving user settings, idempotency, and serde round-trips for settings and layout.

## Validation

```text
cargo test -p core-domain --locked
cargo check -p core-domain --locked
node scripts/test-host-key.mjs .
node scripts/test-credential-provider.mjs .
node scripts/test-proxy-chain.mjs .
node scripts/test-forwarding.mjs .
node scripts/test-transfer.mjs .
node scripts/test-command.mjs .
node scripts/test-settings.mjs .
```