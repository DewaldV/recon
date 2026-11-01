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
    : "Could not save this change. Reload and try again; if the app restarted, use its new startup link.";
  banner.hidden = false;
});
document.addEventListener("htmx:afterRequest", event => {
  if (event.detail.successful) {
    const banner = document.getElementById("request-error");
    if (banner) banner.hidden = true;
  }
});
