# Increment 1 verification

Historical record, validated locally on 2026-11-01. For current behavior, see the
[workflow guide](../single-queue-workflow.md).

## Automated checks

- `devenv shell -- recon-check`: formatting, Clippy (warnings denied), 25 Rust tests.
- `devenv shell -- recon-browser-check`: real headless Firefox through WebDriver,
  using only synthetic fixtures and a temporary private database.
- Browser coverage: bootstrap fragment removal/session exchange, CSV upload,
  exception summary, HTMX row update, refresh, app restart/session invalidation,
  persistent account choice, and identical-file reimport without duplicates.

Rust coverage includes source-scoped schema constraints, strict money/date parsing,
quoted commas/BOM, missing/repeated/conflicting references, foreign original
currencies, explicit non-purchase dispositions, fatal upload rejection, HTTP
Host/Origin/session/CSRF checks, request limits, HTML escaping, row fragments,
ordinary-form fallback, pagination, stale edits, restart, backup/restore, private
permissions, symlink rejection, unsupported schemas, failed migration rollback,
and full import rollback on a simulated database failure.

## Real October exports — temporary validation only

Imported October 17 twice, then October 31 through authenticated local HTTP into
a disposable `/tmp/opencode` database. Original files were read-only; the temporary
database and backups were removed afterward.

| Disposition | October 17 | October 31 |
| --- | ---: | ---: |
| Purchase observations | 672 | 606 |
| Missing reference | 1 | 1 |
| Malformed column count | 5 | 5 |
| Credit/refund | 6 | 4 |
| Fee | 1 | 1 |
| Payment | 19 | 17 |
| Pending | 7 | 8 |
| **Source records** | **711** | **642** |

- Reimport returned the same import ID: sequence `1, 1, 2`.
- Two persisted imports and 715 canonical purchases after overlap handling.
- Zero account decisions written, zero core-field conflicts, zero foreign-key errors.
- Counts match the reference investigation: 563 shared identified purchases and
  43 identified purchases in October 31 absent from October 17.
- These are full rolling histories, **not 715 outstanding payments**. The existing
  83-row October spreadsheet subset is intentionally not migrated in Increment 1.
