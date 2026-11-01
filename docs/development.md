# Development and local safety

## Commands

```sh
direnv allow
recon-check
recon-dev
recon-browser-check
```

Without direnv:

```sh
devenv shell -- recon-check
devenv shell -- recon-dev
```

`recon-check` runs formatting, Clippy with warnings denied, and all Rust tests.
`recon-dev` watches/runs the app against `/tmp/opencode/recon-development`, never
the regular default data directory. `recon-browser-check` builds the app, starts
headless Firefox through geckodriver, and checks synthetic upload/edit/restart,
merchant defaults, account rules, staging, source dispositions, and run preview.
Python and geckodriver come from devenv; Firefox must be installed or specified
with `RECON_FIREFOX`. The smoke test uses temporary private data and cleans up its
app/browser/driver processes. Ports 7310 and 4444 must be free.

These commands do not import real files automatically, enable direnv, or obtain
credentials. Bundled SQLite is used by the app/tests; `sqlite` is available for
manual inspection.

## Implemented runtime defaults

- Bind `127.0.0.1:7310`, never all interfaces. Fail clearly on a port collision.
- Generate a fresh cryptographically random access token per start. Open/print
  the loopback bootstrap URL with the token in its fragment, not query/path.
  A minimal external same-origin bootstrap script exchanges it via POST for an in-memory
  session, then removes the fragment using `history.replaceState`. Never log it.
- Require an HttpOnly, SameSite=Strict, host-only cookie for all data routes. No
  financial data in the unauthenticated bootstrap page or health response.
  HTTP on loopback cannot use a Secure cookie; do not pretend it provides TLS.
- Validate the exact Host and mutation Origin against the bound address/port;
  reject missing/unexpected mutation origins (except a deliberately tested local
  client path if one is added). No permissive CORS. Verify CSRF tokens on every
  state mutation, including HTMX and multipart upload. Session bootstrap validates
  Origin/Host and the access token; rotate sessions on restart.
- Askama escapes untrusted text. Do not mark merchant/import/error text as safe
  HTML. Return generic internal errors; do not expose SQL or raw financial rows.
- Pin and vendor HTMX and its license. No remote scripts, fonts, or analytics.
  Disable HTMX script execution/eval-dependent features and user-supplied HTMX
  processing; imported text remains text. The CSP permits same-origin external
  scripts/styles only, with no inline scripts or event handlers and no eval.
- Add `Cache-Control: no-store` to sensitive responses, `Referrer-Policy: same-origin`,
  MIME sniffing protection, and CSP framing restrictions.
  `same-origin` leaks no referrer cross-origin while preserving Firefox's Origin
  header on ordinary form POSTs; `no-referrer` made those Origins null and broke
  strict Origin validation. See D021.
- Default data directory: `$XDG_DATA_HOME/recon`, else `$HOME/.local/share/recon`.
  The `--data-dir` override must be absolute and outside source/legacy/cloud
  folders; canonicalize existing ancestors to prevent symlink bypass. Cloud-sync
  detection is not reliably automatic: document and reject known configured sync
  roots, and require the user not to use other sync locations.
- Distinct temporary directories for tests; a distinct explicit development data
  directory for browser experiments. Never default dev/test imports to legacy data.
- Establish a 10 MiB request/upload limit, bounded parsing work, and parameterized
  SQL. Original upload filenames are display metadata, never filesystem paths.
- No financial row contents, card/reference numbers, token URLs, or upload bodies
  in logs. Log event IDs and aggregate counts only.

## Merchant review

Use Merchants for catalogue CSV preview/import, grouped missing merchants,
merchant defaults and aliases; use Account rules for allocation defaults and
Review for purchase overrides, bulk edits and staging.
See [current usage](single-queue-workflow.md). Bulk review mutations
use ordinary authenticated forms and immutable server-side previews. Every commit
checks the global review revision; any intervening source/review change requires
a fresh preview. This intentionally rejects even unrelated concurrent changes.

## Backups

The app takes a consistent SQLite snapshot before every new import, before review commits, and before
migrating an existing versioned database. Identical-file reimports do not create
extra backups. Original CSV bytes are inside SQLite, so snapshots include source
provenance. Snapshots are private and verified with integrity/foreign-key checks.
Only files named `recon-*.sqlite3` in `backups/` are verified; a failed snapshot may
leave a `pending-*.sqlite3` file, which must not be used for restore. No automatic
retention policy deletes old backups in this increment.

With the app stopped:

```sh
cargo run -- --backup
```

### Execution recovery record

`execution.guard` is a private, fsynced watermark of the latest execution event
and operation identity. It is written before sending a deposit or note mutation
and after recording its result. It is outside SQLite so restoring an older
database cannot silently erase knowledge of submitted operations. The CLI backup
creates a SQLite snapshot only: preserve the **latest** recovery record separately,
and retain the original data directory. Do not roll the recovery record back to
match an old snapshot.

With the app stopped, create a new private local directory (0700), copy a verified
snapshot as `recon.sqlite3`, and copy the latest `execution.guard` from the original
data directory. Restrict both files to 0600. Start with `--data-dir` pointing there.
Startup verifies SQLite integrity, foreign keys, and supported schema. Confirm
imports, run instructions and results before switching regular use to the restore.

A missing guard on an execution-capable database, a guard newer than the database,
or a mismatched operation identity blocks all execution. Review/history remains
available. Prefer a newer complete database containing those operations. If none
exists, bank outcomes and original operation/deduplication identities must be
reconciled before recovery; there is currently no supported automated ledger-repair
or guard-reset command. Do not delete/edit the guard to bypass the block.

The guard cannot detect simultaneous loss/rollback of both files, nor knowledge
of later payments when an old pre-execution schema is migrated as a new directory.
Such restores need external bank reconciliation; never treat an old backup as a
new payable queue. Missing financial evidence cannot be reconstructed from totals.
Tests cover readable database-only restores, rejection of stale snapshots, and
matching current database/guard restores.
