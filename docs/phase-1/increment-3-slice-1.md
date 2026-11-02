# Increment 3 / Slice 1: Handling, recon scope, and totals

Historical delivery record. Named workspaces are now read-only; use the
[single-queue workflow](../single-queue-workflow.md) for current actions.

## Delivered scope

- Named workspaces with explicit purchase membership and status history.
- Global, versioned handling decisions with evidence notes.
- Preview/confirmation for membership, handling, archive, and reactivation.
- Checked Personal/Joint totals for outstanding-ready members.
- Migration 0004 with append-only events, sequential versions, reopening guards,
  and unique active membership.

## Original handling model

| State | Meaning | Included in ready totals? |
| --- | --- | --- |
| Unknown | No confirmed funding status. | No |
| Outstanding | Confirmed to need funding through Recon. | If other readiness checks pass |
| Handled externally | Funded outside Recon; the meaning of legacy `Paid = Yes`. | No |
| Excluded | Intentionally omitted with a reason. | No |

Handling changes required evidence. Reopening any previously external/excluded
purchase required confirmation, even after an intermediate unknown state.
Imports, account/category edits, and workspace membership did not establish funding
status. Handling history persisted globally across workspaces.

Original readiness required outstanding handling, active workspace membership,
Personal/Joint allocation, category/description, a positive GBP purchase, and no
source conflict. A purchase could belong to only one active workspace. Archiving
released membership without settling purchases; conflicting reactivation was blocked.

## Verification at delivery

Formatting, Clippy, and 54 Rust tests passed. Firefox smoke checks passed for
import, HTMX edits, merchant review, restart, explicit membership, external handling,
and totals, using synthetic data.

Rust coverage included schema-3 upgrade, unknown initial handling, readiness and
overflow, overlap/restart/restore persistence, reopening guards, active-membership
conflicts, stale/replayed previews, rollback, audit constraints, and HTTP protections.

This delivered the first slice. See the [Increment 3 status](increment-3.md#delivery-status)
for subsequent work.
