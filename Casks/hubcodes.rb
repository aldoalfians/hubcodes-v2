# Homebrew cask for hubcodes. Lives in the tap repo HubStudio-id/homebrew-hubcodes
# (copy this file there as Casks/hubcodes.rb); then:
#
#   brew install --cask HubStudio-id/hubcodes/hubcodes
#
# `desktop/scripts/release-mac.sh` prints the sha256 to paste here on every release.
cask "hubcodes" do
  version "0.1.40"
  sha256 "2986cf7cc8db4816ef93db9a88319313fcc151c5ff4999dd46cc7f47551d5e46"

  url "https://github.com/HubStudio-id/hubcodes/releases/download/v#{version}/hubcodes_#{version}_aarch64.dmg"
  name "hubcodes"
  desc "Orchestrator for AI coding agents on tmux: canvas, board, shared sessions"
  homepage "https://github.com/HubStudio-id/hubcodes"

  depends_on arch: :arm64
  depends_on formula: "tmux"

  app "hubcodes.app"
  binary "#{appdir}/hubcodes.app/Contents/MacOS/hubcodes"

  # Signed, not notarized yet: drop the quarantine flag so Gatekeeper opens it.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/hubcodes.app"], sudo: false
  end

  zap trash: [
    "~/.config/hubcodes",
    "~/.local/state/hubcodes",
  ]
end
