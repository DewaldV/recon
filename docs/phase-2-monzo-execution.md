# Phase 2: Monzo execution

Delivery summary of the original execution plan. For operator actions, see
[Run and resume](single-queue-workflow.md#run-and-resume). D026–D029 record the
queue, credential, note-recovery, and restore decisions.

## Delivered

- One active run containing immutable Personal/Joint purchase instructions.
- Persistent account/pot configuration and a temporary token per execution attempt.
- Preflight validation of account type, pot ownership/currency, and available funds.
- Stable deposit keys persisted before submission, with separate deposit/note events.
- Sequential deposit-and-note execution, progress display, and restart/resume.
- Persisted transaction mappings and an independent stale-restore guard.

The user reported successful live reconciliation, including automatic recovery of
pending notes. Immediate deposit-and-note sequencing is covered by mocked tests.

## Execution contract

Each purchase produces one deposit. Confirmed deposits are skipped on resume;
uncertain retries keep their original deduplication key. Timeouts do not prove
failure. Funds checks are snapshots, not guarantees against later balance changes.

A confirmed deposit removes its purchase from outstanding work. The run remains
active until its notes finish. Resume processes pending notes first; notes-only
recovery skips deposits and funds checks. Recorded destinations survive later
configuration changes.

## Transaction correlation

Use a returned transaction ID when available. Otherwise query the source account's
transactions, matching pot, exact GBP debit, and the recorded submission window
with five seconds of clock tolerance. Paginate and retry visibility up to five times.

Exclude occupied mappings and unrelated notes. Existing operation markers take
priority; complete equal-amount groups are paired chronologically. Ambiguous or
incomplete matches remain pending. Persist mappings before annotation and reuse
them after restart.

## Verification

Synthetic execution and mocked HTTP tests cover immutable plans, concurrent attempts,
partial failures, uncertain retries, restart, failed notes, repeated amounts,
request encoding/scoping, preflight rejection, pagination, delayed visibility,
response classification, and stale-restore rejection.

The [recovery procedure](development.md#backups) documents database/guard restore
requirements. Automated ledger reconstruction and audit exports remain open.

## API references

- [Deposit into a pot](https://docs.monzo.com/#deposit-into-a-pot): keep `dedupe_id` stable across retries.
- [Annotate transaction](https://docs.monzo.com/#annotate-transaction): notes/metadata, not category updates.
- [List transactions](https://docs.monzo.com/#list-transactions)
