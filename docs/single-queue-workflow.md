# Single-queue workflow

## Import and review

1. Start Recon and open its private startup link. Import the original Virgin CSV.
   Identical files are reused; unresolved malformed rows are retried with the
   current automatic repair policy. Overlaps preserve decisions and source provenance.
2. Review opens on **Needs review**. The top totals cover the global queue, even
   when merchant/account/import filters or pagination narrow the table.
3. Use **Merchants** to import `Merchant,Category,Description` defaults or approve
   exact aliases. Use **Account rules** for merchant or merchant+card defaults.
   Explicit purchase choices win, then merchant+card, then merchant-wide rules.
   Account rules now provide a searchable merchant list (including catalogue-only
   merchants and aliases) and a card dropdown. Choose a merchant and **Any card**
   or a specific card, then **Load selection**. Cards seen for that merchant are
   labelled. Existing rules load their current account, including disabled rules.
   **Edit rule** in the rule list opens the same editor directly.
4. Open **Review / edit** to complete a purchase. **Save for later** keeps it in
   review; **Save and stage** atomically saves and confirms that it needs funding.
   Set account dropdowns across the visible page, then click **Save page accounts**
   once to save all changed choices atomically. Unchanged rows retain their rule
   provenance and staging. The unsaved count tracks pending choices; save before
   staging, filtering or changing pages. A stale changed row rejects the entire
   save. Page saves refresh totals/filter membership and clear bulk selection
   while preserving unrelated drafts. Bulk actions use page selection and an exact preview.
5. Mark already-funded purchases **Handled externally** with evidence. **Excluded**
   is a separate disposition. Reopening either requires explicit confirmation and
   evidence. Merely setting an account or importing Paid No proves no funding status.
6. Inspect source exceptions through the import result link. For a conflict you
   can explicitly retain the canonical purchase and reject the conflicting
   observation. Other unresolved observations need a supported non-funding
   disposition. Original observations remain in the audit trail.

Staging records an exact effective-decision signature. Relevant changes return a
purchase to review; unrelated merchant changes and identical-file reimports do
not. **Return to review** removes approval without marking it handled. Old named
workspace membership does not stage anything.

**Remember account choice** on a review row opens the rule editor with that saved
merchant, card and account. The purchase editor also links to its saved choice;
save purchase edits before following the link. An existing rule's current setting
takes precedence over the suggested account. These links only open the editor.

Rule approval previews show current and proposed accounts, preserved explicit
choices and recorded runs, and how many staged purchases need staging again.
Card-specific previews include only matching cards; merchant-wide rules respect
more-specific rules. Disabling a card rule falls back to the merchant-wide rule.
Confirm and Cancel return to the selected rule. Merchant search uses local
JavaScript; selection, loading and approval also work with ordinary HTML forms.

## Run and resume

**Run recon** previews all outstanding purchases and Personal/Joint totals. It is
available only once every outstanding purchase is staged and all source exceptions
have dispositions. In **Configuration**, save the Personal and Joint source
account IDs (`acc_…`) and credit-card pot IDs (`pot_…`) once. They persist locally
across restarts and are included in database backups. Leave both fields empty for
an allocation you do not use; every allocation in the next run needs a complete pair.

The run preview displays the saved destinations and asks only for a temporary Monzo
token. Confirming starts one automatically numbered run with immutable instructions.
Preflight checks account type, pot ownership/currency, and available funds.
Failures identify the Personal/Joint allocation and the failed lookup, status,
currency or balance check. API failures include the endpoint and HTTP status;
the same locally composed diagnostic is written to the process log. Tokens,
account/pot IDs and raw remote response bodies are not included. Available funds
means the source account balance, excluding money already held in pots.

Configuration changes apply to future runs and invalidate open run previews;
reload the preview before submitting a token. Purchase staging is preserved.
An active or resumed run always uses its recorded destinations, even if settings
subsequently change. Configuration never accepts or stores an access token.

Each purchase creates one pot deposit, using a key persisted before submission,
then automatically applies its note before continuing to the next purchase.
Results refresh while running. On interruption, reopen **History → Recon #…** and
resume with a fresh token. Confirmed deposits are skipped. Unknown/in-flight
deposits reuse the same key; errors do not prove that no money moved. New imports
remain outside an existing immutable run and await the next run.

Deposit and note outcomes are recorded separately. A confirmed deposit is funded
even while its note is pending. Transaction IDs are used directly when returned;
otherwise Recon automatically queries the source account's transactions, matching
the pot, exact GBP debit and recorded submission window (with five seconds of
clock tolerance). Lookup is paginated and retried up to five times for visibility.
Already assigned transactions and unrelated existing notes are excluded. For older
deposits-only runs, complete equal-amount groups are assigned one-to-one in creation
order; ambiguous or incomplete matches remain pending for a later resume.

Mappings are persisted before annotation and reused after failure or restart.
**Resume recon** applies pending notes first, including notes from older runs.
It needs only a fresh token, with no transaction-ID entry or per-note confirmation.
A notes-only resume does not require funds checks or submit deposits. A run remains
active until all deposits and notes are complete, and blocks starting another run.

Tokens are used in backend memory for that attempt. Forms clear submitted token
fields, responses are not cached, and credentials are not written to SQLite or
logs. Destinations and execution results are persisted. See [backups and recovery](development.md#backups)
before restoring execution data.

## Automatic Virgin CSV repair

Standard-layout exports with extra unquoted commas use the same naming convention
as `monzo batch vcc`: surplus middle columns are joined with spaces into the
merchant name, retaining the last ten fields. This can include city fragments;
it deliberately matches existing merchant catalogue names from the CLI workflow.
Dates, amounts, reference, country, card and trailing classification fields must
validate. Already correctly quoted CSV is parsed normally. Nonstandard column
layouts, missing fields, invalid anchors and duplicate references remain for review.
Credits, fees and pending records keep their classifications rather than becoming
fundable purchases. The permanent posting-date boundary also applies to repairs.

Import results show **automatic CSV repairs**, including the original text and
parsed fields. Original archives and observations remain immutable. Repaired
purchases use existing merchant/account defaults but are never automatically staged.

Re-upload an earlier file to recover its repairable, unresolved malformed rows.
Recovery creates a linked corrected import, then dispositions the original rows
in the same transaction. Existing manual dispositions are preserved. Repeated
reuploads do not repeat repairs or duplicate purchases. With the app stopped,
the equivalent maintenance command is:

```sh
cargo run -- --repair-import 1
```

Use the running app's `--data-dir` if configured. Recovery takes a verified backup
and rejects an active run; failures roll back the corrected import, purchases,
repair provenance and original dispositions together.

## One-time historical CSV

### Starting from a fixed cutover instead

If the spreadsheets remain authoritative for earlier, completed reconciliation,
configure a permanent inclusive **posting-date** boundary with the app stopped:

```sh
cargo run -- --start-date YYYY-MM-DD --cutover-reason "Earlier purchases were reconciled in the historical spreadsheets"
```

Use the same `--data-dir` as the running app if you use a custom data directory.
The command creates a verified backup and atomically excludes purchases with
posting dates **strictly before** the boundary. Purchases on the boundary stay in
scope, matching `monzo batch vcc`'s `posting_date >= filter_date` rule. Account
choices, details and staging for in-scope purchases are preserved. Existing source
records remain available under **Handled / excluded** and in purchase history.

The boundary persists in SQLite and applies to future full/overlapping imports.
It is not a rolling last-run date. The same date can be applied again without
duplicating decisions; changing an established boundary is rejected. Pre-cutover
purchases cannot be reopened into the outstanding queue. Active runs and recorded
run instructions crossing the boundary block configuration.

Source exceptions with a valid pre-cutover posting date are given a historical
disposition. Missing/unreadable dates and conflicts against in-scope purchases
remain for review. Import displays the configured start date. Keep the resulting
database backup and the latest `execution.guard` using the normal backup process.

### Importing historical decisions

On Import, expand **One-time historical decisions**. Prepare a UTF-8 CSV with
exact columns (order may vary):

```csv
Reference,Account,Category,Description,Paid,Reason
00000000000000000000001,Personal,Shopping,Synthetic groceries,Yes,Verified source reference and external funding
```

Keep references as text with leading zeros, and verify matches against the original
source. Account accepts Personal/Joint or blank; Paid accepts Yes/No or blank.
Reason is required (up to 3,500 bytes); at most 100 distinct purchases per file.
Unknown/duplicate references and conflicted purchases are rejected. Review the
matched purchases and proposals before confirmation; approve new categories explicitly.

Existing explicit account decisions, including cleared choices, purchase-detail
versions, handling decisions and run instructions are preserved. Paid Yes records
external handling only without an existing handling decision. Paid No or blank
does not establish outstanding status. Imports never stage. Raw bytes, proposals
and previous snapshots are retained; identical historical files are no-ops.

## Verification and remaining work

`recon-check` runs formatting, Clippy and the Rust suite. Tests cover source
identity, migration, rules/staging, atomic edits, historical import and rollback,
immutable runs, partial failures, restart, uncertain retries, note mapping reuse,
stale restore rejection, and HTTP protections. Local mocked Monzo HTTP tests check
request encoding/scoping, preflight failures, pagination, delayed visibility,
repeated-amount matching and response classification.
`recon-browser-check` uses synthetic data in Firefox for imports, merchant/rule
approval, draft/focus preservation, bulk/individual staging, source dispositions,
run preview, restart and token-only note recovery. It never submits a Monzo credential.

The user has completed a live reconciliation, including automatic recovery of its
pending notes. Immediate deposit-and-note sequencing is covered by the mocked suite.

Still outstanding: missing-reference/general malformed-source repair,
pending-to-billed linking, refund/fee execution, exports, and automated stale-ledger
reconstruction. Source dispositions only resolve evidence already understood; they
are not a substitute for repairing a genuinely payable missing purchase.
