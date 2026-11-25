#!/usr/bin/env bash
# Build the `hubcodes` CLI and place it where Tauri picks it up as a sidecar
# (src-tauri/binaries/hubcodes-<target-triple>). Run before `tauri dev|build`.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
triple="$(rustc -vV | sed -n 's/^host: //p')"
profile="${1:-release}"
if [[ "$profile" == "release" ]]; then
  cargo build --release -p hubcodes --manifest-path ../Cargo.toml
  src="../target/release/hubcodes"
else
  cargo build -p hubcodes --manifest-path ../Cargo.toml
  src="../target/debug/hubcodes"
fi
mkdir -p src-tauri/binaries
cp "$src" "src-tauri/binaries/hubcodes-$triple"
echo "sidecar: src-tauri/binaries/hubcodes-$triple"
