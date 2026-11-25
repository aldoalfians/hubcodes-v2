#!/usr/bin/env bash
# Publish the bundle that `pnpm build:mac` produced as a GitHub release and
# write the updater feed in the form the app reads while the repo is private.
#
#   desktop/scripts/release-github.sh [--notes "text"] [--draft]
#
# Steps: tag v<version> from desktop/src-tauri/tauri.conf.json, upload the
# .dmg, hubcodes.app.tar.gz and its .sig, look up the tarball's asset id,
# rewrite latest.json so its url is the API asset URL
# (https://api.github.com/repos/<repo>/releases/assets/<id>; works with a
# token on a private repo and without one on a public repo), upload it, and
# pin the dmg sha256 in Casks/hubcodes.rb.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
repo="${HUBCODES_REPO:-HubStudio-id/hubcodes}"
out="$root/target/release/bundle"
notes=""
extra=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --notes) notes="$2"; shift 2 ;;
    --draft) extra+=(--draft); shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$root/desktop/src-tauri/tauri.conf.json")"
tag="v$version"
dmg="$(ls "$out"/dmg/hubcodes_"$version"_*.dmg 2>/dev/null | head -1 || true)"
tar="$out/macos/hubcodes.app.tar.gz"
[[ -f "$dmg" ]] || { echo "no dmg for $version in $out/dmg (run pnpm build:mac first)" >&2; exit 1; }
[[ -f "$tar" && -f "$tar.sig" ]] || { echo "no signed updater tarball in $out/macos" >&2; exit 1; }
built="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$out/macos/hubcodes.app/Contents/Info.plist")"
[[ "$built" == "$version" ]] || { echo "bundle is $built but tauri.conf.json says $version; rebuild" >&2; exit 1; }

if gh release view "$tag" -R "$repo" >/dev/null 2>&1; then
  echo "release $tag exists; uploading assets with --clobber"
  gh release upload "$tag" -R "$repo" --clobber "$dmg" "$tar" "$tar.sig"
else
  gh release create "$tag" -R "$repo" --title "hubcodes $version" --notes "${notes:-hubcodes $version}" ${extra[@]+"${extra[@]}"} "$dmg" "$tar" "$tar.sig"
fi

asset_id="$(gh api "repos/$repo/releases/tags/$tag" --jq '.assets[] | select(.name == "hubcodes.app.tar.gz") | .id')"
[[ -n "$asset_id" ]] || { echo "tarball asset not found on $tag" >&2; exit 1; }
# Keep the Linux entries the CI workflow (.github/workflows/linux.yml) may
# already have added to this release's feed.
prev="$(mktemp)"
gh release download "$tag" -R "$repo" -p latest.json -O "$prev" --clobber 2>/dev/null || echo '{}' > "$prev"
python3 - "$out/latest.json" "$repo" "$asset_id" "${notes:-}" "$prev" <<'PY'
import json, sys
path, repo, asset_id, notes, prev = sys.argv[1:6]
feed = json.load(open(path))
for p in feed["platforms"].values():
    p["url"] = f"https://api.github.com/repos/{repo}/releases/assets/{asset_id}"
if notes:
    feed["notes"] = notes
try:
    old = json.load(open(prev))
except Exception:
    old = {}
if old.get("version") == feed["version"]:
    for k, v in old.get("platforms", {}).items():
        if not k.startswith("darwin-"):
            feed["platforms"][k] = v
json.dump(feed, open(path, "w"), indent=2)
PY
rm -f "$prev"
gh release upload "$tag" -R "$repo" --clobber "$out/latest.json"

sha="$(shasum -a 256 "$dmg" | cut -d' ' -f1)"
cask="$root/Casks/hubcodes.rb"
if [[ -f "$cask" ]]; then
  sed -i '' -e "s/^  version \".*\"/  version \"$version\"/" -e "s/^  sha256 \".*\"/  sha256 \"$sha\"/" "$cask"
fi
echo
echo "released https://github.com/$repo/releases/tag/$tag"
echo "dmg sha256 $sha (pinned in Casks/hubcodes.rb)"
echo "feed url   https://api.github.com/repos/$repo/releases/assets/$asset_id"
