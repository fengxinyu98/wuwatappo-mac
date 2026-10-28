#!/usr/bin/env bash
# Build "Wuwa Tappo.app" on macOS. Run from anywhere: ./build-mac.sh
set -euo pipefail
cd "$(dirname "$0")"

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Installing Xcode Command Line Tools; re-run this script when it finishes."
  xcode-select --install
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Installing Rust (rustup)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

if ! cargo tauri --version >/dev/null 2>&1; then
  echo "Installing tauri-cli..."
  cargo install tauri-cli --version "^2" --locked
fi

cd src-tauri
cargo tauri build --bundles app

APP="target/release/bundle/macos/Wuwa Tappo.app"
echo
echo "Built: $(pwd)/$APP"
echo "Install: cp -R \"$(pwd)/$APP\" /Applications/"
