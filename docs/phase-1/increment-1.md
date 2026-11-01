# Phase 1 / Increment 1: Import → review → persist

Status: implemented and verified. See [verification](increment-1-verification.md)
for automated checks, browser smoke coverage, and temporary real-data validation.

## Goal

Start the local app, upload a Virgin CSV, inspect purchases, assign Personal/Joint,
restart, and reimport without losing decisions or duplicating purchases.
This is a complete offline vertical slice, not a payment-capable recon yet.

## Prepared inputs

- [Decision log](../decisions.md), including the selected HTMX stack.
- [Reference investigation](reference-investigation.md) and guarded identity policy.
- [Schema contract](schema.md) and `migrations/0001_initial.sql`.
- [Development/security defaults](../development.md).
- In-memory SQL constraint tests and repeatable `recon-check`/`recon-dev` commands.

## In scope

1. **Bootstrap:** Axum + Tokio, Askama, rusqlite, CSV parsing, SHA-256 hashing,
   secure random/session primitives, structured errors, and locally vendored HTMX.
   Pin resolved dependencies in Cargo.lock. Never block Tokio workers with SQLite
   or parsing work; use a bounded blocking boundary for this small single-user app.
2. **Storage:** source-account bootstrap, private data directory, transactional
   migrations, connection configuration, schema-version checks, and backup support.
   No live data is created merely by tests or a development shell starting.
3. **Import:** bounded file upload (10 MiB initial limit); preserve exact bytes and
   hash. Validate headers, encoding, row shapes, dates, strict GBP billing amounts and
   enum values. A fatal file/header error rejects the import with no partial state;
   recoverable row issues are committed as explicit observations/exceptions.
   Virgin's `Transaction Currency` describes the original purchase, not necessarily
   the currency of `Billing Amount`. Preserve it; do not label the billed amount
   USD or reject a foreign purchase merely because the original currency is USD.
   Confirm the export's GBP billing semantics before the real-data validation.
4. **Identity:** source-scoped references, identical-file no-op, overlapping import
   provenance, and conflict quarantine as described in the investigation. No
   legacy posting-date cutoff and no speculative comma repairs in this increment.
5. **Review:** paginated purchase table (50 rows/page), date/amount/merchant/card/
   reference, current account choice, and an unset/Personal/Joint control. Include
   an exceptions summary with source row numbers and safe diagnostic reasons.
6. **Edits:** append review events; update a row fragment through HTMX, with normal
   form redirect fallback. Use expected decision versions to reject stale writes
   (409 with reload guidance). Preserve account decisions on reimport.
7. **Recovery:** restart with persisted imports and decisions; clear errors for
   unavailable data directories, migrations, malformed uploads, and database locks.

## Explicitly out of scope

Merchant categories/descriptions/rules, bulk editing, historical spreadsheet
decisions migration, pending-to-billed identity reconciliation, resolving ambiguous
identities, CSV exports, approval, Monzo calls, tokens, and money movement.
Do not add routes or dependencies for a future executor now.

## HTTP sketch

- Bootstrap/session exchange via a non-sensitive landing page and URL fragment;
  remove the token from browser history immediately after exchange.
- `GET /`: authenticated dashboard/import form.
- `POST /imports`: protected multipart upload; redirect to persisted import result.
- `GET /imports/{id}`: counts, exceptions, and link to purchases.
- `GET /purchases?page=…`: full template or HTMX table fragment.
- `POST /purchases/{id}/account`: CSRF-protected, version-checked account decision;
  fragment response or redirect.
- `/assets/*`: local CSS and pinned HTMX; no third-party network dependencies.
- `/health`: no sensitive data, process liveness only.

Route details may change; authorization, escaping, upload bounds, and consistent
full-page/fragment behavior are acceptance requirements, not optional polish.

## Test matrix / definition of done

- Fresh temporary database migrates once; restart is a no-op; unknown/newer schema
  is refused. Migration failure leaves the previous version intact.
- Same file/source imported twice: one import, unchanged purchase count/decisions.
- Overlap file adds observations/new purchases, preserving previous choices.
- Same reference under a different source account remains distinct.
- Distinct references with identical date/merchant/amount remain distinct.
- Missing/repeated/conflicting references and malformed rows are visible exceptions.
- Every input row has exactly one persisted observation/disposition; summaries
  reconcile. No malformed row is silently repaired or ignored.
- Quoted commas, leading-zero references, formatted amounts, null posting dates,
  unsupported currency, bad dates, missing headers, and oversized uploads are tested.
- Account assignment survives reconnect/restart; invalid values and stale form
  versions cannot overwrite decisions.
- Unauthenticated, wrong Host/Origin, missing/invalid CSRF, oversized requests,
  and HTML/script merchant text are covered by HTTP tests.
- A browser smoke test confirms upload, fragment edit, refresh, restart, and
  duplicate import. Test fixtures are synthetic and visibly non-real.
- `recon-check` passes; no calls to Monzo or access to legacy files in automated tests.

## Real-data validation (explicit local action)

After the synthetic suite passes, use a separate local database to import October
17, repeat it, then import October 31. Check the aggregate reference findings and
visible exceptions. The full rolling exports include older history; do not expect
the 83-row selected spreadsheet total or assume any row is payable. Do not import
spreadsheet decisions or mark old rows paid during this increment. Leave original
files unchanged and keep real data out of fixtures/logs/screenshots committed to Git.
