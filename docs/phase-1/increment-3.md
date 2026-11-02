# Increment 3: Historical handling, account rules, and recon totals

Historical plan. D026–D029 superseded its named-workspace and offline-only scope.
Use the [current workflow](../single-queue-workflow.md) for operation.

## Delivery status

| Original slice | Outcome |
| --- | --- |
| Handling, recon membership, and totals | [Slice 1](increment-3-slice-1.md) delivered; workspaces later became read-only history. |
| Historical decisions | Reference-matched CSV import delivered. Reference-free workbook matching remains unsupported. |
| Source-exception resolution | Dispositions and validated extra-column repairs delivered. General repairs and pending/billed linking remain open. |
| Account rules | Exact merchant and merchant+card rules delivered. Historical suggestions remain open. |
| Compatibility exports | Export manifests and external-run handoff remain open. Integrated Monzo execution was delivered instead. |

## Handling and migration requirements

- Record handling separately from account/category decisions. Unknown history
  cannot become payable through enrichment or import alone.
- Map verified legacy `Paid = Yes` to **handled externally**: the purchase was
  funded outside Recon, not necessarily repaid on the credit-card bill. Keep
  generic exclusion separate (D023).
- Preserve evidence and previous decisions. Reopening handled/excluded work needs
  explicit confirmation, including after an intermediate unknown state.
- Match historical rows by source reference. Missing-reference candidate matches
  require evidence; date, amount, merchant, and card similarity alone is insufficient.
- Preview proposals and totals, preserve explicit app decisions, and commit atomically.
  Copied workbooks must not count as independent evidence of purchases or payments.

## Undelivered design requirements

### General source repair

Retain original observations and append audited corrections. Preview changes to
identity, decisions, and totals. Distinguish bank references from any internal
identity used for verified reference-free purchases. Link pending/billed records
only with reliable evidence and count the purchase once.

### Account-rule suggestions

Use confirmed historical choices, deduplicated across exports/workbooks. Show
distinct-purchase support counts, conflicts, and affected rows. Choose a documented
evidence threshold; suggestions require approval. Precedence remains purchase
choice → merchant+card → merchant.

### Compatibility export and handoff

Export reviewed purchases as properly quoted `personal.csv`/`joint.csv` with
`date,amount,merchant,account,category,description` and fixed two-decimal amounts.
An immutable manifest should retain purchase IDs, decision/rule revisions, hashes,
totals, and provenance. Exporting does not establish payment.

Reserve handed-off work until external outcomes are reconciled per purchase,
including partial/uncertain deposits and failed notes. Integrated execution must
respect this state before allowing another deposit.

## Acceptance requirements

Historical migration must be repeatable and provenance-preserving. Tests must
cover ambiguous matches, reopening, stale previews, rollback, rule precedence,
and protection of externally handled purchases across import/restart/restore.
Future repairs must preserve originals; future handoffs must block duplicate funding.

The original local comparison used the October 17 selected totals recorded in the
[Phase 1 plan](../phase-1-import-and-review.md#original-migration-acceptance), subject
to reconfirming funding status at migration time.
