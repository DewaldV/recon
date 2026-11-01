# Phase 2: Safe Monzo execution

> Current implementation: immutable single-queue runs, temporary token entry,
> preflight, stable deposit keys, persisted recovery, and automatic transaction
> matching/annotation are implemented and covered by synthetic/mock tests. The
> user has completed live deposits and automatic recovery of pending notes. This
> document retains the original work packages; exports and automated stale-ledger
> repair remain outstanding. See the
> [current guide](single-queue-workflow.md) and D026–D029.

## Outcome

Approve and execute deposits for both accounts, with persistent progress and
safe recovery. Keep one deposit and descriptive note per purchase.

## Prerequisites

- Phase 1 identity, review, backup, and migration acceptance criteria pass.
- Legacy/external payment outcomes have been resolved for selected transactions.
- Explicit user approval before any live test or real deposit.

## Work packages

### 1. Client and preflight

- Inspect the underlying Monzo Rust client's deposit implementation; ensure callers
  can provide a persistent `dedupe_id`, or implement that endpoint directly.
- Accept a temporary token for each online attempt; keep it in backend memory only.
- Import existing pot-name mappings, resolve and persist account/pot IDs, and
  validate each pot belongs to the intended source account.
- Validate positive integer-penny amounts, supported currency, row readiness,
  exclusions, prior handling, destination validity, and available funds. Balance
  checks are advisory snapshots, not guarantees against subsequent changes.
- Paginate read APIs and handle expiry, permission errors, rate limits, and timeouts.

### 2. Immutable approval and execution ledger

- Preview counts/totals, exact destinations, amounts, and notes before approval.
- Persist a versioned immutable approved plan. Editing its unsubmitted decisions
  invalidates approval; submitted financial instructions cannot be edited/replayed.
- Persist operation identity and `dedupe_id` before sending any deposit request.
  Reuse that exact key for retries of the same operation, never generate one anew
  on restart or recon-directory change.
- Record deposit lifecycle separately from annotation lifecycle: not submitted,
  in flight, confirmed, failed, or outcome unknown; note pending/succeeded/failed.
- Serialize execution and prevent concurrent processes from submitting the same
  work. Make the database transition to in-flight atomic.

### 3. Recovery and transaction correlation

- Never repeat a confirmed deposit because annotating it failed.
- Do not treat timeouts as proof of failure. Verify retry/deduplication semantics
  and reconcile uncertain outcomes before continuing; avoid claiming exactly-once
  behavior beyond the API's guarantees.
- Establish a reliable way to correlate a deposit with its Monzo transaction for
  notes. The deposit response returns pot state, not a transaction ID. Amount and
  a one-minute time window alone are insufficient for repeated equal deposits.
- Persist transaction IDs before annotation and leave ambiguous matches pending
  for automatic retry/resume. No per-transaction ID entry or confirmation is required.
- Retry safe read/note operations with bounded backoff. Use the persistent key for
  any permitted deposit retry. Present explicit uncertain/blocked states.
- Optionally annotate confirmed transactions with an application operation ID;
  this helps later audits but does not solve initial transaction correlation.

### 4. Operator experience

- One approved plan may include both Personal and Joint transactions.
- Show live/polled execution progress, partial completion, and actionable errors.
- Resume pending operations without reprocessing confirmed deposits.
- Define completion clearly: distinguish money moved from notes completed, and
  retain outstanding note repairs even if all deposits are confirmed.
- Export an audit report. Fail any future automation/CLI invocation appropriately
  on incomplete or uncertain outcomes, rather than returning silent success.
- Transaction identity and ledger state replace manual posting-date checkpoints.

## Acceptance criteria

- Mock tests cover duplicate invocation, concurrent invocation, token expiry,
  partial batches, timeout after remote success, crash before/after submission,
  note failure, and repeated equal-amount deposits.
- Restart/resume reuses persisted operation keys and skips confirmed deposits.
- Destination mismatch, unresolved legacy state, invalid rows, or changed plans
  prevent submission.
- Tests verify no token appears in logs, database records, or browser responses.
- A backup/restore drill documents how restored stale execution state is reconciled
  before any further live execution.
- A separately approved small live test verifies API behavior and correlation
  before the October backlog is submitted.

## API references

- https://docs.monzo.com/#deposit-into-a-pot
- https://docs.monzo.com/#annotate-transaction
- https://docs.monzo.com/#list-transactions

The documented deposit endpoint requires a static `dedupe_id` across retries.
The documented annotation endpoint updates metadata/notes, not categories.
