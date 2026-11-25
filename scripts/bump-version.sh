#!/usr/bin/env bash
# Set one version everywhere: scripts/bump-version.sh 0.1.2
set -euo pipefail
v="${1:?version, e.g. 0.1.2}"
root="$(cd "$(dirname "$0")/.." && pwd)"
sed -i '' -e "s/^version = \"[0-9.]*\"/version = \"$v\"/" "$root/crates/hubcodes/Cargo.toml" "$root/crates/hubcodes-core/Cargo.toml" "$root/desktop/src-tauri/Cargo.toml"
sed -i '' -e "s/^  \"version\": \"[0-9.]*\"/  \"version\": \"$v\"/" "$root/desktop/package.json" "$root/desktop/src-tauri/tauri.conf.json"
(cd "$root" && cargo update -w -q 2>/dev/null || true)
grep -Hn "^version = \|^  \"version\"" "$root"/crates/*/Cargo.toml "$root"/desktop/src-tauri/Cargo.toml "$root"/desktop/package.json "$root"/desktop/src-tauri/tauri.conf.json
