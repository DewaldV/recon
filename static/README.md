# Locally served assets

`htmx.min.js` is HTMX **2.0.8**, downloaded unchanged from:
https://raw.githubusercontent.com/bigskysoftware/htmx/v2.0.8/dist/htmx.min.js

License: `HTMX-LICENSE`, from the same release.

`app.js` handles session bootstrap, HTMX errors, page account/selection state,
focus restoration, token-field clearing, run-progress refresh, and merchant search.
`app.css` styles the interface. Assets are embedded by `src/web.rs`.
