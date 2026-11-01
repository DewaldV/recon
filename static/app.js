"use strict";

const bootstrapStatus = document.getElementById("bootstrap-status");
if (bootstrapStatus) {
  const token = location.hash.slice(1);
  history.replaceState(null, "", "/start");
  if (!/^[a-f0-9]{64}$/.test(token)) {
    bootstrapStatus.textContent = "Open the private startup link printed by the running Recon process.";
  } else {
    fetch("/session", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ token }),
      credentials: "same-origin",
      cache: "no-store",
    }).then(response => {
      if (!response.ok) throw new Error("Session rejected");
      location.replace("/");
    }).catch(() => {
      bootstrapStatus.textContent = "Could not open a session. Use the startup link from the current app process.";
    });
  }
}

document.addEventListener("htmx:responseError", event => {
  const banner = document.getElementById("request-error");
  if (!banner) return;
  banner.textContent = event.detail.xhr.status === 409
    ? "This decision changed in another tab. Reload before editing again."
    : new DOMParser().parseFromString(event.detail.xhr.responseText, "text/html").querySelector("#error-message")?.textContent || "Could not save this change. Reload and try again; if the app restarted, use its new startup link.";
  banner.hidden = false;
});
document.addEventListener("htmx:afterRequest", event => {
  if (event.detail.successful) {
    const banner = document.getElementById("request-error");
    if (banner) banner.hidden = true;
  }
});

function updateSelection() {
  const boxes = [...document.querySelectorAll('input[form="bulk-review"]:not(:disabled)')];
  const count = boxes.filter(box => box.checked).length;
  const label = document.getElementById("selection-count");
  if (label) label.textContent = `${count} selected on this page`;
  const all = document.getElementById("select-page");
  if (all) { all.checked = count > 0 && count === boxes.length; all.indeterminate = count > 0 && count < boxes.length; }
  document.querySelectorAll("[data-selection-action]").forEach(button => { button.disabled = count === 0; });
}
document.addEventListener("change", event => {
  if (event.target.id === "select-page") {
    document.querySelectorAll('input[form="bulk-review"]:not(:disabled)').forEach(box => { box.checked = event.target.checked; });
  }
  updateSelection();
});
let restoreFocus = null;
document.addEventListener("htmx:beforeRequest", event => {
  restoreFocus = event.detail.elt.querySelector("select[id]")?.id || null;
});
document.addEventListener("htmx:afterSwap", () => {
  updateSelection();
  if (restoreFocus) document.getElementById(restoreFocus)?.focus({ preventScroll: true });
  restoreFocus = null;
});
updateSelection();
document.addEventListener("click", event => { if (event.target.closest("[data-back]")) history.back(); });
document.addEventListener("formdata", event => {
  // The FormData already contains the submission; don't retain credentials in form history.
  event.target.querySelectorAll('input[name="token"]').forEach(input => { input.value = ""; });
});
if (document.querySelector("#run-progress[data-refresh=true]")) setTimeout(() => location.reload(), 2000);
