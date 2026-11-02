# Development

## Commands

Enter the devenv environment with `direnv allow`, or prefix a command with
`devenv shell --`.

| Command | Purpose |
| --- | --- |
| `cargo run` | Run with the regular data directory. |
| `recon-dev` | Watch/run against `/tmp/opencode/recon-development`. |
| `recon-check` | Check formatting, Clippy with warnings denied, and all Rust tests. |
| `recon-browser-check` | Build and run the headless Firefox smoke test. |

Browser checks require free ports **7310** and **4444**. devenv supplies Python
and geckodriver; install Firefox or set `RECON_FIREFOX` to its executable.
The script uses a temporary database and cleans up its processes.

Tests use synthetic data and mocked Monzo responses. Rust coverage includes
identity, rules/staging, atomic edits, historical imports, immutable runs, retries,
note matching, restore rejection, and HTTP protections. Firefox covers imports,
merchant/rule approval, draft/focus preservation, staging, configuration, source
dispositions, run preview, restart, and the token-only note-recovery form.

## Runtime protections

- Bind `127.0.0.1:7310`. Each start generates a private URL whose fragment is
  exchanged for an in-memory session and removed from browser history.
- Data routes require an HttpOnly, SameSite=Strict, host-only cookie. Requests
  validate the exact Host; mutations also require Origin and CSRF validation.
  Session bootstrap uses the startup token. Sessions rotate on restart.
- Askama escapes imported text. Assets are local; CSP restricts scripts/styles
  to same-origin files. HTMX evaluation, script execution, and history caching
  are disabled. Responses use `no-store`, framing/MIME protections, and
  `Referrer-Policy: same-origin` ([D021](decisions.md#d021--browser-compatible-referrer-policy-implementation-default-2026-11-01)).
- Uploads are limited to 10 MiB, with 64 KiB multipart overhead. Parsing/SQLite
  work is bounded; SQL is parameterized. Upload filenames are display metadata.
- Errors omit SQL, raw financial rows, and remote response bodies. Monzo tokens
  remain in backend memory; the private startup URL is printed only at launch.

Data defaults to `$XDG_DATA_HOME/recon`, falling back to `~/.local/share/recon`.
`--data-dir` must be absolute and outside the repository, `~/Projects`, legacy
recons, and configured sync roots. Existing ancestors are resolved to check symlinks.
Directories use mode 0700; database, lock, and recovery files use 0600.

Keep live SQLite data outside cloud-sync folders. Known home-directory sync roots
are rejected; add others through colon-separated `RECON_SYNC_ROOTS` paths.

Review previews record a global revision. Any intervening source/review change,
including an unrelated edit, requires a fresh preview. Commits are atomic.

## Backups

Recon takes verified SQLite snapshots before new imports, review commits, and
migrations of versioned databases. Unchanged reimports need no snapshot; recovery
of quarantined rows takes one. Snapshots include original CSV bytes and provenance.
There is no automatic retention policy.

With the app stopped, create a snapshot manually:

```sh
cargo run -- --backup
```

For a custom database, include its `--data-dir`. Verified snapshots are named
`backups/recon-*.sqlite3`; `pending-*.sqlite3` files are incomplete and must not
be restored.

### Execution recovery record

`execution.guard` is a private, fsynced watermark of the latest execution event
and operation identity. It is written before deposit/note mutations and after
recording their results. It lives outside SQLite to detect a restored stale ledger.

The backup command copies **SQLite only**. Preserve the **latest** `execution.guard`
separately and retain the original data directory. Do not roll the guard back to
match a snapshot.

### Restore

With the app stopped:

1. Create a private local directory (0700).
2. Copy a verified snapshot into it as `recon.sqlite3`.
3. Copy the latest `execution.guard` from the original data directory. Set both
   files to 0600.
4. Start with `cargo run -- --data-dir /absolute/path/to/restore`.
5. Check imports, run instructions, and results before adopting the restore.

Startup checks integrity, foreign keys, and schema compatibility. A missing guard
on an execution-capable database, a newer guard, or a mismatched operation identity
blocks execution while leaving review/history available. Prefer a newer complete
database. Otherwise, reconcile bank outcomes and original operation/deduplication
identities; there is no automated ledger-repair or guard-reset command. Do not
delete or edit the guard to bypass the block.

The guard cannot detect rollback/loss of both files or later payments absent from
an old pre-execution database. Reconcile such restores against bank records before
treating any purchases as outstanding.
