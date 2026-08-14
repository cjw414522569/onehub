# proxy

- Layer: L2 (protocol adapters)
- Dependencies: `core-domain`, `core-protocol`, `forwarding`, `tokio`
- Scope: SOCKS5 (RFC 1928/1929) and HTTP CONNECT proxy adapters, implemented
  in-house so the wire format stays auditable and the compatibility matrix runs
  over real loopback sockets.

## T051: SOCKS5 and HTTP CONNECT proxies

| Area | Coverage |
|---|---|
| SOCKS5 auth | RFC 1929 username/password negotiation; no-auth fallback; `E_PROXY_NO_METHOD` / `E_PROXY_AUTH_REJECTED`. |
| DNS policy | `DnsPolicy::RemoteResolve` (ATYP=DOMAINNAME) vs `LocalResolve` (ATYP=IPv4/IPv6), with `AddressFamily` preference. |
| IPv6 | IPv6 literal targets (ATYP=IPv6) and IPv6-first local resolution (Any). |
| Timeouts | Configurable handshake timeout for both protocols (`E_PROXY_TIMEOUT`). |
| HTTP CONNECT | `CONNECT host:port HTTP/1.1` + optional `Proxy-Authorization`; 2xx required, non-2xx -> `E_PROXY_HTTP_STATUS`. |
| Errors | Stable `E_PROXY_*` codes, no secret context. |

The compatibility matrix (protocol x auth x DNS policy x address family x
timeout x reply codes) runs as an integration test over loopback TCP with
in-process test servers.