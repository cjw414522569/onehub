# syntax=docker/dockerfile:1
# Multi-stage build for the SSH gateway (T141).
# The runtime image runs as an unprivileged user (default non-root) with a
# read-only root filesystem (enforced by the compose/Helm runtime flags).

FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
RUN cargo build --release -p gateway --locked

FROM debian:bookworm-slim
# Unprivileged gateway user (no login shell).
RUN groupadd --gid 10001 gateway \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin gateway \
    && mkdir -p /tls /tmp \
    && chown -R gateway:gateway /tmp
COPY --from=build /src/target/release/ssh-gateway /usr/local/bin/ssh-gateway
USER 10001:10001
# TLS certificates are mounted read-only by the runtime (compose/Helm).
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ssh-gateway"]