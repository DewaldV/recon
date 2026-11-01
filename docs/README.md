# Recon implementation plans

## Objective

Replace the spreadsheet preparation workflow with an exception-based local app:

1. Import a Virgin CSV.
2. Resolve known merchants and approved account rules automatically.
3. Review only missing decisions and exceptions, with an all-transactions view
   available for inspection.
4. Approve exact per-account deposit instructions.
5. Execute safely and resume without repeating completed deposits.

Virgin-app export and Proton Drive transfer remain manual initially.

## Agreed constraints

- New standalone project; existing `monzo-cli` remains usable and unchanged.
- Rust application; SQLite is the persistent source of truth.
- devenv + direnv supply development tools.
- Axum + Askama + HTMX, with no Node-based frontend tooling; see
  [decision log](decisions.md) and [alternatives](frontend-options.md).
- Local-only by default; financial data and secrets stay off hosted services.
- Offline import/review needs no Monzo token.
- Monzo credentials stay in the backend, injected through `pass-cli run` when
  needed. Do not store them in the database, browser storage, or logs.
- Categories are local reporting data: the documented Monzo API does not expose
  category updates. Descriptions can become transaction notes.
- Preserve one deposit per purchase rather than aggregating by account.
- No money movement in Phase 1, including through a hidden UI action.

## Proposed architecture

Start with a single Rust executable/crate, not a multi-service system. Separate
domain/import/rules/storage/HTTP/Monzo concerns into modules as implemented. Keep
business rules independent of HTTP handlers and templates so they are testable
without a browser. Use Axum handlers, Askama templates, and locally served HTMX.
Store templates and static assets alongside the Rust source;
there is no frontend build pipeline requirement.

The backend owns validation, rules, approval, database transactions, and money
movement. SQLite migrations are versioned in source control. Use integer pennies
for money, with strict decimal parsing and explicit currency handling.

Bind to loopback only. Protect state-changing requests with CSRF and origin/host
checks, avoid GET mutations, escape imported text, and serve assets locally.
Localhost alone is not an authorization boundary. Select the local session/access
mechanism before exposing sensitive data through HTTP.

Store live data in a configurable location outside the repository, defaulting to
the appropriate XDG application data directory. Preserve original CSVs and their
hashes as import provenance. Define backup and restore before the first live
migration. Do not watch a cloud-synced SQLite database directly.

## Existing data and transition

At planning time (2026-11-01):

- Existing recons: `~/Documents/financial/recons`.
- Reference CLI: `~/Projects/dewaldv/monzo-cli`.
- `2026-10-17` is prepared but has not been run against Monzo (user confirmed).
- `2026-10-31` is newly started; its copied workbook content matches October 17.
  A fresh `transactions.csv` is now present (confirmed during preparation).
- `lastpostingdate.txt` contains `2026-09-27`; it is a migration hint, not evidence
  of payment or a reliable identity boundary.
- The latest workbook contains a merchant catalogue and reviewed account choices.

Read legacy files without modifying them. Import merchant details and transaction
decisions with provenance. Never infer execution from a CSV export, directory
date, or a `Paid = No` cell. Historical execution status must be explicitly
confirmed. Older rows must not inadvertently become newly payable.

The user clarified that legacy `Paid = Yes` means handled outside recon (for
example, their partner already moved money to the Monzo credit-card pot), not
repayment of the credit-card bill. Increment 3 will preserve that evidence as
handled externally and prevent duplicate funding, subject to confirmed row matching.

## Delivery sequence

- [Decisions](decisions.md)
- [Development and local safety](development.md)
- [Reference-number investigation](phase-1/reference-investigation.md)
- [Increment 1: Import, review, persist](phase-1/increment-1.md)
- [Increment 2: Merchant enrichment and exception review](phase-1/increment-2.md)
- [Increment 3: Historical handling, account rules, and recon totals](phase-1/increment-3.md)
- [Increment 3 / Slice 1: Handling, recon scope, and totals](phase-1/increment-3-slice-1.md)
- [Initial schema contract](phase-1/schema.md)

1. [Offline import and review](phase-1-import-and-review.md).
2. [Safe Monzo execution](phase-2-monzo-execution.md).
3. [Workflow improvements](phase-3-workflow-improvements.md).

Start with synthetic fixtures. Test real October imports locally, without
committing private data. Live execution is a separate explicitly approved step.
