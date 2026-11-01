# Increment 1 schema contract

This documents the original import/review schema. Migration 0003 subsequently
adds merchant review, and migration 0004 adds append-only handling, recon status,
and membership events; see [Increment 3 / Slice 1](increment-3-slice-1.md).

The executable starting schema is `migrations/0001_initial.sql`; migration 0002
adds original transaction currency for canonical overlap checks. The production
runner applies pending migrations transactionally and tests cover fresh/reopened,
upgraded, rejected, and failed migrations. Do not rewrite an applied migration;
add numbered migrations for changes.

## Tables

| Table | Purpose |
| --- | --- |
| `source_accounts` | Stable Virgin account scope, distinct from Personal/Joint allocation and physical cards. |
| `imports` | One source file per account/hash, original filename (display only), exact original bytes, UTC import time. SQLite archival makes provenance atomic with import. |
| `purchases` | Canonical, positive GBP billed debit purchase keyed by source account + reference. Never store execution status in account assignment. |
| `source_observations` | One record per input row, including non-purchases/malformed rows; source text, disposition/reason, and optional link to purchase. Conflicts can link to the existing purchase without overwriting it. |
| `review_decisions` | Append-only versioned account decisions; NULL clears an assignment. View `current_account_decisions` selects the latest decision. |

All tables are STRICT. Composite foreign keys enforce same-account links between
imports, observations, and purchases. Triggers prohibit modification/deletion of
source observations, imports, and review events. Canonical purchases are immutable
in this increment: corrections require an explicit future audited resolution.

The account decision sequence starts at version 1; absent decision means version
0. A transaction checks the expected current version before appending, then
commits; the uniqueness constraint prevents simultaneous equal-version writes.
The HTTP layer treats conflicts as stale updates, not silent overwrites.

## Import transaction

Parse bounded input before a database write transaction. Check file hash within
the account. A duplicate returns the original import. Otherwise, write the original
bytes, canonical purchases, and one disposition per row atomically. Canonical
reuse requires core-field agreement. Conflicts and unknown identities do not get
new speculative purchase identities. Any storage failure rolls back the import.

`currency` on a purchase is the billing currency (GBP), not necessarily Virgin's
`Transaction Currency`. The raw original-currency field remains in source
provenance; never confuse the converted billing amount with the original amount.

Dates use validated ISO calendar dates; validate in Rust, not just SQLite string
checks. Currency/enum validation and amount parsing happen before insertion.
JSON-shaped parsed fields, if added later, do not replace preservation of source
bytes. `source_text` may contain embedded newlines; row number is the CSV record
ordinal including the header as record 1, not necessarily a physical line number.

## Connection/migration policy

- Enable and verify `PRAGMA foreign_keys = ON` on every connection.
- Use a bounded busy timeout and one serialized writer. Start with SQLite's
  default rollback journal and `synchronous=FULL`; WAL is unnecessary initially.
  An advisory process lock prevents two app instances sharing a data directory.
- Apply each pending migration in an exclusive transaction. Set `user_version`
  inside that same transaction only after success. Refuse unsupported newer schemas.
- Use the SQLite online backup API (not copying an open database file). Back up
  before migrating an existing nonempty database. Restore only with the app stopped;
  verify integrity, schema version, and foreign keys before opening the UI.
- Create private directories (0700) and files (0600); original files are sensitive.
- Future payment operations, rules, merchant details, decisions history beyond
  account assignment, and explicit handling states arrive through later migrations.
  In this increment all purchases have unknown execution history and no payable
  state or executable operation exists.
