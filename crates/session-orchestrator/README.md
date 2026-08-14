# session-orchestrator

- Layer: L1 (core services)
- Dependencies: `core-domain`, `core-errors`, `core-protocol`, `serde`, `tokio`; no UI, database, or SSH implementation types.
- Scope: session lifecycle orchestration and structured concurrency conventions.

## T023: cancellation, deadlines, and structured concurrency

| Model | Purpose |
|---|---|
| `CancellationToken` | Shareable, cooperative cancellation signal (race-free via `watch`). |
| `CancelReason` | `Cancelled` or `DeadlineExpired`. |
| `Deadline` | Absolute `tokio::time::Instant`-based deadline with `remaining()` / `is_expired()`. |
| `select_cancellation` | Run a future; the token wins only if cancellation is signalled first. |
| `select_deadline` | Run a future; the deadline wins only if it expires first. |
| `select_guarded` | Run a future under both a token and a deadline. |

### Structured concurrency conventions

1. Every spawned task receives a cancellation token (or inherits the parent's).
2. Cancellation is cooperative: check the token between await points; use the `select_*` helpers.
3. Parents join their children before returning; no task leaks after `cancel()`.
4. `biased` selection makes operation completion win over simultaneous cancellation for deterministic semantics.
5. Deadlines are fixed at operation start and passed down; expired deadlines yield `CancelReason::DeadlineExpired`.
6. Tokio `JoinSet` is the recommended primitive for bounded task groups.

## T024: bounded message channel, backpressure, slow-consumer policy

`BoundedChannel<T>` is a bounded multi-producer multi-consumer channel with `close()` and running stats.

| Policy | Producer behavior | Use case |
|---|---|---|
| `Block` | `send` awaits capacity (backpressure) | Input/command path where loss is unacceptable |
| `DropNewest` | Never blocks; drops the newest item when full | High-rate terminal output |
| `DropOldest` | Never blocks; evicts the oldest buffered item | Live streaming where freshness matters |

Memory is bounded by `capacity` regardless of producer rate. Overflow is counted (`dropped_newest` / `dropped_oldest`) and observable via `stats()`. `close()` wakes blocked receivers; they drain remaining items and then observe `None`. The 1 GiB synthetic-output stress test (`one_gib_synthetic_output_stays_bounded_and_non_blocking`) verifies the producer never blocks and the buffer stays bounded under a deliberately slow consumer.

## T027: event-sourced session state machine

`SessionState`, `SessionEvent`, `SessionEffect`, and the pure function `apply(state, event)` define the session lifecycle:

- States: `Disconnected → Connecting → Authenticating → Online`, with `Reconnecting`, `Suspended`, `Closing`, and terminal `Closed`.
- Every state/event pair has a deterministic outcome: an accepted transition (with effects) or an explicit rejection. `Closed` is terminal.
- `replay` / `replay_from` fold an event log into a `SessionSnapshot`, stopping at the first rejected event (event sourcing).
- The exhaustive property test `every_state_event_pair_has_a_deterministic_outcome` checks all 8×10 pairs; `acceptance_flow_is_unambiguous` replays the full connect/auth/online/suspend/resume/reconnect/close flow to `Closed`.

## Verification

```text
cargo test -p session-orchestrator --locked
cargo check -p session-orchestrator --locked
node scripts/test-concurrency.mjs .
node scripts/test-bounded-channel.mjs .
node scripts/test-session-state.mjs .
```