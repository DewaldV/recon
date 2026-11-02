# Frontend selection

Axum + Askama + HTMX was selected on 2026-11-01; see D003–D004 in the
[decision log](decisions.md).

Rust renders pages and fragments. HTMX updates the review workspace; vanilla
JavaScript handles session bootstrap, selection/focus, merchant search, and
progress refresh. Assets are served locally.

The main tradeoff is maintaining fragment boundaries and preserving drafts/focus
across updates. Ordinary forms support review actions as a fallback.

## Alternatives considered

| Option | Benefit | Cost |
| --- | --- | --- |
| Askama + ordinary forms | Simple request/response debugging. | Full-page refreshes for frequent edits and progress. |
| Askama + vanilla JavaScript | Direct control of interactions. | Custom fetching, error handling, and DOM/state management. |
| Rust/WASM UI (Leptos or Dioxus) | Rich reactive UI in Rust. | Additional compilation pipeline and framework complexity. |
