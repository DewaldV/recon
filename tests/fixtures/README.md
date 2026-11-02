# Synthetic CSV fixtures

All names, references, cards, and amounts are invented. Keep fixtures synthetic.

- `virgin-first.csv`: two genuine distinct references with otherwise identical
  purchase details, plus one missing-reference purchase to quarantine.
- `virgin-overlap.csv`: one reference from the first file and one new purchase
  with a quoted comma in its merchant name.

Expected after importing first, repeating first, then overlap under one account:
two distinct imports, three canonical purchases, five source observations, and
one missing-reference exception. Any account decision on the overlapping purchase
must survive. These expectations are exercised in `tests/import_review.rs`.

`merchants.csv` is a synthetic UTF-8 catalogue with whitespace/case duplicates,
quoted commas, and emoji.
