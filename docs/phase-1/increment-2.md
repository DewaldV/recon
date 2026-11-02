# Increment 2: Merchant enrichment and exception review

Historical scope, implemented with Rust and Firefox checks passing. Separate
merchant-sheet real-data acceptance was not recorded. See
[verification](increment-2-verification.md) and the
[current workflow](../single-queue-workflow.md).

## Delivered

- UTF-8 `Merchant,Category,Description` catalogue preview/import, with conflict
  selection, category approval, and source provenance.
- Versioned merchant defaults, exact aliases, and per-purchase field overrides.
- Missing-detail groups, search/filters, and distinct-purchase counts.
- Page-selected bulk account/detail changes with atomic preview/confirmation.
- Migration 0003 for catalogue, override, alias, and preview records.

This increment addressed review completeness. Handling, staging, account rules,
and execution followed later.

## Review contract

Matching uses Unicode lowercase and collapsed whitespace. Punctuation, processor
prefixes, and digits remain significant. Approved aliases are exact; fuzzy matches
and historical rule suggestions were outside scope.

Each detail resolves as purchase override → merchant default → missing. Clearing
an override restores the default. Merchant changes preview affected purchases and
preserve overrides. Account allocation remains independent of merchant details.

Catalogue imports coalesce identical definitions and require selection among
conflicts. Identical-file reimports preserve later edits. New categories require
approval; values are not seeded from private files at startup.

Bulk operations use explicit IDs, cap selections at 100 purchases, and reject stale
or blocked rows atomically. Previews capture the global review revision; any
intervening source/review change requires another preview.

## Acceptance coverage

- Parsing, normalization collisions, conflicts, category approval, and idempotence.
- Override/default precedence, clearing, aliases, overlap, and restart.
- Migration from schema 2, backup/restore, and append-only provenance.
- Overlapping missing-field counts, source conflicts, pagination, and selection bounds.
- Stale/replayed previews, injected database failures, escaping, and HTTP protections.
- Firefox catalogue/default/override workflows, bulk edits, filters, and restart.

The original real-data check called for a disposable database, a selected merchant
export, aggregate match/conflict counts, and local sample comparison.
