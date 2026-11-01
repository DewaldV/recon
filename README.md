# Recon

A local-first Virgin Money credit-card reconciliation app, built in Rust with
SQLite. Import transactions, review exceptions, approve a deposit plan, and
execute resumable transfers into Monzo pots.

## Status

The current flow is **Import → Review → Stage → Run**:

- One global outstanding queue, with Personal/Joint totals and approved merchant/account defaults.
- Searchable merchant/card rule selection, prefilled rule editing, and account-change/staging impact previews.
- Individual Save and stage, bulk review, version-checked staging, and explicit external-funding/exclusion decisions.
- Reference-matched historical CSV migration with provenance and preservation of existing decisions.
- Optional permanent posting-date cutover for keeping earlier reconciliation in historical spreadsheets.
- Audited monzo-cli-compatible extra-column CSV repair, including recovery of previously quarantined rows on reupload.
- One immutable run at a time, temporary Monzo tokens, persisted deposit results and deduplication keys, and restart/resume.
- Persistent Personal/Joint account and pot IDs on Configuration; enter only a fresh token when running or resuming.
- Automatic notes immediately after each deposit, with persisted transaction mappings and separate deposit/note recovery progress.

See the [current workflow guide](docs/single-queue-workflow.md). Old named
workspaces are read-only history. Source dispositions and recognised extra-column
repairs are implemented; general source repair, pending-to-billed linking, and
export/handoff are still deferred.
Automated verification uses synthetic data and mocked Monzo HTTP responses.
The user has completed a live reconciliation, including automatic recovery of its
pending notes. The test suite also covers immediate deposit-and-note execution.

## Development

Tools are provided by devenv and direnv; no Node-based tooling is required.

```sh
direnv allow
recon-check
recon-dev
```

Alternatively, use `devenv shell -- recon-dev` without enabling direnv.

`recon-dev` uses the separate `/tmp/opencode/recon-development` database. Open the
private startup URL printed in the terminal; restarting the app needs a new URL,
but saved decisions persist. Stop with Ctrl+C. Do not share the startup URL.

For regular use, run `cargo run` in the devenv shell. Data defaults to
`$XDG_DATA_HOME/recon` or `~/.local/share/recon`, separate from development data.
Use `cargo run -- --data-dir /absolute/private/local/path` to override it. Do not
put a live database in Proton Drive or another sync folder. Set `RECON_SYNC_ROOTS`
to colon-separated additional sync roots to reject them explicitly.

Upload an original Virgin CSV to open Review, with a link to source exceptions.
`/review` combines page-wide account editing, bulk actions, and staging. Merchant
defaults and account rules have separate screens. Run recon includes every
outstanding purchase, regardless of filters, after all blockers are resolved.
Choose accounts across the page and click **Save page accounts** once. Only changed
choices are saved, atomically. Old `/purchases` bookmarks redirect to Review. Saves refresh the filtered
workspace and clear its page selection so counts and bulk decision versions stay current.
Only one underlying Virgin account (`virgin-primary`) is supported in this first
interface; Personal/Joint are review allocations, not separate source accounts.
Full rolling exports include old history. They are not a list of unpaid purchases.

## Checks and backups

- `recon-check`: formatting, Clippy, and Rust tests, using synthetic inputs only.
- `recon-browser-check`: headless Firefox smoke test with temporary synthetic data.
  devenv supplies Python and geckodriver; install Firefox or set `RECON_FIREFOX`.
  Ports 7310 and 4444 must be free. No Node tooling is needed.
- `cargo run -- --backup`: with the app stopped, create a verified private snapshot
  under the selected data directory's `backups/`. Use the same `--data-dir` if
  backing up a non-default database. Preserve the latest `execution.guard` separately
  alongside snapshots. See [restore instructions](docs/development.md#backups).

## Plans

- [Scope and architecture](docs/README.md)
- [Decision log](docs/decisions.md)
- [Development and safety defaults](docs/development.md)
- [First increment](docs/phase-1/increment-1.md)
- [Increment 2 scope](docs/phase-1/increment-2.md)
- [Increment 3 plan](docs/phase-1/increment-3.md)
- [Increment 3 / Slice 1 usage and verification](docs/phase-1/increment-3-slice-1.md)
- [Increment 1 verification](docs/phase-1/increment-1-verification.md)
- [Frontend choice and alternatives](docs/frontend-options.md)
- [Phase 1: Offline import and review](docs/phase-1-import-and-review.md)
- [Phase 2: Safe Monzo execution](docs/phase-2-monzo-execution.md)
- [Phase 3: Workflow improvements](docs/phase-3-workflow-improvements.md)

Existing recon files and `monzo-cli` remain unchanged. Real financial data,
SQLite databases, and credentials must stay outside this source repository.
