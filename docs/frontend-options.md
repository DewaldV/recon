# Frontend options — HTMX selected

Decision: Axum + Askama + HTMX, confirmed by the user on 2026-11-01.
See [the decision log](decisions.md). The alternatives below are retained as
context, not open choices.

All options use a Rust backend, local SQLite storage, ordinary CSS, locally
served assets, and no Node/npm build tooling. A browser interface is not a
requirement to introduce a separate frontend service.

## 1. Server-rendered HTML + HTMX (recommended)

Selected stack: Axum + Askama templates + a pinned, locally vendored HTMX script.
Dependencies and the asset will be installed during implementation.

- Rust renders complete pages and small HTML fragments.
- HTMX submits review actions and replaces relevant rows, counters, or panels.
- Standard HTML forms can support basic workflows without JavaScript.
- A little plain JavaScript can handle keyboard shortcuts and selection state.
- No JavaScript bundler, package manager, or asset CDN is needed at runtime.

Best fit for a filterable review table, inline merchant editing, bulk decisions,
and execution progress. Main costs: learning HTMX conventions, careful fragment
boundaries, and preserving selection/focus across updates. Use a simple polling
endpoint for progress first; SSE can wait.

## 2. Server-rendered HTML + ordinary forms

Candidate stack: Axum + Askama, with no frontend JavaScript initially.

- Forms submit decisions and redirect to updated pages.
- Fewest moving parts and easiest request/response debugging.
- Tables, filters, and bulk actions work without a frontend framework.
- Full-page refreshes are less fluid for frequent inline edits and progress.

Good if absolute simplicity matters more than interaction polish. Can evolve
into option 1 without replacing the backend or template model.

## 3. Server-rendered HTML + small vanilla JavaScript

Candidate stack: Axum + Askama plus hand-written browser JavaScript modules.

- No library dependency or build step.
- Fine-grained control of table interactions, keyboard input, and progress.
- We own fetching, error handling, DOM updates, and focus/state management.
- More bespoke browser code than HTMX as interactions grow.

Good if avoiding a frontend library is more important than minimizing custom
JavaScript. Keep financial logic in Rust, not duplicated in the browser.

## 4. Rust-native browser UI with WASM

Candidates include Leptos or Dioxus, using Rust/WASM tooling rather than Node.

- Can keep UI implementation in Rust and support richer reactive interactions.
- Introduces a WASM compilation/asset pipeline and framework-specific tooling.
- More complexity than this mostly form-and-table app currently warrants.

Possible without Node, but not recommended for the first version.

## Recommendation

Option 1 gives the best balance of simple deployment and efficient review and is
now selected. Ordinary forms remain a useful fallback, not a separate frontend.
