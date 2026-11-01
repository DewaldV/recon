# Phase 1 / Increment 2: Merchant enrichment and exception review

Status: implemented; Rust checks pass. Extended Firefox verification and separate
real-data acceptance remain outstanding. See [usage and verification](increment-2-verification.md).
Builds on [Increment 1](increment-1.md).

## Goal

Import the existing merchant knowledge, automatically fill known categories and
descriptions, and concentrate review on missing decisions. Resolve an unfamiliar
merchant once rather than edit every purchase separately.

This increment improves review completeness only. It introduces no paid/unpaid
inference, payable queue, payment approval, or Monzo access.

## In scope

### 1. Merchant catalogue import

- Inspect the latest selected workbook's merchant sheet read-only, including
  repeated names, conflicting definitions, empty fields, and formula cells.
- Support a bounded UTF-8 merchant CSV import with `Merchant`, `Category`, and
  `Description` columns. A user-exported CSV is the initial supported path; do not
  require an ODS reader or a running LibreOffice process. Document the one-time
  export procedure; any inspection helper must remain read-only and off the
  normal startup/test path.
- Preview inserts, unchanged rows, incomplete definitions, and conflicts before
  committing. Coalesce identical definitions; require explicit resolution of
  conflicting definitions rather than choosing the first or last row.
- Preserve source bytes/hash, source record, original name, and import provenance.
  Reimport is idempotent and must not replace later user edits silently.

### 2. Merchant matching and field provenance

- Use exact matching after conservative case/whitespace normalisation. Preserve
  the original merchant text; version the normalisation policy.
- Do not strip arbitrary digits, payment-processor prefixes, or punctuation.
  Explicit user-approved exact aliases may connect known variants. Fuzzy/pattern
  matching and account-rule suggestions are outside this increment.
- Keep category/description defaults separate from Personal/Joint allocation.
- Store versioned merchant definitions and explicit per-purchase overrides, with
  field-level provenance. Resolve each field as explicit override, approved
  merchant default, or missing. Allow an override to be cleared back to the default.
- Editing a catalogue definition must preview its affected purchases. Do not
  overwrite transaction overrides. Record an audit event and refresh affected
  review counts; later payment approvals must not rely on mutable defaults.
- Treat blank/whitespace-only values as missing. Preserve Unicode and emoji.
  Use a local category catalogue seeded from legacy values, not the API's limited
  category list; allow deliberate additions while preventing accidental variants.
- Enrich both existing and newly imported purchases without altering source
  observations, purchase identity, original amounts, or account decisions.

### 3. Exception-focused review UI

- Add needs-attention, review-complete, and all-purchases views, with search and
  filters for merchant, account, missing field, and import membership.
- Include account, category, description, and provenance in the table/detail view.
  Import membership is a convenience filter, not evidence of unpaid spending.
- Show an overall distinct-purchase count and separate missing-field counts; a
  purchase can contribute to several reasons without being counted twice overall.
- Keep identity/source conflicts visible and blocked. A complete set of editable
  fields must not conceal an unresolved source exception.
- Label review-complete clearly: **details complete; payment history unknown**.
  It must not be called ready to pay or included in a payable total.
- Group unknown merchants for editing. Offer explicit actions for selected
  purchases only, saving a merchant default, or creating an exact alias.
- Show the proposed values, number of affected purchases, and preserved overrides
  before a multi-row/default change. Refresh the preview if underlying state changes.

### 4. Bulk Personal/Joint assignment

- Support explicit selection and Personal/Joint/unset assignment with a preview.
  Default selection is the current page; never silently select every matching row
  across pages or every historical purchase from a merchant.
- Send explicit purchase IDs and expected versions. Validate the whole selection
  and commit atomically; stale, missing, or conflicted rows reject the operation
  without partially applying it. Bound batch size and show conflicts for retry.
- Append the same audited account decisions used by individual edits. This does
  not create a remembered account rule; that capability belongs to Increment 3.

### 5. Storage, security, and migration

- Add numbered migrations; never rewrite migrations 0001/0002 or backfill
  fabricated merchant/payment decisions into the existing purchase records.
- Preserve existing account decision versions and append-only source provenance.
  Apply backups and transactional migration rules from Increment 1.
- Keep all import/preview/commit routes authenticated, CSRF/Origin protected,
  bounded, parameterised, and escaped. Preserve ordinary-form fallback and clear
  HTMX stale-preview/error feedback. No Node tooling or external asset services.

## Explicitly out of scope

Historical account/payment migration, paid/excluded states, account rules,
resolving missing/colliding references or malformed Virgin rows, refund treatment,
pending-to-billed reconciliation, recon eligibility/totals, CSV batch exports,
Monzo credentials, approval, and execution.

## Suggested delivery order

1. Definition/override schema and tested effective-field resolution.
2. Catalogue CSV preview/import with provenance and conflict handling.
3. Enrichment of existing/new purchases and single-merchant editing.
4. Needs-attention filters, grouped review, and explicit bulk account changes.
5. Migration/restart/browser regression checks, then separate local validation.

## Acceptance tests / definition of done

- Importing the same catalogue twice produces no duplicate definitions or changes
  to explicit edits; conflicting normalised names require a decision.
- Unknown merchant details can be supplied once and applied to the previewed
  purchases. Emoji, quoted commas, and HTML-like names remain safe and intact.
- An approved default enriches later imports and existing purchases; a transaction
  override wins and survives catalogue changes, overlapping imports, and restart.
- Clearing an override restores the current approved default with correct provenance.
- Merchant category/description decisions never change account assignment.
- Filters/counts agree when one purchase lacks multiple fields. Review-complete
  remains explicitly separate from any payment status.
- Bulk changes update exactly the selected rows; a stale version or blocked row
  rolls back the complete operation. No hidden cross-page selection occurs.
- Failed previews/imports/migrations leave prior data intact. Backup/restore keeps
  catalogue history, overrides, and existing account decisions.
- Synthetic Rust and Firefox tests cover normalisation collisions, import conflicts,
  field precedence, group changes, stale previews, bulk rollback, escaping, and
  session/CSRF protections. Existing Increment 1 tests continue to pass.
- `recon-check` and the extended `recon-browser-check` pass without real files or
  network access to financial services.

## Real-data validation

After synthetic checks pass, use a disposable database and a user-selected export
of the latest merchant sheet. Report aggregate matched/missing/conflicting counts;
do not assume the old plan's claimed match rate still applies. Compare a small
sample with the workbook locally. Keep catalogue contents and financial data out
of source control, logs, and test fixtures. Do not infer historical payment status
or seed the regular application database without a separate explicit action.
