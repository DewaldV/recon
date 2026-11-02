# Recon

A local-first Virgin Money credit-card reconciliation app, built in Rust with
SQLite. **Import → Review → Stage → Run**: reuse merchant and account defaults,
review outstanding purchases, then fund Monzo pots with resumable deposits and notes.

## Start

Development tools are provided by devenv and direnv.

```sh
direnv allow
cargo run
```

Without direnv, use `devenv shell -- cargo run`.

Open the private startup URL printed in the terminal. Each restart needs a new
URL; saved decisions persist. Stop with Ctrl+C.

Data defaults to `$XDG_DATA_HOME/recon` or `~/.local/share/recon`. Override it with
`cargo run -- --data-dir /absolute/private/local/path`. Keep live data outside the
repository and cloud-sync folders.

Follow the [workflow guide](docs/single-queue-workflow.md) to import, review,
configure destinations, and run or resume a reconciliation.

## Development

```sh
recon-check
recon-dev
```

`recon-check` runs formatting, Clippy, and Rust tests. `recon-dev` watches the app
using a separate database at `/tmp/opencode/recon-development`.

See [development](docs/development.md) for browser checks, runtime protections,
and backup/restore instructions.

## Documentation

- [Architecture and documentation index](docs/README.md)
- [Decisions](docs/decisions.md)
- [Remaining work](docs/single-queue-workflow.md#remaining-work)
- [Workflow improvements](docs/phase-3-workflow-improvements.md)
