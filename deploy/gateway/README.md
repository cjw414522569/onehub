# SSH gateway deployment and hardening guide (T141)

The gateway is a self-hosted WebSocket/QUIC SSH gateway. This directory
provides a container, a Helm chart, a standalone (single-machine)
deployment, and the hardening guide.

## Layout

- `Dockerfile` - multi-stage build; the runtime image runs as
  `10001:10001` (default non-root) with a read-only root filesystem
  enforced at runtime.
- `docker-compose.yml` - standalone deployment: non-root user, read-only
  rootfs, `tmpfs` for `/tmp`, no-new-privileges, all capabilities dropped,
  TLS certificates mounted read-only (`./certs:/tls:ro`), and explicit
  CPU/memory limits.
- `helm/` - Helm chart `ssh-gateway`: `securityContext` (runAsNonRoot,
  readOnlyRootFilesystem, allowPrivilegeEscalation=false, capabilities
  drop ALL), resource limits, TLS secret volume, and a `tmpfs` emptyDir.
- `hardening.json` - the static container-scan matrix consumed by
  `scripts/test-gateway-deploy.mjs`.
- `scan-report.snapshot.json` - regenerable scan report (byte-identical).

## Standalone (docker compose)

```bash
mkdir -p deploy/gateway/certs
# place tls.crt / tls.key into deploy/gateway/certs (chmod 600)
docker compose -f deploy/gateway/docker-compose.yml up -d --build
```

## Helm

```bash
kubectl create secret tls ssh-gateway-tls \
  --cert=certs/tls.crt --key=certs/tls.key
helm install ssh-gateway deploy/gateway/helm
```

## Hardening checklist

| Control | Default |
| --- | --- |
| Default non-root | `USER 10001:10001`, `runAsNonRoot: true` |
| Read-only root filesystem | `read_only: true`, `readOnlyRootFilesystem: true`; only `/tmp` writable |
| TLS | certificates mounted read-only; never embedded in the image |
| Resource limits | CPU + memory limits on the container |
| Privilege escalation | `no-new-privileges`, `allowPrivilegeEscalation: false`, `cap_drop: ALL` |
| Secrets | the gateway never stores credentials; keys stay client-side (T137) |

## Verification

`scripts/test-gateway-deploy.mjs` runs a deterministic container scan
(every hardening control present, every forbidden pattern absent) plus an
install smoke (the gateway crate builds with `--locked`). A real
`docker`/`helm` render-and-run is `blocked_unavailable_toolchain` on hosts
without those toolchains; the static scan and config smoke cover the same
contract deterministically.