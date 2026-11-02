# Phase 1: Offline import and review

Historical plan. Current operation is documented in the
[workflow guide](single-queue-workflow.md).

## Outcome

Import Virgin CSVs into a persistent ledger, reuse approved decisions, and review
exceptions. Keep review completeness separate from evidence that funding is needed.

## Delivery

| Increment | Delivered scope | Record |
| --- | --- | --- |
| 1 | Import, source identity, individual account edits, persistence, reimport. | [Plan](phase-1/increment-1.md) · [verification](phase-1/increment-1-verification.md) |
| 2 | Merchant catalogue, defaults/aliases, purchase overrides, bulk review. | [Plan](phase-1/increment-2.md) · [verification](phase-1/increment-2-verification.md) |
| 3 | Handling and named workspaces, later superseded by one queue, staging, exact account rules, and reference-matched historical import. | [Plan](phase-1/increment-3.md) · [first slice](phase-1/increment-3-slice-1.md) |

Recognized extra-column repair and a permanent historical cutover were added later.
General source repair, pending/billed linking, account-rule suggestions, and
compatibility exports remain undelivered parts of the original plan.

## Core requirements

- Retain original bytes, hashes, and every source observation, including exceptions.
- Identify purchases by guarded source-account/reference identity; preserve genuine
  identical-looking purchases and quarantine conflicts.
- Parse money as integer pennies and distinguish original currency from GBP billing.
- Preserve explicit decisions across overlapping imports and default changes.
- Preview bulk changes and commit atomically with stale-version checks.
- Keep handled/excluded history out of payable work; account assignment alone
  cannot establish outstanding status.
- Verify migrations, restart persistence, backup/restore, and local HTTP protections
  with synthetic data.

## Original migration acceptance

The October 17 workbook contained 83 selected rows with Personal £393.89 and Joint
£1,607.89. The user confirmed it was unexecuted at planning time (2026-11-01).
October 31's workbook was a copy; its fresh transaction export was independent.

Those totals were a migration comparison, not a permanent payment instruction.
Any later migration must use current funding evidence and explain discrepancies.
`lastpostingdate.txt`, directory dates, and `Paid = No` do not prove eligibility.
