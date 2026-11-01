{ pkgs, ... }:

{
  languages.rust = {
    enable = true;
    channel = "stable";
  };

  packages = with pkgs; [
    cargo-deny
    cargo-watch
    sqlite
    pkg-config
    openssl
    geckodriver
    python3
  ];

  scripts.recon-check.exec = ''
    set -euo pipefail
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-targets --all-features
  '';

  scripts.recon-dev.exec = ''
    exec cargo watch -x 'run -- --data-dir /tmp/opencode/recon-development'
  '';

  scripts.recon-browser-check.exec = ''
    set -euo pipefail
    cargo build
    exec python3 tools/browser_smoke.py
  '';
}
