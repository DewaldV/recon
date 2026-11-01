# Locally served assets

`htmx.min.js` is HTMX **2.0.8**, downloaded unchanged from:
https://raw.githubusercontent.com/bigskysoftware/htmx/v2.0.8/dist/htmx.min.js

License: `HTMX-LICENSE`, from the same release. No CDN is used at runtime.

`app.js` handles the startup fragment exchange and accessible HTMX error feedback.
Imported text is escaped by Askama; it is never interpreted as script or markup.
HTMX evaluation/script execution and history caching are disabled by page config.
