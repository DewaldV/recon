"""Synthetic Firefox/WebDriver smoke test; no Node or Python packages needed.

devenv provides Python and geckodriver. Firefox must be available on PATH, or set
RECON_FIREFOX to its executable. The app and driver are stopped in finally blocks.
No financial files or default app database are read.
"""

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent
ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


def call(method, path, data=None):
    body = None if data is None else json.dumps(data).encode()
    request = Request("http://127.0.0.1:4444" + path, body, method=method)
    request.add_header("Content-Type", "application/json")
    with urlopen(request, timeout=30) as response:
        result = json.load(response)["value"]
    if isinstance(result, dict) and "error" in result:
        raise RuntimeError("WebDriver command failed: " + result["error"])
    return result


def start_app(directory):
    process = subprocess.Popen(
        [str(ROOT / "target/debug/recon"), "--data-dir", directory],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )
    for line in process.stdout:
        if line.startswith("http://127.0.0.1:7310/start#"):
            return process, line.strip()
    process.wait(timeout=5)
    raise RuntimeError("Recon failed to start: " + process.stderr.read())


def stop(process):
    if process and process.poll() is None:
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def run():
    browser = os.environ.get("RECON_FIREFOX") or shutil.which("firefox")
    if not browser:
        raise RuntimeError("Install Firefox or set RECON_FIREFOX to its executable.")
    driver = app = None
    session = None
    with tempfile.TemporaryDirectory(prefix="recon-browser-", dir="/tmp/opencode") as data:
        try:
            driver = subprocess.Popen(
                ["geckodriver", "--host", "127.0.0.1", "--port", "4444"],
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
            )
            for line in driver.stdout:
                if "Listening on" in line:
                    break
            else:
                raise RuntimeError("WebDriver could not bind port 4444.")
            session = call("POST", "/session", {"capabilities": {"alwaysMatch": {
                "browserName": "firefox",
                "moz:firefoxOptions": {"binary": browser, "args": ["-headless"], "prefs": {
                    "datareporting.healthreport.uploadEnabled": False,
                    "toolkit.telemetry.enabled": False,
                    "browser.shell.checkDefaultBrowser": False,
                }},
            }}})["sessionId"]
            prefix = "/session/" + session

            def execute(script):
                return call("POST", prefix + "/execute/sync", {"script": script, "args": []})

            def wait(script):
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    if execute(script):
                        return
                    time.sleep(0.1)
                # This test uses synthetic inputs only; diagnostic page text is safe.
                raise AssertionError("Browser condition timed out: " + script + "\n" + execute("return document.body.textContent"))

            def find(selector):
                return call("POST", prefix + "/element", {"using": "css selector", "value": selector})[ELEMENT]

            def click(selector):
                call("POST", prefix + "/element/" + find(selector) + "/click", {})

            def navigate(url):
                call("POST", prefix + "/url", {"url": url})

            def import_file():
                field = find("input[type=file]")
                path = str(ROOT / "tests/fixtures/virgin-first.csv")
                call("POST", prefix + "/element/" + field + "/value", {"text": path})
                click("button[type=submit]")
                wait("return document.body.textContent.includes('3 source records accounted for')")

            app, url = start_app(data)
            navigate(url)
            wait("return !!document.querySelector('input[type=file]')")
            assert execute("return location.hash") == ""
            import_file()
            click("a.button")
            wait("return !!document.querySelector('#purchase-1 select')")
            assert execute("return typeof htmx !== 'undefined' && !htmx.config.allowEval && !htmx.config.allowScriptTags")
            execute("window.reconSmokeMarker = 'no-navigation'; return true")
            click("#purchase-1 option[value=personal]")
            click("#purchase-1 button")
            wait("return document.querySelector('#purchase-1 input[name=version]').value === '1'")
            assert execute("return window.reconSmokeMarker") == "no-navigation", "Expected HTMX update, not full-page fallback"
            assert execute("return document.querySelector('#purchase-1 select').value") == "personal"
            call("POST", prefix + "/refresh", {})
            wait("return document.querySelector('#purchase-1 select')?.value === 'personal'")
            stop(app)
            app, url = start_app(data)
            navigate("http://127.0.0.1:7310/purchases")
            assert execute("return document.body.textContent.includes('startup link')")
            navigate(url)
            wait("return !!document.querySelector('input[type=file]')")
            import_file()
            click("a.button")
            wait("return document.querySelector('#purchase-1 select')?.value === 'personal'")
            assert execute("return document.querySelectorAll('tr[id^=purchase-]').length") == 2
            # Increment 2: catalogue preview, coalescing, bulk review, and overrides.
            navigate("http://127.0.0.1:7310/review")
            wait("return document.body.textContent.includes('2 distinct purchases')")
            field = find("form[action='/review/catalogue'] input[type=file]")
            catalogue_path = str(ROOT / "tests/fixtures/merchants.csv")
            call("POST", prefix + "/element/" + field + "/value", {"text": catalogue_path})
            click("form[action='/review/catalogue'] button")
            wait("return document.body.textContent.includes('Import merchant catalogue')")
            assert execute("return document.querySelectorAll('input[name^=record_]:checked').length") == 2
            click("input[name=allow_categories]")
            click("form button")
            wait("return document.body.textContent.includes('Synthetic groceries 🛍️')")
            click("#review-1 input[type=checkbox]")
            click("#review-2 input[type=checkbox]")
            click("button[value=accounts]")
            wait("return document.body.textContent.includes('2 potentially affected purchases')")
            click("form button")
            wait("return !!document.querySelector('#review-1')")
            navigate("http://127.0.0.1:7310/review?view=complete&import_id=")
            wait("return document.body.textContent.includes('2 distinct purchases')")
            click("#review-1 input[type=checkbox]")
            execute("document.querySelector('#bulk-review input[name=category]').value = 'Shopping'; document.querySelector('#bulk-review textarea').value = 'Explicit override 🧾'; return true")
            click("button[value=details]")
            wait("return document.body.textContent.includes('Explicit override 🧾')")
            click("form button")
            wait("return document.querySelector('#review-1').textContent.includes('Explicit override 🧾')")
            navigate("http://127.0.0.1:7310/review?q=SYNTHETIC%20SHOP")
            execute("document.querySelector('input[value=default]').form.querySelector('textarea').value = 'Changed default'; return true")
            click("input[value=default] + label + label + label + button")
            wait("return document.body.textContent.includes('1 purchases have explicit overrides')")
            click("form button")
            wait("return document.querySelector('#review-2').textContent.includes('Changed default')")
            assert execute("return document.querySelector('#review-1').textContent.includes('Explicit override 🧾')")
            click("#review-1 input[type=checkbox]")
            click("button[value=details]")
            wait("return document.body.textContent.includes('Set selected purchase overrides')")
            click("form button")
            wait("return document.querySelector('#review-1').textContent.includes('Changed default')")
            stop(app)
            app, url = start_app(data)
            navigate(url)
            wait("return !!document.querySelector('input[type=file]')")
            navigate("http://127.0.0.1:7310/review?view=complete")
            wait("return document.body.textContent.includes('2 distinct purchases')")
            assert execute("return document.querySelector('#review-1').textContent.includes('Changed default')")
            print("Browser smoke passed: merchant catalogue, bulk review, overrides, defaults, filters, restart; session, upload, HTMX edit, refresh, restart, duplicate import.")
        finally:
            if session:
                try:
                    call("DELETE", "/session/" + session)
                except Exception:
                    pass
            stop(app)
            stop(driver)


if __name__ == "__main__":
    run()
