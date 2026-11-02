# Single-queue workflow

## Import and review

1. Start Recon and open its private startup link. Upload the original Virgin CSV.
   Overlaps preserve decisions and provenance; identical files are reused, with
   unresolved malformed rows retried under the current repair policy.
2. **Review** opens on **Needs review**. Top totals cover the whole queue,
   regardless of filters or pagination. Rolling exports include old history;
   importing a purchase does not establish that it still needs funding.
3. Complete category, description, and Personal/Joint allocation through
   **Review / edit**. **Save for later** keeps the purchase in review;
   **Save and stage** also confirms it needs funding.
4. To change accounts across a page, choose values and click **Save page accounts**.
   Only changed rows are saved, atomically; a stale row rejects the whole save.
   Save before staging, filtering, or changing pages. Saving clears page selection.
5. For bulk edits or **Stage selected**, select purchases on the current page,
   preview the exact changes, and confirm. Blank category/description overrides
   fall back to merchant defaults.
6. Mark already-funded purchases **Handled externally** with evidence. Use
   **Excluded** for other intentional omissions. Reopening either requires evidence
   and the reopening checkbox.
7. Follow **View import results and exceptions** to resolve source blockers.
   A conflict can retain the canonical purchase and reject the conflicting
   observation; other exceptions need a supported non-funding disposition.
   Original evidence remains in history.

Staging approves the effective account, category, and note. Relevant changes
return a purchase to review; unrelated edits and unchanged reimports preserve
staging. **Return to review** removes approval without marking a purchase handled.

The interface supports one Virgin source account (`virgin-primary`). Personal/Joint
are funding allocations. Old named workspaces are read-only history.

## Merchant defaults and account rules

Use **Merchants** to set category/description defaults or approve exact aliases.
Matching ignores case and repeated whitespace; punctuation and digits remain
significant. Purchase overrides take precedence. Alias chains and alias
correction/removal are not supported.

To migrate a merchant catalogue:

1. Export the merchant sheet as UTF-8 CSV with headers
   `Merchant,Category,Description`. Export formulas as displayed values and quote
   commas, quotes, and newlines. Preserve the original workbook.
2. Under **Merchants → Import merchant catalogue**, upload the CSV and inspect
   normalized-name conflicts. Select one definition per merchant; unchecked
   definitions are skipped. Existing different defaults are not selected automatically.
3. Approve new categories and confirm the preview. Identical-file reimports
   preserve later edits and do not reconsider skipped definitions; edit the default
   or upload a revised CSV to change them.

Use **Account rules** for merchant-wide or merchant+card allocation defaults:

1. Search/select a merchant, choose **Any card** or a specific card, and click
   **Load selection**. **Edit rule** opens an existing rule directly.
2. Choose Personal, Joint, or **Disable this rule**.
3. Click **Preview rule**, inspect affected purchases and staging changes, and confirm.

Account precedence is purchase choice → merchant+card rule → merchant rule.
Clearing a purchase choice restores rule fallback; disabling a card rule restores
the merchant-wide fallback. Rules preserve explicit choices and recorded runs.

**Remember saved account choice** opens the rule editor from a purchase. Save
purchase edits first; an existing rule's setting takes precedence over the suggestion.

## Run and resume

1. In **Configuration**, save the source account ID (`acc_…`) and credit-card pot
   ID (`pot_…`) for Personal and Joint. Leave both fields empty for an unused
   allocation. Settings persist across restarts and are included in backups.
2. Open **Run recon**. Every outstanding purchase must be staged and all source
   blockers resolved. The preview includes the whole outstanding queue.
3. Check totals, destinations, and notes. Enter a temporary Monzo token and click
   **Confirm and run recon**. Preflight checks account type, pot ownership/currency,
   and source-account funds, excluding money held in pots.

Each purchase creates one deposit followed by its note. Instructions and
deduplication keys are persisted before submission. Configuration changes affect
future runs and invalidate open previews; active runs retain their destinations.
New imports wait for the next run.

After interruption, open **History → Recon #…** and **Resume recon** with a fresh
token. Confirmed deposits are skipped; uncertain deposits reuse their original
keys. An error does not prove that no money moved.

Deposit and note progress are separate. A confirmed deposit is funded even while
its note is pending. Recon finds and saves transaction mappings automatically;
ambiguous matches remain pending. Resume processes pending notes first. A
notes-only resume skips deposits and funds checks. Another run can start only
after all deposits and notes finish.

Tokens remain in backend memory for the attempt. Read the
[backup and recovery instructions](development.md#backups) before restoring execution data.

## Automatic Virgin CSV repair

For standard-layout exports with extra unquoted commas, Recon joins surplus
merchant columns with spaces, matching `monzo batch vcc` catalogue names. Remaining
fields must validate. Quoted CSV is parsed normally; unsupported layouts, missing
fields, invalid anchors, and duplicate references remain for review.

Import results show **automatic CSV repairs** with original text and parsed fields.
Repaired rows retain their purchase/credit/fee/pending classification and the
posting-date boundary. Purchases receive defaults but still need staging.

Re-upload an earlier file to recover repairable, unresolved malformed rows. This
creates a linked corrected import and resolves the original observations atomically.
Manual dispositions are preserved; repeated uploads do not duplicate repairs.
Recovery takes a verified backup and requires no active run.

Alternatively, stop the app and run this for the import ID shown in its results:

```sh
cargo run -- --repair-import 1
```

Include the app's `--data-dir` when using a custom database.

## Historical migration

### Fixed posting-date cutover

If spreadsheets remain authoritative for completed earlier reconciliation, stop
the app and set a permanent inclusive **posting-date** boundary:

```sh
cargo run -- --start-date YYYY-MM-DD --cutover-reason "Earlier purchases were reconciled in the historical spreadsheets"
```

Include the app's `--data-dir` when using a custom database. The command takes a
verified backup and excludes unresolved/outstanding purchases strictly before the
boundary. Purchases on the date remain in scope; their decisions and staging are
preserved. Earlier records remain visible under **Handled / excluded**.

The boundary applies to future imports. It cannot be changed, and pre-cutover
purchases cannot be reopened. Active runs or recorded instructions before the
boundary block configuration. Reapplying the same date does not duplicate decisions.

Source exceptions with valid earlier posting dates receive historical dispositions.
Unreadable dates and conflicts against in-scope purchases still need review.
Preserve the resulting backup and latest `execution.guard`.

### Historical decisions CSV

On **Import**, expand **One-time historical decisions**. Prepare a UTF-8 CSV with
these exact columns (order may vary):

```csv
Reference,Account,Category,Description,Paid,Reason
00000000000000000000001,Personal,Shopping,Synthetic groceries,Yes,Verified source reference and external funding
```

- Preserve references as text, including leading zeros, and verify source matches.
- Account accepts Personal/Joint or blank; Paid accepts Yes/No or blank.
- Reason is required (up to 3,500 bytes); files allow at most 100 distinct purchases.
- Unknown/duplicate references and conflicted purchases are rejected.

Review the matched purchases and proposals, approve new categories, then confirm.
Existing explicit account, detail, handling, and run records take precedence.
`Paid = Yes` records external funding only when no handling decision exists;
`Paid = No` or blank proves no funding status. Historical imports never stage
purchases. Original bytes and proposals are retained; identical files are no-ops.

## Remaining work

- General malformed-source and missing-reference repair.
- Pending-to-billed linking and refund/fee execution.
- Export/handoff and audit reports.
- Automated stale-ledger reconstruction.

Source dispositions resolve understood evidence; repair is still needed for a
genuinely payable purchase missing from the queue. See
[workflow improvements](phase-3-workflow-improvements.md) for other candidates.
