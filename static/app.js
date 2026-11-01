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

function pendingAccounts() {
  return [...document.querySelectorAll('[data-page-account]')].filter(select => select.value !== select.dataset.original);
}

function updateSelection() {
  const boxes = [...document.querySelectorAll('input[form="bulk-review"]:not(:disabled)')];
  const count = boxes.filter(box => box.checked).length;
  const label = document.getElementById("selection-count");
  if (label) label.textContent = `${count} selected on this page`;
  const all = document.getElementById("select-page");
  if (all) { all.checked = count > 0 && count === boxes.length; all.indeterminate = count > 0 && count < boxes.length; }
  document.querySelectorAll("[data-selection-action]").forEach(button => { button.disabled = count === 0 || pendingAccounts().length > 0; });
  const save = document.getElementById("save-page-accounts");
  if (save) save.disabled = pendingAccounts().length === 0;
}
let lastAccountFocus = null;
document.addEventListener("change", event => {
  if (event.target.matches('[data-page-account]')) {
    lastAccountFocus = event.target.id;
    const count = pendingAccounts().length;
    document.getElementById("account-edit-count").textContent = count ? `${count} unsaved account choices on this page` : "No unsaved account changes.";
  }
  if (event.target.id === "select-page") {
    document.querySelectorAll('input[form="bulk-review"]:not(:disabled)').forEach(box => { box.checked = event.target.checked; });
  }
  updateSelection();
});
let restoreFocus = null;
document.addEventListener("htmx:beforeRequest", event => {
  restoreFocus = event.detail.elt.id === "page-accounts" ? lastAccountFocus : null;
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

const rulePicker = document.getElementById("rule-picker");
if (rulePicker) {
  const search = document.getElementById("rule-merchant-search");
  const merchants = document.getElementById("rule-merchant");
  const cards = document.getElementById("rule-card");
  const status = document.getElementById("rule-search-status");
  const options = [...merchants.options];
  const normalize = value => value.trim().replace(/\s+/g, " ").toLowerCase();
  document.getElementById("rule-search-label").hidden = false;
  status.hidden = false;
  search.addEventListener("input", () => {
    const query = normalize(search.value);
    const selected = merchants.value;
    const matches = options.filter(option => !option.value || normalize(option.textContent).includes(query));
    merchants.replaceChildren(...matches);
    merchants.value = matches.some(option => option.value === selected) ? selected : "";
    status.textContent = `${matches.length - 1} merchants match`;
    updateCards();
  });
  function updateCards() {
    const seen = new Set(JSON.parse(merchants.selectedOptions[0]?.dataset.cards || "[]"));
    [...cards.options].forEach(option => {
      if (option.value) option.textContent = option.value + (seen.has(option.value) ? " · used for this merchant" : "");
    });
  }
  merchants.addEventListener("change", updateCards);
  // Editing always applies to the explicitly loaded scope, not an unsubmitted selection.
  rulePicker.addEventListener("change", () => {
    const editor = document.getElementById("account-rule-editor");
    if (editor) editor.hidden = true;
  });
}
