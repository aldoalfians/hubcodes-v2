---
name: hubcodes
description: Install, update, set up and check hubcodes, and manage the skills every coding tool on this Mac shares (list, add, sync, pull, copy from another device). Use when the user asks to install or update hubcodes, set it up, keep skills in sync across Claude Code, Codex, OpenCode, Pi, OMP and Gemini, add a skill from a file or link, pull a skills repository, or fetch a skill from another device such as the Mac mini.
---

<!-- hubcodes:builtin — written by `hubcodes setup`; replaced on upgrade. Copy it under another name to customise. -->

# hubcodes

hubcodes runs several coding agents side by side on tmux and keeps their skills in sync. Everything below is a shell command; run it, read the output, and report what changed. Do not edit `~/.config/hubcodes/hubcodes.toml` by hand when a command below does the job.

## Install or update

hubcodes lives in the private repository `HubStudio-id/hubcodes`. The user needs `gh` signed in with an account that can read it. Builds exist for macOS on Apple silicon, Linux x86_64 and aarch64, and Windows through WSL 2.

macOS or Linux (also inside WSL):

```bash
command -v gh >/dev/null || brew install gh            # Linux: sudo apt install gh (ask before sudo)
gh auth status || echo "ask the user to run: gh auth login"   # never type their password or token
command -v tmux >/dev/null || brew install tmux        # Linux: sudo apt install tmux libfuse2
gh api -H "Accept: application/vnd.github.raw" repos/HubStudio-id/hubcodes/contents/install.sh | sh
```

Windows (PowerShell, outside WSL): `gh api -H "Accept: application/vnd.github.raw" repos/HubStudio-id/hubcodes/contents/install.ps1 | powershell -NoProfile -Command -`. It needs WSL with a Linux distribution; without one, tell the user to run `wsl --install -d Ubuntu` in an administrator PowerShell and restart. Do not change Windows features yourself.

Already installed: `hubcodes update --check` reports the installed and the latest version (exit code 10 when a newer one, patch releases included, is out) and `hubcodes update` installs it. `hubcodes doctor` shows the same on its Version line.

`gh api` answering `404 Not Found` means the account has no access yet: tell the user to ask a repository admin. On macOS the installer replaces `/Applications/hubcodes.app` (quitting a running copy first) and links `~/.local/bin/hubcodes`; on Linux it writes `~/.local/bin/hubcodes` and `~/.local/bin/hubcodes-desktop` (an AppImage). Running it again updates. If `hubcodes` is not found afterwards, `~/.local/bin` is missing from `PATH`: use the full path and tell the user.

## Set up

```bash
hubcodes setup                               # skills sync on, this skill installed, sync once, then a health check
hubcodes setup --source ~/.hub-skills/skills # also link a folder of skills (repeatable)
hubcodes setup --drop-source <path>          # stop linking a folder
hubcodes doctor                              # tmux, PATH, providers, daemon; exit 1 when something fails
```

`setup` is safe to run again. It never removes a skill or overwrites a folder the user made. Fix what `doctor` reports as failing, or tell the user the exact command to run when it needs them (a login, a permission prompt).

## Skills

Each tool loads skills from its own folder (`~/.claude/skills`, `~/.codex/skills`, `~/.config/opencode/skills`, `~/.pi/agent/skills`, `~/.omp/agent/skills`, `~/.gemini/skills`) plus the shared `~/.agents/skills`. With sync on, hubcodes links every skill into every installed tool each minute, so a skill added once works everywhere.

```bash
hubcodes skill list                     # skills here and which tools have each
hubcodes skill list --device mini       # skills on another device
hubcodes skill add <source>             # folder, SKILL.md, .zip/.skill, download link, GitHub link to one skill, or a git repository
hubcodes skill copy <name> --from mini  # fetch one from another device and link it into every tool
hubcodes skill pull                     # git pull every skills repository, then link what is new
hubcodes skill sync                     # link everything now instead of waiting a minute
hubcodes skill show <name>              # print a skill's SKILL.md
```

A skill is instructions an agent follows. Before `skill add` from a link the user did not give you, show them the source and ask. An existing skill of the same name is never replaced; say so when `add` skips one.

## Report

End with what you ran, what changed (skills added or linked, settings changed) and anything the user still has to do.
