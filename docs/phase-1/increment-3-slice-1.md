# Increment 3 / Slice 1: Handling, recon scope, and totals

Status: implemented. Formatting, Clippy, and all 54 Rust tests pass. The expanded
Firefox smoke test is prepared but blocked by an existing app occupying port 7310;
the existing process was left untouched. Browser acceptance remains pending.

This is the first delivery slice of [Increment 3](increment-3.md), **not completion
of the whole increment**. It uses synthetic inputs only; no historical workbook,
real transaction import, credentials, or Monzo integration was accessed for it.

## What is available

- Named recon workspaces, explicitly populated with canonical purchase selections.
- Global, versioned handling decisions with reason/evidence notes.
- Preview/confirmation for recon creation, selection, handling, and archiving or
  reactivation. A later review/source/handling/scope change makes the preview stale.
- Checked integer-penny Personal/Joint totals for outstanding-ready members only.
- Handling/readiness filters and per-purchase handling/account/membership history.
- Archived recon status history and guarded reactivation.
- Schema migration 0004, with verified backup, append-only audit tables, and SQL
  guards for sequential versions, explicit reopening, and unique active membership.

## Handling states

| State | Meaning | Included in ready totals? |
| --- | --- | --- |
| Unknown | No confirmed evidence of whether the purchase was handled. This is the initial state, including all existing app purchases. | No |
| Outstanding | User explicitly confirms it still needs funding through recon. | Only if all other readiness conditions pass |
| Handled externally | Already funded outside this recon process, e.g. the partner moved its amount to the Monzo credit-card pot. This is the clarified meaning of legacy `Paid = Yes`. | Never |
| Excluded | Intentionally not part of outstanding funding, with a reason. Distinct from already-funded external handling. | Never |

Every recorded change needs a nonblank reason/evidence note. These are manual
assertions, not verified Monzo results or claims that the credit-card bill was
repaid. Existing account/category decisions, imports, and workspace selection do
not set handling automatically. No handling events are fabricated during upgrade.

Reopening a purchase that was **ever** externally handled or excluded requires
both an evidence note and the explicit reopening checkbox before confirmation.
Changing it to unknown first does not bypass that guard. A warning appears in the
preview; the old events remain visible in decision history.

## Workflow

1. Open **Recons**, enter a name, preview, and confirm creation. Names are unique
   after the same conservative case/whitespace normalisation used elsewhere.
   A new workspace starts empty; it does not select a rolling export implicitly.
2. Open **Review**. Select specific purchases on the current page, choose the
   target recon in the bulk form, provide a selection reason, and preview adding
   them. Confirm the exact affected rows. Batches remain limited to 100 distinct
   purchases; nothing on another page is implicitly selected.
3. Select the purchases whose handling you can confirm. Choose a handling state,
   supply evidence, preview, and confirm. External funding is global to the
   purchase, so a later import/workspace cannot make it outstanding by itself.
4. Complete account and merchant details as before. The workspace filter narrows
   the review to selected members; filters do not silently change that selection.
5. Open the recon's summary to inspect state counts and per-account ready totals.
   Use **Decision history** on a purchase to inspect its handling notes and scope
   changes. Status history appears on the recon summary.

The shared bulk reason field is required for handling/membership actions, not for
unrelated merchant or account edits; the backend enforces it for each action.

## Scope and readiness safeguards

A purchase belongs to at most one **active** recon, regardless of handling state.
To move it, explicitly remove it from the first selection or archive the first
workspace. Archived selections remain historical; they do not reserve purchases
against active work, and archiving never marks a purchase handled or paid.
Reactivation is blocked if it would duplicate another active workspace's selection.

Outstanding-ready requires:

- Confirmed outstanding handling.
- Explicit membership in this active recon.
- Personal or Joint allocation and nonblank category/description.
- A valid positive canonical GBP billing amount and no unresolved source conflict.

Summary counts partition the exact distinct member set into ready, outstanding
but blocked, unknown, externally handled, and excluded. Only ready amounts enter
the Personal/Joint sums; overflow is an explicit error, not a wrapped/partial total.
Archived summaries and ready filters cannot borrow another active recon's readiness.

Unknown handling now contributes to **Needs attention** even when merchant/account
details are complete. **Review-complete** still means details only. Per-row messages
explain blockers; filters include unknown/external/excluded/ready and recon scope.

Conflicted canonical purchases may receive handling/scope decisions, but stay
blocked for enrichment and readiness. Raw missing-reference/malformed/pending
observations cannot yet be put in a workspace or repaired. Inspect import summaries:
these are selected-canonical-purchase totals, not a fully executable recon.

## Verification

Rust tests cover:

- Unknown handling after schema 3 upgrade; existing account choices unchanged.
- No readiness inferred from complete details or scope alone.
- Correct Personal/Joint sums, partitioned counts, incomplete/conflicted rows, and
  overflow rejection.
- External/excluded handling surviving overlapping imports, new workspaces,
  restart, and snapshot/restore.
- Reopening confirmation, including attempted bypass through unknown state.
- Duplicate active scope, removal, archiving, and reactivation conflicts.
- Reasons, names, selection bounds, nonexistent records, stale/replayed previews,
  rollback of partial bulk changes, and append-only SQL constraints.
- Authenticated/CSRF/Origin-protected HTTP workflows and escaped names/evidence.

`recon-browser-check` now also exercises creating a recon, adding an explicit
selection, external/outstanding handling, totals, and restart persistence with
synthetic fixtures. Stop the running app before using it: ports 7310/4444 must be
free. The script never uses the regular app database or contacts Monzo.

## Remaining Increment 3 work

1. Staged historical account/detail/handling migration with explicit row matching.
2. Audited source-exception resolution and pending/billed links, including explicit
   exception scope for recon eligibility.
3. Account-rule suggestions, approval, precedence, provenance, and re-evaluation.
4. Guarded compatibility exports, immutable manifests, and external execution
   outcome/handoff recording.
5. Browser acceptance and separately approved real-data validation of those slices.

No approval/export/execution UI is available in this slice. Continue with historical
migration next, rather than interpreting unknown old history as outstanding.
