# Increment 2: usage and verification

Implemented 2026-11-01. No real merchant/workbook data was accessed or imported.

## Workflow

Open **Review** after uploading an original Virgin export. The earlier Allocations
page remains available for individual HTMX account edits.

- Filter by merchant substring, account, missing field, source conflict, import ID,
  needs-attention, or review-complete. Counts describe distinct purchases in the
  filtered view; missing-field counts overlap. Payment history remains unknown.
- Select individual purchases on the current 50-row page. Preview Personal/Joint/
  unset changes or category/description overrides, then confirm. There is no hidden
  cross-page selection. Backend batches are capped at 100 distinct purchase IDs.
- Selected override changes replace both override fields. Blank clears that field
  back to the current merchant default. Explicit overrides survive default changes,
  reimports, and restart. Source-conflicted purchases cannot receive selected edits.
- Click a merchant or a missing-details group to search its purchases and prefill
  its current default. Saving a default previews all affected purchases, including
  preserved overrides. Matching uses version 1 Unicode lowercase and collapsed
  whitespace; punctuation, processor prefixes, and digits remain significant.
- Create an exact alias from a new variant name to an existing merchant default.
  Preview the affected purchases before approval. Alias chains and replacing an
  existing definition with an alias are rejected. Aliases are append-only in this
  increment; no alias correction/removal interface is provided.

## One-time merchant sheet export

Open a copy of the selected legacy workbook read-only. Export the merchant sheet
(or a copy containing only the desired columns) as UTF-8 CSV with one header row:
`Merchant,Category,Description`. Export formula cells as their displayed values,
not formula expressions. Use standard CSV quoting for commas, quotes, and newlines.
Do not overwrite the original workbook or put the export in this repository.

Upload under **Import merchant catalogue**. Inspect every normalized-name conflict.
Identical definitions are coalesced; conflicting rows start unchecked. Choose one
definition explicitly or leave that merchant unchecked to skip it. Incomplete
rows retain missing fields. Existing different defaults are never selected for
replacement automatically. The affected-purchase list conservatively includes all
merchant names in the file; only selected definitions are committed.

Approve new categories explicitly on confirmation. Existing case/whitespace
variants resolve to the same local category. Category values come from the approved
legacy export or deliberate additions, not a remote API list. No values are seeded
from private files automatically.

Original catalogue bytes, hash, filename, record ordinals, and selected definition
provenance are retained. An identical file reimport is a no-op, preserving later
edits; skipped definitions are not reconsidered automatically on reimport. Use an
explicit merchant-default edit or a revised CSV to reconsider them.

## Persistence and concurrency

Migration 0003 adds append-only merchant definitions, aliases, categories, source
archives, purchase overrides, and audited previews. Merchant and purchase-detail
versions are sequential. Existing source records and account versions are untouched.
Previews record the complete proposed change and global review revision. Any
intervening review/source change makes confirmation stale, including across restart.
Confirmation runs atomically; failure retains the preview and rolls back all edits.
Confirmed previews cannot be replayed or rewritten. Backups include all new data;
review commits take a verified snapshot before mutation.

## Verification

Formatting, Clippy with warnings denied, and all 35 Rust tests pass. New coverage:

- Catalogue parsing, normalization collisions, identical rows, conflicting choices,
  provenance, category approval, later edits, and idempotent reimport.
- Effective defaults, per-purchase overrides, clearing, aliases, overlap, restart,
  migration from schema 2, and backup/restore.
- Missing-field counts, conflict blocking, import membership, pagination, explicit
  selection bounds, stale preview rejection, and atomic bulk account changes.
- Injected database failures during bulk and catalogue commits roll everything back.
- HTTP authentication, CSRF/Origin protections, blank filters, escaped preview
  values, confirmation, replay rejection, and stale multi-row submissions.

The Firefox smoke script now includes catalogue upload/coalescing, bulk allocation,
complete-view filtering, purchase overrides, default changes preserving overrides,
clearing overrides, and restart persistence, alongside the original HTMX checks.
It could not run in this restricted environment: geckodriver returns
`Operation not permitted` when binding its local listener. Run
`recon-browser-check` in the normal development shell before declaring browser
acceptance complete. The extended browser assertions remain unverified here.

Separate real-data validation from the plan remains pending: use a disposable
database and a selected merchant export, report aggregate coverage/conflicts, and
compare samples locally. No regular database was seeded. No credentials, handling
states, exports, account rules, payment readiness, or Monzo operations were added.
