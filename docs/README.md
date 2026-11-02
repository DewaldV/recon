# Recon documentation

## Guides

- [Workflow](single-queue-workflow.md): import, review, staging, execution, and migration.
- [Development](development.md): commands, runtime protections, and backup/restore.
- [Decisions](decisions.md): rationale and superseded choices.
- [Schema](phase-1/schema.md): migrations and storage invariants.

## Architecture

Recon is one Rust executable with a local SQLite ledger. Axum serves Askama
templates, HTMX, CSS, and JavaScript from the application. HTTP handlers delegate
to domain modules; SQLite and parsing work use a bounded blocking worker.

| Code | Responsibility |
| --- | --- |
| `src/main.rs`, `src/config.rs` | CLI, private data paths, loopback startup. |
| `src/storage.rs`, `migrations/` | Imports, source provenance, migrations, snapshots. |
| `src/importer.rs`, `src/import_repair.rs` | Virgin CSV parsing and audited extra-column repair. |
| `src/review.rs`, `src/rules.rs`, `src/queue.rs` | Merchant defaults, account rules, review, staging. |
| `src/history_import.rs`, `src/cutover.rs` | Historical decisions and permanent posting-date boundary. |
| `src/recon.rs` | Handling history and legacy workspace records. |
| `src/settings.rs` | Persistent Personal/Joint account and pot destinations. |
| `src/execution.rs`, `src/monzo/`, `src/monzo.rs` | Immutable runs, deposits, notes, and recovery. |
| `src/web/`, `src/web.rs`, `templates/`, `static/` | Local HTTP interface and assets. |

Purchases are identified by source account and Virgin reference, with canonical
field checks on overlap. Original CSVs and observations are retained. Money uses
integer pennies; account allocation, handling, staging, and execution are separate
records. Each purchase produces one deposit and note. Categories remain local.

Import and review work offline. Execution uses a temporary token held in backend
memory for one attempt. Run instructions and deduplication keys are persisted
before submission; an independent `execution.guard` detects stale database restores.

## Planning and delivery history

These records describe their original scope. Use the guides above for current
behavior and [remaining work](single-queue-workflow.md#remaining-work).

- [Frontend selection](frontend-options.md)
- [Reference-number investigation](phase-1/reference-investigation.md)
- [Phase 1: Offline import and review](phase-1-import-and-review.md)
  - [Increment 1](phase-1/increment-1.md) · [verification](phase-1/increment-1-verification.md)
  - [Increment 2](phase-1/increment-2.md) · [verification](phase-1/increment-2-verification.md)
  - [Increment 3](phase-1/increment-3.md) · [Slice 1 delivery](phase-1/increment-3-slice-1.md)
- [Phase 2: Monzo execution](phase-2-monzo-execution.md)
- [Phase 3: Workflow improvements](phase-3-workflow-improvements.md) — candidate work.
