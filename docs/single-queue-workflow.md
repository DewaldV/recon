# Single-queue workflow

## Import and review

1. Start Recon and open its private startup link. Import the original Virgin CSV.
   Identical files are reused; overlaps preserve decisions and source provenance.
2. Review opens on **Needs review**. The top totals cover the global queue, even
   when merchant/account/import filters or pagination narrow the table.
3. Use **Merchants** to import `Merchant,Category,Description` defaults or approve
   exact aliases. Use **Account rules** for merchant or merchant+card defaults.
   Explicit purchase choices win, then merchant+card, then merchant-wide rules.
4. Open **Review / edit** to complete a purchase. **Save for later** keeps it in
   review; **Save and stage** atomically saves and confirms that it needs funding.
   Bulk actions use page selection and an exact preview. Inline saves clear the
   selection while preserving unrelated drafts and keyboard focus.
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

## Run and resume

**Run recon** previews all outstanding purchases and Personal/Joint totals. It is
available only once every outstanding purchase is staged and all source exceptions
have dispositions. Specify exact source account and pot IDs and a temporary Monzo
token. Confirming starts one automatically numbered run with immutable instructions.
Preflight checks account type, pot ownership/currency, and available funds.

Each purchase creates one pot deposit, using a key persisted before submission.
Results refresh while running. On interruption, reopen **History → Recon #…** and
resume with a fresh token. Confirmed deposits are skipped. Unknown/in-flight
deposits reuse the same key; errors do not prove that no money moved. New imports
remain outside an existing immutable run and await the next run.

Deposit and note outcomes are separate. A confirmed deposit is funded even while
its note is pending. Monzo returns no transaction ID from a pot deposit: find and
verify the exact transaction, enter its ID, confirm the match, and provide a token
to apply the note. The backend checks account membership, amount and currency;
these checks alone do not establish identity. Once annotation is attempted the
mapping is fixed, including after failure. Retry with that mapping and a fresh token.
Never repeat a deposit to repair a note. A run remains active until all deposits
and notes are complete, and blocks starting another run.

Tokens are used in backend memory for that attempt. Forms clear submitted token
fields, responses are not cached, and credentials are not written to SQLite or
logs. Destinations and execution results are persisted. See [backups and recovery](development.md#backups)
before restoring execution data.

## One-time historical CSV

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
request encoding/scoping, preflight failures, pagination and response classification.
`recon-browser-check` uses synthetic data in Firefox for imports, merchant/rule
approval, draft/focus preservation, bulk/individual staging, source dispositions,
run preview and restart. It never submits a Monzo credential.

Still outstanding: real-data acceptance and live Monzo verification, automatic
deposit-to-transaction correlation, missing-reference/malformed-source repair,
pending-to-billed linking, refund/fee execution, exports, and automated stale-ledger
reconstruction. Source dispositions only resolve evidence already understood; they
are not a substitute for repairing a genuinely payable missing purchase.
