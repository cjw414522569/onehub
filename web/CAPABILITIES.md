# Web restricted-capability model (T133)

The browser cannot open raw TCP sockets, so direct SSH is impossible. All
connections go through the gateway (WebSocket). This document lists every
capability's availability and the user-visible difference.

## Raw TCP

`raw_tcp.status = unavailable` - browsers cannot open raw TCP sockets.
Direct SSH is never offered; connections go through the gateway.

## Capabilities

| Capability | Status | Gated | User-visible note |
| --- | --- | --- | --- |
| terminal.rendering | available | no | Terminal renders via WebAssembly |
| ssh.direct_tcp | unavailable | yes | Direct SSH unavailable; use the gateway |
| ssh.via_gateway | available | no | SSH through the gateway WebSocket |
| sftp.via_gateway | available | no | File transfers through the gateway |
| local.filesystem | unavailable | yes | No local filesystem access |
| clipboard | partial | no | Requires a permission prompt |
| notifications | partial | no | Requires a permission prompt |
| port_forward.direct | unavailable | yes | No raw TCP |
| port_forward.via_gateway | available | no | Gateway-mediated forwarding |
| key_storage.hardware | unavailable | yes | No OS keystore; keys in memory |
| multi_window | available | no | Browser tabs/windows act as windows |

## UI gating

Every capability marked `gated` is hidden or shown as unavailable in the UI
(a gated capability is never shown as usable). `scripts/lint-web-capabilities.mjs`
enforces this consistency; `scripts/test-web-capabilities.mjs` adds a
regenerable snapshot.