# Phase 1 / Increment 3: Historical handling, account rules, and recon totals

Status: planned, not implemented. Depends on
[Increment 2](increment-2.md) and the existing identity/ledger safeguards.

## Goal

Distinguish outstanding purchases from already-handled history, migrate reviewed
legacy decisions safely, and suggest useful account rules. Produce an auditable
offline recon with trustworthy per-account totals—not merely a complete-looking
table of a rolling transaction history.

No Monzo calls, credentials, live approval, or payment execution are introduced.

## 1. Explicit handling states and recon scope

- Add append-only handling decisions, separate from account allocation and merchant
  details. Initial history is **unknown**, including purchases already in the app.
- Support confirmed outstanding, handled manually/externally, and excluded-with-
  reason states. Record provenance, time, and supporting note/evidence. Unknown
  history is never payable; category/account completion cannot change this state.
- Legacy `Paid = Yes` means the purchase was handled outside recon, as clarified
  by the user: for example, their partner already moved the purchase amount into
  the Monzo credit-card pot. It does not mean the credit-card bill itself was repaid.
  Record a confirmed match as handled externally and exclude it from outstanding
  totals, executable exports, and deposits in every recon. Preserve the purchase
  and audit history; do not conflate this with a generic exclusion. See D023.
- Add an explicit named recon workspace/selection. Its scope is a persisted set of
  canonical purchases, not all rows from a rolling export or a posting-date cutoff.
  Import membership/date filters may help selection but cannot confirm eligibility.
- Prevent a purchase from appearing in multiple active recon selections as separate
  payable work. Preserve identity globally across imports/workspaces. Existing
  October overlap is one purchase, not one payment per directory.
- Corrections append an audited superseding decision and invalidate affected
  summaries/exports. Reopening handled work requires an explicit warning/reason;
  generic edits or reimports cannot silently make it outstanding again.

## 2. Read-only historical decision migration

- Define and inspect a read-only input adapter for legacy workbook sheets or
  bounded exported CSVs. Preserve source file/hash/sheet/row and a preview manifest.
  Use a consistent input route with documented handling of formula-derived values.
- Stage matches, unmatched rows, ambiguities, duplicate selections, and conflicting
  decisions before committing. Never import source directories on app startup.
- Prefer a source-account-scoped reference when available. Legacy sheet rows
  without references require candidate matching using date, pennies, merchant,
  card, and raw-export provenance; a unique-looking fingerprint is a suggestion,
  not proof. Require user confirmation before applying those mappings; never
  collapse genuine identical purchases or fabricate a bank reference.
- Migrate account/category/description decisions independently from handling state.
  Existing explicit app overrides win unless the user approves a specific replacement.
- The user confirmed October 17 was prepared but unexecuted on 2026-11-01. Reconfirm
  this at migration time; execution may have happened since. The October 31 workbook
  copied from October 17 is not independent evidence of new decisions or payments.
- `Paid = No`, exported batch files, directory dates, and `lastpostingdate.txt` do
  not prove outstanding status. `Paid = Yes` is evidence of external handling,
  not payment of the credit-card bill; confirm the source match and provenance
  before applying that handling decision. Older history without confirmed evidence
  stays unknown unless explicitly resolved.
- Preview every state/account change and totals before an atomic, version-checked
  commit. Repeat migration is idempotent; conflicting revisions cannot overwrite
  decisions silently. Support audited correction rather than deleting provenance.

## 3. Audited source-exception resolution

- Extend the exception ledger with resolution events while retaining immutable raw
  inputs and prior dispositions. Never disable immutable-source triggers to repair
  a row or rewrite an already-applied migration.
- Offer controlled manual linking of missing/repeated references, explicit
  separate-purchase creation with an internal identity where justified, and
  superseding canonical interpretations for proven corrections. Keep bank references
  distinct from internal IDs; preview affected decisions and totals before commit.
- For malformed rows, show an escaped local source excerpt and allow a user-verified
  corrected interpretation, with field validation and a complete audit trail.
  No blanket comma repair or silent financial-field overwrite.
- Pending observations remain non-payable. Link them to later billed observations
  only with reliable identity/evidence; otherwise require confirmation. Pending
  and billed views must not count the same purchase twice.
- Credits/refunds/fees remain visible blockers requiring an explicit supported
  disposition. Exclusion may be confirmed with a reason; automatic refund netting,
  pot withdrawals, or pretending these are positive purchases is out of scope.
- A recon containing unresolved selected source exceptions is not exportable.
  Exceptions outside the selected scope remain visible without making unrelated
  reconciliations impossible. Excluding an exception from scope must be deliberate.

## 4. Approved account-rule suggestions

- Analyse confirmed historical account decisions as evidence for merchant-only or
  merchant + card rules. De-duplicate copied workbooks/rolling exports so repeated
  observations are not counted as independent purchases supporting a rule.
- Show distinct-purchase support counts, conflicting outcomes, recent examples,
  and proposed affected rows. Low evidence or mixed allocation remains unresolved.
  Choose/document a conservative suggestion threshold during implementation;
  no suggested rule activates itself.
- Initial rule language is exact merchant identity plus optional exact card. Defer
  regex/fuzzy/amount-based rules unless a separate decision justifies them.
- Precedence: explicit purchase choice, approved merchant + card rule, approved
  merchant rule, then unset. Equal-precedence conflicts require review, not first
  or last match. Keep account rules separate from merchant enrichment definitions.
- Default application fills unset decisions only. Preview any proposed replacement
  of an earlier derived decision; preserve explicit overrides. Version rules and
  record the rule revision behind every derived decision.
- Disabling/correcting a rule must show affected unsubmitted purchases and support
  an audited re-evaluation; never erase historical decisions or infer payment status.

## 5. Readiness, totals, and compatibility exports

- Provide needs-attention, review-complete, outstanding-ready, already-handled,
  excluded, and all-purchases views. Explain why a row is blocked.
- Outstanding-ready requires confirmed outstanding handling, Personal/Joint,
  nonblank category/description, valid positive GBP billing pennies, resolved
  identity/source issues, and explicit recon membership. It is not a Phase 2
  payment approval or evidence of a successful deposit.
- Calculate distinct-purchase counts and checked integer-penny sums per account
  for the selected recon. Display ready, blocked, handled, excluded, and unknown
  counts separately; do not silently omit invalid rows from a supposedly complete
  total. Keep missing-field counts distinguishable from distinct row counts.
- Generate compatible `personal.csv`/`joint.csv` only from a reviewed explicit
  selection with no unresolved eligibility blockers. Include the legacy fields
  `date,amount,merchant,account,category,description`, using proper CSV quoting and
  fixed two-decimal amounts. Warn that notes/categories are local export data and
  the old runner does not set Monzo categories.
- Create an immutable export manifest containing exact purchase IDs, decision/rule
  revisions, content hashes, per-account totals, and audit provenance. Exporting
  is not execution and must not mark rows paid.
- Treat an exported selection as handed off to the legacy executor until its outcome
  is explicitly reconciled. Warn against rerunning a batch; require per-purchase
  completed/failed/uncertain outcome recording, including partial batches and
  deposits whose note failed. Unknown outcomes block a new executable handoff.
  Phase 2 must respect this handoff state before offering integrated execution.
- Do not try to make the legacy runner safely resumable here. CSV export is an
  explicit compatibility escape hatch, not the recommended final executor.

## Storage and delivery order

Use numbered, backed-up, transactional migrations for handling events, recon
membership, historical migration provenance, exception resolutions, versioned
account rules, and export handoff records. Keep domain eligibility logic independent
of templates and share it between UI summaries and exporters.

Suggested slices:

1. Handling state machine, recon membership, readiness predicates, and totals.
2. Staged historical decision migration and explicit matching/confirmation.
3. Audited exception resolution and pending/billed links.
4. Account-rule suggestions, approval, and provenance.
5. Guarded compatibility exports and external-outcome recording.

Each slice needs synthetic tests before using real history. Keep the existing
loopback/session/CSRF protections, bounded workers/uploads, escaping, stale-edit
checks, and ordinary-form/HTMX paths. No Node or external financial services.

## Acceptance tests / definition of done

- Fully categorised history with unknown handling is never outstanding-ready or
  exportable. Account rules and reimports cannot change handling state.
- Historical migration is repeatable, previewed, provenance-preserving, and atomic.
  Ambiguous/missing-reference matches cannot silently apply decisions.
- Copied workbooks do not duplicate purchases, inflate rule evidence, or prove
  execution. Old handled rows cannot reappear as payable after overlap/restart.
- A matched and confirmed legacy `Paid = Yes` purchase is handled externally and
  never contributes to outstanding totals or executable exports, including after
  reimport or selection in another recon. `Paid = No` alone proves no handling state.
- Exception corrections preserve originals, create no duplicate canonical purchase,
  and show any affected readiness/totals. Pending/billed links count once.
- Rule approval/precedence, conflicts, disable/re-evaluation, and explicit override
  protection are covered by tests and visible in the UI.
- Recon totals match the exact selected distinct purchases; checked arithmetic
  rejects overflow. Unknown, excluded, and handled history never leaks into exports.
- Export bytes/manifest match the preview; exports do not mark payments complete.
  Partial or uncertain external outcomes block unsafe repeat handoffs and remain
  correct after restart and backup/restore.
- Rust and Firefox tests cover migrations, preview conflicts, state transitions,
  selection overlap, rules, exception resolution, totals, exports, and handoff.
  `recon-check` and `recon-browser-check` pass with synthetic inputs only.

## Real-data acceptance and Phase 2 boundary

With current user confirmation and a disposable database, migrate October 17's
selected 83 rows and compare Personal £393.89 / Joint £1,607.89. If the source or
handling has changed, explain/reconcile differences rather than force old totals.
Import October 31's fresh CSV and verify overlap preserves decisions without
mistaking the copied workbook for newly reviewed work. Require explicit decisions
for any historical handling unknowns and selected source exceptions.

No live batch needs to be run to validate Phase 1. Do not mutate the legacy files,
obtain credentials, contact Monzo, or seed the regular database without explicit
approval. Phase 2 begins only after eligibility, historical/external outcomes,
and the separate stable-deduplication/correlation execution design are validated.
