#!/usr/bin/env bash
# Build hubcodes.app and a .dmg, signed with the best identity in the keychain.
#
#   Developer ID Application  -> distributable; set APPLE_ID, APPLE_PASSWORD
#                                (app-specific) and APPLE_TEAM_ID to notarize too
#   Apple Development         -> runs on this Mac and registered devices
#   none                      -> ad-hoc signature ("-"), Gatekeeper will warn
#
# Override with APPLE_SIGNING_IDENTITY=<name or SHA-1> pnpm build:mac
set -euo pipefail
cd "$(dirname "$0")/.."

pick() {
  security find-identity -v -p codesigning 2>/dev/null | grep -o "\"$1[^\"]*\"" | head -1 | tr -d '"' || true
}
if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  APPLE_SIGNING_IDENTITY="$(pick 'Developer ID Application')"
  [[ -n "$APPLE_SIGNING_IDENTITY" ]] || APPLE_SIGNING_IDENTITY="$(pick 'Apple Development')"
  [[ -n "$APPLE_SIGNING_IDENTITY" ]] || APPLE_SIGNING_IDENTITY="-"
fi
export APPLE_SIGNING_IDENTITY
echo "signing identity: $APPLE_SIGNING_IDENTITY"
if [[ "$APPLE_SIGNING_IDENTITY" != Developer\ ID* ]]; then
  echo "note: not a Developer ID certificate, so no notarization; other Macs will see a Gatekeeper warning."
fi

# Updater artifacts are signed with the minisign key; Tauri reads the key *content* from
# TAURI_SIGNING_PRIVATE_KEY. Default to ~/.tauri/hubcodes.key when it exists.
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  keyfile="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.tauri/hubcodes.key}"
  if [[ -f "$keyfile" ]]; then
    TAURI_SIGNING_PRIVATE_KEY="$(cat "$keyfile")"
    export TAURI_SIGNING_PRIVATE_KEY
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  fi
fi

pnpm tauri build --bundles app "$@"

# Tauri's bundle_dmg.sh drives Finder with AppleScript and fails with
# "couldn't unmount: Resource busy" whenever Finder or Spotlight holds the
# interstitial volume. hdiutil alone is deterministic.

# Cargo workspace: the target dir is at the repo root, not under src-tauri.
out="$(cd src-tauri && cargo metadata --format-version 1 --no-deps | python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])')/release/bundle"
app="$out/macos/hubcodes.app"
version="$(python3 -c 'import json; print(json.load(open("src-tauri/tauri.conf.json"))["version"])')"
arch="$(uname -m)"
[[ "$arch" == "arm64" ]] && arch="aarch64"
dmg="$out/dmg/hubcodes_${version}_${arch}.dmg"
staging="$(mktemp -d)"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
mkdir -p "$out/dmg"
rm -f "$dmg"
hdiutil create -quiet -volname hubcodes -srcfolder "$staging" -ov -format UDZO "$dmg"
rm -rf "$staging"
if [[ "$APPLE_SIGNING_IDENTITY" != "-" ]]; then
  codesign --force --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
fi

# Notarize and staple when a Developer ID and the App Store Connect credentials are present.
if [[ "$APPLE_SIGNING_IDENTITY" == Developer\ ID* && -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  echo "notarizing $dmg (this takes a few minutes)"
  xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
  xcrun stapler staple "$app"
  xcrun stapler staple "$dmg"
fi

# Updater feed: the signed .app.tar.gz (from createUpdaterArtifacts) plus latest.json.
# Needs TAURI_SIGNING_PRIVATE_KEY_PATH (or TAURI_SIGNING_PRIVATE_KEY) at build time.
tarball="$(ls "$out"/macos/*.app.tar.gz 2>/dev/null | head -1 || true)"
if [[ -n "$tarball" && -f "$tarball.sig" ]]; then
  base_url="${HUBCODES_RELEASE_URL:-https://github.com/HubStudio-id/hubcodes/releases/download/v$version}"
  python3 - "$tarball" "$version" "$base_url" <<'PY' > "$out/latest.json"
import json, sys, datetime, os
tar, version, base = sys.argv[1:4]
sig = open(tar + ".sig").read().strip()
print(json.dumps({
  "version": version,
  "notes": os.environ.get("HUBCODES_RELEASE_NOTES", f"hubcodes {version}"),
  "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
  "platforms": {"darwin-aarch64": {"signature": sig, "url": f"{base}/{os.path.basename(tar)}"}},
}, indent=2))
PY
  echo "updater feed: $out/latest.json (upload with $tarball and its .sig to the release)"
else
  echo "note: no signed updater artifact (set TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/hubcodes.key to produce one)"
fi

echo
echo "bundle:"
ls -1 "$app" "$dmg"
codesign -dv --verbose=2 "$out"/macos/*.app 2>&1 | grep -E '^(Identifier|Authority|Signature|TeamIdentifier)' || true
