# hubcodes installer for Windows: hubcodes needs tmux, so it runs inside
# WSL 2 (Ubuntu). This script checks WSL, then runs the Linux installer
# (install.sh) inside it. The desktop app opens as a Linux window through
# WSLg (Windows 11, or Windows 10 with a recent WSL).
#
#   gh api -H "Accept: application/vnd.github.raw" repos/HubStudio-id/hubcodes/contents/install.ps1 | powershell -NoProfile -Command -
#
# Run it in a normal (not administrator) PowerShell. Set $env:HUBCODES_DISTRO
# to install into a WSL distribution other than the default one.

$ErrorActionPreference = 'Stop'
$repo = if ($env:HUBCODES_REPO) { $env:HUBCODES_REPO } else { 'HubStudio-id/hubcodes' }
$distro = $env:HUBCODES_DISTRO

function Say($m) { Write-Host $m -ForegroundColor Cyan }

if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
  Say 'WSL is not installed. In an administrator PowerShell run:  wsl --install -d Ubuntu'
  Say 'then restart Windows, open Ubuntu once to create your Linux user, and run this installer again.'
  exit 1
}

$wslArgs = @()
if ($distro) { $wslArgs += @('-d', $distro) }

# A Linux distribution must exist and start.
wsl.exe @wslArgs -- true 2>$null
if ($LASTEXITCODE -ne 0) {
  Say 'No WSL Linux distribution found. In an administrator PowerShell run:  wsl --install -d Ubuntu'
  Say 'then open Ubuntu once to create your Linux user, and run this installer again.'
  exit 1
}

# The release is private: the installer inside WSL needs gh signed in there.
$script = @"
set -e
command -v curl >/dev/null || { sudo apt-get update && sudo apt-get install -y curl; }
command -v tmux >/dev/null || { sudo apt-get update && sudo apt-get install -y tmux; }
if ! command -v gh >/dev/null; then
  echo 'Installing gh (GitHub CLI) inside WSL'
  sudo apt-get update && sudo apt-get install -y gh
fi
if ! gh auth status >/dev/null 2>&1; then
  echo 'Sign in to GitHub inside WSL with your HubStudio-id account:'
  gh auth login --hostname github.com --git-protocol https --web
fi
gh api -H 'Accept: application/vnd.github.raw' repos/$repo/contents/install.sh | sh
grep -q 'HOME/.local/bin' ~/.bashrc 2>/dev/null || echo 'export PATH="`$HOME/.local/bin:`$PATH"' >> ~/.bashrc
"@

Say "hubcodes: installing inside WSL $(if ($distro) { $distro } else { '(default distribution)' })"
wsl.exe @wslArgs -- bash -lc $script
if ($LASTEXITCODE -ne 0) { Say 'The installer inside WSL failed; see the messages above.'; exit $LASTEXITCODE }

Say 'hubcodes installed in WSL. Start it from Windows with:'
Say '  wsl bash -lc hubcodes-desktop      (the canvas, through WSLg)'
Say '  wsl bash -lc "hubcodes setup"      (skills in every coding tool, then a health check)'
Say 'Coding tools (claude, codex, opencode...) must be installed inside WSL too.'
