# Phase 1: Offline import and review

## Outcome

Import CSVs into a persistent ledger, reuse known decisions, and display a review
queue. No Monzo credentials or money movement are required.

## Work packages

Implementation sequence:

1. [Increment 1: Import, review, persist](phase-1/increment-1.md) — implemented.
2. [Increment 2: Merchant enrichment and exception review](phase-1/increment-2.md)
   — implemented; review completeness only, no payment-history inference.
   Extended Firefox and real-data acceptance remain outstanding.
3. [Increment 3: Historical handling, account rules, and recon totals](phase-1/increment-3.md)
   — in progress; [Slice 1](phase-1/increment-3-slice-1.md) adds handling, explicit
   scope, and totals. Migration/rules/exception repair/exports remain planned;
   there is still no Monzo access or money movement.

### 1. Domain and storage

- Add versioned SQLite migrations and a storage layer.
- Model imports, immutable source observations, canonical purchases, merchant
  details, account rules, review decisions, exclusions, and provenance.
- Preserve transaction date, posting date, reference, debit/credit indicator,
  card identifier, currency, and original merchant text.
- Keep decision readiness separate from financial execution state. A row may
  lack several decisions at once; queue counts must not imply disjoint groups.
- Represent manually paid and excluded rows explicitly, with reason/history.
- Define backup/restore and database migration failure behavior.

### 2. Robust Virgin import

- Preserve original input and file hash; validate headers and row shapes.
- Port useful format knowledge from `monzo-cli`, not its blanket comma-splitting
  repair. Handle quoted CSV correctly and flag uncertain repairs.
- Parse amounts strictly into pennies, rejecting ambiguous values and unsupported
  currencies rather than guessing.
- Import billed purchases for review. Report pending items separately and promote
  them safely if later exports show them billed.
- Surface refunds, fees, payments, malformed rows, and unsupported formats in an
  import summary with explicit dispositions. Never silently lose an input row.
- Investigate reference-number uniqueness/stability on real exports before
  selecting an identity scheme. Scope identifiers to the source account where
  needed; never use amount/merchant/date alone as proof of identity.
- Deduplicate repeated and overlapping imports across recon directories. Flag
  missing/ambiguous references and conflicting observations for review; preserve
  distinct genuine identical purchases.
- Use dates as optional import hints, not the sole duplicate/eligibility filter.

### 3. Merchant catalogue and account rules

- Read the latest legacy workbook's merchant sheet through a read-only importer
  or a user-exported CSV. Preserve category/description and flag conflicting
  duplicate merchant definitions.
- Conservative case/whitespace matching; explicit aliases/patterns only.
- Keep merchant descriptions/categories separate from account assignment rules.
- Suggest account rules from historical decisions, including merchant + card
  combinations; do not activate suggestions without approval.
- Apply explicit transaction overrides ahead of approved defaults. Record which
  rule supplied each field. Do not overwrite manual decisions on reimport.
- Offer transaction-only and remembered-rule actions. Preview the affected rows
  before bulk updates or rule changes.

### 4. Local review UI

- Use the agreed Axum + Askama + HTMX stack; see `decisions.md`.
- Implement needs-attention, ready, already-handled, and all-transactions views.
- Show date, amount, merchant, card, account, category, description, handling state,
  warnings, and decision provenance.
- Support filtering, bulk account decisions, merchant-group editing, explicit
  paid/excluded decisions, and persistent save/resume.
- Unknown account, category, or description prevents readiness unless an explicit
  non-payment disposition resolves the row.
- Offer per-account counts/totals and an auditable preview of selected rows.
- Establish loopback/session/CSRF protections before serving real data.

### 5. Legacy transition and exports

- Import October 17's reviewed decisions as unexecuted, as confirmed by the user.
- Do not interpret the copied October 31 workbook as new source transactions.
- Treat historical execution as unknown unless explicitly confirmed; quarantine
  old-history imports from the payable queue by default.
- Reconcile imported decisions against source references where possible. Legacy
  sheet rows without references require explicit resolution of ambiguous matches.
- Generate compatible personal/joint CSVs automatically, with warnings that the
  legacy runner is not safely resumable. Exporting never marks rows deposited.
- If the user executes an export externally, require recording that outcome before
  those rows can be selected for the later integrated executor.

## Acceptance criteria

- Reimporting the same file changes neither purchase count nor saved decisions.
- Overlapping October files retain previous decisions and show genuinely new rows.
- Distinct identical-looking purchases survive import; ambiguous matches are shown.
- Every source row is accounted for in the import summary.
- Resolve an unknown merchant once and update its selected matching transactions.
- Restarting the app preserves all review state.
- October 17's imported totals match the existing workbook (Personal £393.89,
  Joint £1,607.89) or discrepancies are explained before proceeding.
- Automated tests use synthetic fixtures for malformed CSV, quoted commas, amount
  parsing, overlapping imports, pending-to-billed transitions, rule precedence,
  migrations, and validation. No live Monzo endpoint can be invoked in this phase.

## First vertical slice

Detailed scope and acceptance tests: [Increment 1](phase-1/increment-1.md).

Import a synthetic CSV twice, list one deduplicated review queue, edit one account
decision, restart, and verify the decision persists. Then validate the same flow
against the real October data locally.
