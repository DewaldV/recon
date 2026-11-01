# Recon

A local-first Virgin Money credit-card reconciliation app, built in Rust with
SQLite. Import transactions, review exceptions, approve a deposit plan, and
execute resumable transfers into Monzo pots.

## Status

Increments 1 and 2 are implemented: persistent CSV import, guarded overlap handling,
merchant catalogue preview/import, versioned category/description defaults, exact
aliases, purchase overrides, filtered review, and atomic bulk Personal/Joint edits.
Private local sessions, version-checked edits, and verified backups remain in place.

Increment 3's first slice adds explicit handling states, named recon selections,
decision history, and outstanding-ready Personal/Joint totals. See the
[Slice 1 guide](docs/phase-1/increment-3-slice-1.md) for usage and limitations.
Unknown history, external funding (legacy Paid = Yes), exclusions, and blocked
rows do not enter those totals. Setting an account or merchant details alone
does not make a purchase outstanding-ready.

Account rules, historical spreadsheet migration, source-exception repair, exports,
approval, and Monzo access are not implemented. Review-complete still means
details complete, not evidence of funding or permission to execute.

See [Increment 2 usage and verification](docs/phase-1/increment-2-verification.md).
Rust checks pass. The extended Firefox smoke test remains pending: the latest
attempt was blocked by an existing app on port 7310, which was left running.
Stop the app before running `recon-browser-check`.

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

Upload an original Virgin CSV, inspect its exception summary, and open Review.
`/review` combines individual account edits, bulk actions, and merchant enrichment.
Old `/purchases` bookmarks redirect to Review. Inline saves refresh the filtered
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
  backing up a non-default database. See [restore instructions](docs/development.md).

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
