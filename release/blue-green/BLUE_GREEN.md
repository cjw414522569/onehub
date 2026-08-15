# Web/PWA + gateway release, migration, and rollback (T168)

Version: 1.0.0.

## Blue-green release

Two environments (blue = live, green = staging-inactive). The release:
deploy to the inactive environment -> health check -> switch traffic ->
verify. On any post-switch failure, traffic is switched back (rollback).
The gateway versioned session protocol (T135/T164) and the Web/PWA shell
keep a compatibility window so blue and green coexist with zero downtime.

## Database migration

storage-sqlite migrations are forward-only and idempotent (T101). The
drill: migrate the incoming environment's DB, health-check, switch, and on
failure restore the pre-migration backup (the T101 startup flow's backup
guidance). Migration rollback is rehearsed, not assumed.

## Drill

`scripts/test-blue-green.mjs` runs the deploy -> health -> switch ->
verify sequence and the rollback path, validates the compatibility window,
and archives `release/blue-green/blue-green.report.json`.