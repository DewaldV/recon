# Virgin reference-number investigation

Read-only inspection on 2026-11-01. No real reference values, merchant details,
amounts, or raw files are copied into this repository.

## Method and limits

Inspected 26 `*/transactions.csv` files with a `Reference Number` header beneath
`~/Documents/financial/recons`, using a standard CSV reader. Compared only
well-shaped rows (exact header field count, no missing fields). A billed debit
purchase means `Status=BILLED`, `SICMCC Code=PR`, `Debit or Credit=DBIT`.
Amounts were compared numerically after removing thousands separators; references
were compared as strings. Other filenames/exports were not part of this sample.
Malformed rows were counted, not repaired or included in identity conclusions.

## Findings

- 2,767 distinct nonempty billed-debit purchase reference groups.
- 2,373 appear in more than one export.
- No duplicate nonempty billed-debit purchase references within an inspected file.
- Shared purchase references have matching transaction dates, merchants, card
  identifiers, currencies, and numeric amounts within this sample.
- 38 groups have textual amount-format changes only; raw string comparison would
  incorrectly flag these as financial conflicts.
- Seven billed-debit purchase observations across the sample lack references.
  These are observations, not necessarily seven distinct purchases.
- Non-purchase references do have duplicates/changed values. Do not assume a
  purchase identity strategy safely covers payments, fees, or refunds.

| Export | Source rows | Unexpected column counts | Well-shaped billed debit purchases | Missing purchase references |
| --- | ---: | ---: | ---: | ---: |
| 2026-10-17 | 711 | 5 | 673 | 1 |
| 2026-10-31 | 642 | 5 | 607 | 1 |

The October exports share 563 nonempty purchase references. October 31 contains
43 nonempty purchase references not in October 17. These are full rolling
exports, not counts of outstanding payable purchases; the earlier spreadsheet's
83 reviewed rows are a selected subset.

## Identity policy for Increment 1

1. Configure one stable internal source account (`virgin-primary` initially).
   This is the Virgin account, not the Personal/Joint Monzo allocation or the
   changing last four digits of a card.
2. Identify well-formed billed debit purchases by `(source_account_id, reference)`.
3. Preserve reference text and leading zeros; do not parse it as an integer or
   require exactly 23 digits solely because that format is common in this sample.
4. On overlap, compare canonical date, pennies, currency, direction, and merchant/
   card fields. Any unexplained disagreement is an explicit conflict, not an
   overwrite. Preserve the conflicting source observation.
5. A missing reference or repeated reference within one file is an exception.
   Do not guess identity from amount/date/merchant or collapse identical purchases.
6. Pending, credits, payments, fees, malformed rows, and conflicts remain accounted
   for in source observations and import summaries, outside the purchase-edit queue
   in this increment. Later reconciliation is a separate capability.

This evidence supports using references with guards; it does not establish a
bank-wide uniqueness guarantee. Test missing/colliding/changing references using
synthetic fixtures and retain a manual resolution path in the full Phase 1.
