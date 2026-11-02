{ pkgs, config, ... }:

let
  rust = config.languages.rust.toolchainPackage;
  llvmTools = "${rust}/lib/rustlib/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/bin";
  # The pinned nixpkgs package tests Rust 1.99 with incompatible LLVM 22 tools.
  cargo-llvm-cov = (pkgs.cargo-llvm-cov.override {
    rustPlatform = pkgs.makeRustPlatform {
      cargo = rust;
      rustc = rust;
    };
  }).overrideAttrs (old: {
    env = old.env // {
      LLVM_COV = "${llvmTools}/llvm-cov";
      LLVM_PROFDATA = "${llvmTools}/llvm-profdata";
    };
  });
in
{
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
      "llvm-tools-preview"
    ];
  };

  packages = with pkgs; [
    cargo-deny
    cargo-llvm-cov
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

  scripts.recon-coverage.exec = ''
    set -euo pipefail
    cargo llvm-cov --all-targets --all-features
    cargo llvm-cov report --html
  '';

  scripts.recon-browser-check.exec = ''
    set -euo pipefail
    cargo build
    exec python3 tools/browser_smoke.py
  '';
}
