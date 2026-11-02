# Schema and migrations

The ledger schema is defined in [`migrations/`](../../migrations/) and applied by
`src/storage.rs`. Add a numbered migration for changes; do not rewrite applied SQL.

## Migration map

| Migration | Adds |
| --- | --- |
| [0001](../../migrations/0001_initial.sql) | Source accounts, imports, canonical purchases, observations, account decisions. |
| [0002](../../migrations/0002_original_currency.sql) | Original transaction currency for overlap checks. |
| [0003](../../migrations/0003_merchant_review.sql) | Merchant catalogue, aliases, categories, overrides, audited previews. |
| [0004](../../migrations/0004_recon_handling.sql) | Handling, named workspace status, and membership events. |
| [0005](../../migrations/0005_single_queue.sql) | Single-queue staging, account rules, source dispositions. |
| [0006](../../migrations/0006_runs.sql) | Immutable runs, operations, execution events, transaction mappings. |
| [0007](../../migrations/0007_historical_decisions.sql) | Historical decision import provenance. |
| [0008](../../migrations/0008_recon_cutover.sql) | Permanent posting-date boundary. |
| [0009](../../migrations/0009_import_repairs.sql) | Audited links between original and repaired observations. |
| [0010](../../migrations/0010_destination_settings.sql) | Versioned Personal/Joint account and pot settings. |

## Source and decision invariants

`source_accounts` identifies the Virgin account, independently of physical cards
and Personal/Joint allocation. Imports retain exact bytes, hash, filename, and time.
Purchases use `(source_account_id, reference)` identity; composite foreign keys
keep observations and purchases in the same source account.

Every source record has an observation, including malformed and non-purchase rows.
Overlaps reuse a purchase only when canonical fields agree. Conflicts retain their
observations without overwriting the purchase. Repairs link corrected observations
to originals. CSV record ordinals include the header as record 1 and may span
multiple physical lines.

Amounts are integer pennies. Purchase currency is GBP billing currency; original
transaction currency is stored separately. Rust validates calendar dates and
financial fields before insertion.

Source/audit records are append-only. Account decision versions start at 1; absence
means version 0. Mutations check expected versions before appending. Staging records
effective-decision signatures; runs snapshot their approved instructions and keys.

## Connection and migration policy

- SQLite STRICT tables, foreign keys enabled, three-second busy timeout,
  `synchronous=FULL`, and the default rollback journal.
- An advisory process lock per data directory and serialized database access.
- Each migration and its `user_version` update commit in one exclusive transaction.
  Unsupported newer schemas and nonempty unversioned databases are rejected.
- Existing versioned databases receive a verified snapshot before migration.
  Startup checks integrity and foreign keys.
- SQLite's backup API creates consistent snapshots. Data directories use 0700;
  database/lock files use 0600.

Execution recovery also depends on `execution.guard`, outside SQLite. Follow the
[restore procedure](../development.md#restore) when restoring a snapshot.
