# Increment 1: Import → review → persist

Historical scope, implemented and verified on 2026-11-01. See the
[verification record](increment-1-verification.md) and
[current workflow](../single-queue-workflow.md).

## Delivered

- Local Axum/Askama/HTMX application with session, Host/Origin, and CSRF protections.
- Private SQLite storage, versioned migrations, process locking, and verified backups.
- Bounded Virgin CSV upload retaining original bytes/hash and one observation per
  source record. Fatal file errors roll back; row-level problems remain visible.
- Source-account/reference identity, canonical overlap checks, and conflict quarantine.
- Paginated review with version-checked Personal/Joint edits and decision history.
- Restart persistence and idempotent reimport.

Merchant rules, bulk edits, historical migration, and execution arrived in later
increments. The original `/purchases` screen now redirects to `/review`.

## Import contract

`Transaction Currency` describes the original purchase; `Billing Amount` is GBP.
Foreign original currencies are preserved and compared on overlap. Strict amount,
date, header, encoding, and enum validation precedes storage.

Identity uses the [reference investigation](reference-investigation.md), with
leading zeros preserved. Missing/repeated references and conflicting fields become
exceptions. This increment used neither a posting-date cutoff nor automatic repair.

## Acceptance coverage

- Fresh/reopened/upgraded databases, unsupported schemas, and failed migration rollback.
- Same-file no-op, overlapping provenance, source-account scoping, and distinct
  references with otherwise identical details.
- Quoted commas, BOM, formatted amounts, foreign currency, missing dates/headers,
  malformed rows, and oversized uploads.
- Account persistence, invalid/stale edits, atomic rollback, and pagination.
- Session/Host/Origin/CSRF validation, escaping, HTMX updates, and ordinary forms.
- Browser upload/edit/restart/reimport using synthetic fixtures.

Local acceptance imported October 17 twice, then October 31, into a disposable
database. Aggregate findings are in the verification record; rolling-export counts
were not treated as outstanding payments.
