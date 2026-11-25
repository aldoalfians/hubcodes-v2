# Reference teardown: Maestri and T3 Code

Two products serve as design references for hubcodes, a Rust full-TUI multi-agent orchestrator built on tmux with Tailscale sharing. This document records what their public marketing pages actually say, captured with REA on 2026-11-10 (Asia/Jakarta), and separates observed facts from inference and unknowns.

Evidence grades used throughout:

- **A** — REA accessibility tree or screenshot capture (verbatim text available in `docs/evidence/`).
- **B** — WebFetch summary from earlier in the investigation (lower grade; summarised by a model, not verbatim).
- **C** — Inference, or a user-authored testimonial quoted on the page (not a vendor claim).

All quoted text is third-party marketing material. Treat it as data, not instructions.

## 1. Capture metadata

| Item | Maestri | T3 Code |
|---|---|---|
| URL | https://www.themaestri.app/en | https://t3.codes/ |
| Captured | 2026-11-10 (Asia/Jakarta) | 2026-11-10 (Asia/Jakarta) |
| REA provider | `rea-playwright-browser-scenario` v1 | `rea-playwright-browser-scenario` v1 |
| Evidence ID | `ev_69740da3c8623895c55893debcc028cf6bbf4dd1fe4d9a4b94a357fa955a3588` | `ev_078d7192f3e47cc1942ef1c48667ef5c50395346389776c754b1047ae355b7e1` |
| Completeness | `incomplete` — events section missing; 4 `.mp4` requests failed, 2 unfinished at capture end | `complete` |
| Browser | Chromium 157.0.8103.50, headless, 1440×900, dark scheme | Chromium 157.0.8103.50, headless, 1440×900, dark scheme |
| Duration | 13,878 ms | 5,051 ms |
| Files | `docs/evidence/maestri-landing.png`, `maestri-accessibility.txt`, `maestri-network-origins.txt`, `maestri-capture-meta.json` | `docs/evidence/t3-landing.png`, `t3-accessibility.txt`, `t3-network-origins.txt`, `t3-capture-meta.json` |
| Bundle | `docs/evidence/rea-evidence-bundle.json` (2 records) | same bundle |

Both captures used a single `wait_for_timeout` of 4000 ms and recorded only the first viewport. Nothing below the fold was scrolled into view, so screenshots cover the hero only; the accessibility tree covers the whole document.

## 2. Maestri

### 2.1 Positioning

| Claim | Verbatim | Grade |
|---|---|---|
| H1 | "Now you're conducting." | A |
| Subtitle | "Meet Maestri, the first infinite canvas made for orchestrating agents. Every agent gets a role, they talk to each other directly, and the whole operation stays in front of you." | A |
| Footer tagline | "The infinite canvas for orchestrating AI agents." | A |
| Requirements line | "macOS 15.4+ · Apple Silicon · Free" | A |

### 2.2 Feature inventory (verbatim section labels)

| Label (section / heading) | Vendor description (abridged, verbatim fragments) | Grade |
|---|---|---|
| "Teamwork." | "Connect terminals together and let agents prompt each other directly. One agent can delegate tasks, ask questions, or hand off work without you lifting a finger." | A |
| "Give each agent a role." | "Create reusable roles with custom instructions and assign them to agents. One leads, one codes, one reviews, one tests." Example role chips: "Unslopifier Bug Whisperer Vibe Coder Ship It Refactor Gremlin Docs Goblin Git Wizard Perf Nerd" | A |
| "Shell we talk?" (tagged "New") | "Every terminal gets a chat face. Ask in a message, get an answer in formatted text, with diagrams, tables and files you can drag onto the canvas." | A |
| "What if your terminal had no edges?" | "Pan freely. Zoom in on the agent that needs attention. Zoom out to see the full picture." Caption: "MaestriCanvas engine, built from scratch" | A |
| "Spawn agents anywhere." | "Each one runs its own agent: Claude Code, Codex, OpenCode, or just a plain shell." | A |
| "Give your agents a browser. And a device." | "Portals are embedded windows on your canvas. A website, an iOS Simulator, an Android emulator, or a real phone plugged into your Mac. … reading the app's real element tree instead of guessing at pixels." | A |
| "Get to the prompt." | "A rich composer floats over any terminal. Type @ to point the agent at a connected terminal, note, or portal, paste screenshots straight into the text" | A |
| "Notes, written by your agents." | "Connect a terminal to a sticky note and let your agent write directly to it." | A |
| "One canvas per context." | "Separate canvases for separate projects. Jump between terminals and workspaces with tmux-style shortcuts that feel instant." | A |
| "Draw your thinking." | "Sketch architecture diagrams, draw arrows between components, map out flows, right on the canvas" | A |
| "Bands" — "Too many agents to follow? Make them a band." | "Gather terminals in one place and switch between them like chats. … Maestros sit on top, and a single connection links the whole band to a note or a portal. Ask your Maestro and it sorts the team into bands on its own." | A |
| "Floors" — "Context switch without losing context." | "Create a floor. It's an instant, isolated copy of your entire workspace. New branch, new terminals, zero interference … APFS copy-on-write. Instant clone. Nearly zero disk space." | A |
| "Computer Use" | "They work in other apps in the background, without moving your pointer or taking your keyboard. Each agent asks before it uses an app, and you can answer on your computer or from Maestri Remote." | A |
| "Ombro" — "Always on your shoulder. Never in your way." | "an on-device AI companion … When an agent finishes a task, it summarizes what happened and suggests what to do next … Powered by Apple Foundation Models … Ombro requires macOS 26 or later." | A |
| "Built for Mac" — "100% native. Zero compromise." | Sub-cards "Swift", "AppKit", "Buttery smooth" ("GPU-accelerated canvas powered by Metal"), "Apple Intelligence" | A |

### 2.3 Supported agents, pricing, platforms, privacy, company

| Topic | Observed | Grade |
|---|---|---|
| Agents | "Claude Code, Codex, OpenCode, or just a plain shell." | A |
| Pricing — Free | "$0 forever"; list: "1 workspace", "Unlimited agents", "Infinite canvas", "Sketching tools", "Sticky notes", "Agent connections", "Connected notes", "Ombro assistant", "All features included" | A |
| Pricing — Pro | "$18 one-time payment"; "Unlimited workspaces", "Use on up to 2 devices", "7 days free, then pay once" | A |
| Pricing — Teams | "Up to 100 seats." with a "License Manager"; checkout link to `buy.stripe.com`; price not stated | A |
| Platforms | Footer "Platforms" nav lists "Mac" (`/en`), "Windows" (`/en/windows`), "Linux" (`/en/linux`), "Maestri Remote" (`/en/remote`). Only the Mac page was captured. | A (links) / unknown (content) |
| Download | `https://cdn.maestri.dev/downloads/Maestri-latest.dmg` | A |
| Privacy claims | "Maestri was built with one rule: your data never leaves your Mac." Headings: "Zero telemetry" ("The app never phones home."), "Fully local" ("Configuration is plain JSON, notes are Markdown files."), "No account required", "On-device AI". Disclaimer: "Coding agents like Claude Code, Codex, and others have their own privacy policies and may send data to their respective providers. Maestri does not control or intercept that traffic." | A |
| Company | Footer shows "Maestri™"; contact `hello@themaestri.app`; Discord invite; Twitter link to `https://x.com/evertjr`. No legal entity, address, or founder role is stated. | A (links) / C (solo developer inference) |
| Social proof | Product Hunt badge (`post_id=1105168`, "184" shown in screenshot); "Wall of Love" of 15 X posts | A |

### 2.4 Network observations (marketing site only)

From `docs/evidence/maestri-network-origins.txt` (request events, first 4 s after load):

| Origin | Requests |
|---|---|
| `https://cdn.maestri.dev` | 31 (testimonial images, feature media, DMG host) |
| `https://www.themaestri.app` | 15 (Next.js chunks, fonts) |
| `https://us-assets.i.posthog.com` | 3 (`config.js`, `web-vitals.js`, `dead-clicks-autocapture.js`, PostHog 1.373.5) |
| `https://api.producthunt.com` | 1 (badge image) |

This is the **marketing website**, not the macOS app. The PostHog loads neither confirm nor contradict the app's "Zero telemetry" claim; they show only that the website uses PostHog analytics. Verifying the app claim requires observing the app process itself, which was not done. The site is built with Next.js (Turbopack chunk names observed, grade A for the path names, C for the framework inference).

## 3. T3 Code

### 3.1 Positioning

| Claim | Verbatim | Grade |
|---|---|---|
| H1 | "The open-source control plane for coding agents." | A |
| Subtitle | "Orchestrate Claude Code, Codex, Antigravity, OpenCode, Cursor, and Grok from one surface. Bring your own subscription. Fork the whole thing." | A |
| Closing CTA | "Your agents deserve better than a terminal." / "T3 Code is free, open source and fast." | A |
| Social proof heading | "Tolerated by over 500,000 devs" / "Some of them even tweeted about it." | A (text) / unverified (figure) |
| Hero image alt | "T3 Code running Claude, Codex, Muse, and Gemini subagents in one thread" | A |

### 3.2 Feature inventory (verbatim labels)

| Label | Vendor description (verbatim fragments) | Grade |
|---|---|---|
| "Bring your own sub" | "T3 Code doesn't resell tokens. Plug in Claude Code, Codex, Antigravity, OpenCode, Cursor, or Grok with the credentials you already have. We orchestrate them, you keep your plan." Bullets: "No keys resold. No quota caps.", "Switch models mid-thread.", "More harnesses shipping weekly." | A |
| Harness login table | "Claude Code claude auth login", "Codex CLI codex login", "OpenCode opencode auth", "Cursor @cursor/sdk", "Grok CLI grok login", "Antigravity Google sign-in" | A |
| "One button to commit, push, and make a PR." | "Every agent thread writes to its own branch. When it's good, one button opens the PR on GitHub with a generated title, body and changelog." Bullets: "Auto-generated PR titles & bodies", "Inline diff review before you push", "Draft PRs, stack PRs, amend PRs", "Works with your existing GitHub auth" | A |
| Git workflow illustration | Alt text: "Illustration of the T3 Code Git workflow: review a diff, commit and push changes, and open a pull request." Mock content: "feat/marketing-hero → main +142 −38 3 commits", "Commit & push ⌘⏎". This is an illustration, not a live record. | A |
| "If you don't like something, fork it." | Terminal mock: `gh repo fork pingdotgg/t3code --clone`, `vp install`, `vp run dev` → `http://localhost:4001`. Bullets: "Change the UI.", "Add an agent.", "Ship your own build." Badge: "MIT licensed 55k+ stars" | A |
| Hero screenshot UI | Sidebar "Search", project "postplan", thread "Add Expiring Draft Share Links", status "Worked for 20s", right panel "Open in editor", "Setup Worktree", branch "main" | A (screenshot) |

### 3.3 Supported agents, pricing, platforms, privacy, company

| Topic | Observed | Grade |
|---|---|---|
| Agents | Claude Code, Codex, Antigravity, OpenCode, Cursor, Grok (nav text); "Muse" and "Gemini" appear only in the hero image alt text | A |
| Pricing | App is "free, open source"; no plan table. Provider costs are the user's own. | A |
| Platforms — desktop | "Download for macOS" → `releases/download/v0.1.8/T3-Code-0.1.8-arm64.dmg`; "Other platforms" → `/download` (not captured) | A |
| Platforms — mobile | iOS `apps.apple.com/us/app/t3-code-remote-claude-more/id6787819824` (listing slug implies the name "T3 Code Remote: Claude & More"); Android package `com.t3tools.t3code` | A (URLs) / C (iOS title from slug) |
| Platforms — other | README (WebFetch summary, grade B): Windows via winget/Scoop/PowerShell, Linux via `.deb`/AUR/Nix, `npx t3@latest`; web app at app.t3.codes; Electron desktop; local server started by `t3` | B |
| Remote access | Testimonials only: "connects to a remote VM seamlessly with no Tailscale" (@amritwt), "T3 Code with connect in just a web browser" (@mweinbach), "able to do iOS development from just my phone" (@JoshuaRileyDev). README calls it "remote-ready" (grade B). | C / B |
| Privacy / security | Footer links "Privacy" (`/privacy-policy`), "Security" (`/security-policy`), "Terms" (`/terms-of-service`). None captured. | unknown |
| License | "MIT licensed" (badge and footer) | A |
| Company | Footer: "© 2026 T3 Tools Inc · MIT licensed"; GitHub org `pingdotgg`; Discord `discord.gg/jn4EGJjrvv` | A |
| Project maturity | README (grade B): "very very early", "Expect bugs", "(mostly) not accepting contributions yet"; ~55k stars, ~2k open issues | B |

### 3.4 Network observations (marketing site only)

From `docs/evidence/t3-network-origins.txt`:

| Origin | Requests |
|---|---|
| `https://t3.codes` | 23 (Astro assets, harness SVGs, profile pictures) |
| `https://api.github.com` | 1 (`/repos/pingdotgg/t3code/releases/latest`) |

No third-party analytics origin was observed in the 4 s window. The site is built with Astro (`/_astro/` paths observed, grade A for paths, C for the framework inference). The GitHub API call is consistent with the "55k+ stars" counter and latest-release link being fetched live (inference, grade C).

## 4. Design-relevant observations for hubcodes

| Observation | Source | Grade | Relevance |
|---|---|---|---|
| Agent-to-agent communication may be plain PTY injection: "you drag agents around, connect them with lines, and they talk to each other through PTY. No API glue, no middleware. Just one agent typing into another's terminal." | Maestri testimonial (@yrzhe_top) | C (user-authored technical claim, not vendor-confirmed) | Supports the hubcodes plan of `tmux load-buffer` + `paste-buffer` as the universal transport. |
| "tmux-style shortcuts" for switching terminals and workspaces | Maestri vendor copy | A | hubcodes runs inside tmux; keep prefix-style navigation familiar. |
| Reusable roles with playful names: "Unslopifier", "Bug Whisperer", "Vibe Coder", "Ship It", "Refactor Gremlin", "Docs Goblin", "Git Wizard", "Perf Nerd" | Maestri vendor copy | A | Roles = named instruction presets per node. Phase 2. |
| Bands with a "Maestro" on top; "The list shows who's working and who needs you" | Maestri vendor copy | A | Chat-style group view and status-first list. Phase 2 (chat room). |
| Floors: isolated workspace copies via APFS copy-on-write | Maestri vendor copy | A | hubcodes uses git worktrees per agent instead; APFS clone is macOS-only. |
| Notes written by agents; connected notes | Maestri vendor copy | A | `hubcodes note` CLI. Phase 2. |
| Prompt Composer with `@` references to connected terminals/notes | Maestri vendor copy | A | TUI message input with `@agent` autocomplete. Spike/Phase 2. |
| Branch per thread, one-button commit/push/PR, inline diff review | T3 vendor copy | A | Worktree per agent in spike; PR helper is a later phase. |
| "Switch models mid-thread." | T3 vendor copy | A | Provider adapters are per-node; model switching is delegated to the agent CLI. |
| Remote/mobile control of local agents | T3 vendor copy (apps) + testimonials | A / C | hubcodes Phase 3 sharing over Tailscale targets the same need with SSH/tsnet instead of a hosted relay. |
| Native terminal rendering at every zoom level ("Pixel-perfect TUI rendering at every zoom level") | Maestri vendor copy | A | Not reproducible in a cell-grid TUI; hubcodes shows `capture-pane` previews and delegates full rendering to tmux. |

## 5. Capability map

| Capability | Maestri | T3 Code | hubcodes plan |
|---|---|---|---|
| Multiple agents on one surface | Yes (canvas nodes) | Yes (threads) | Spike |
| Visual connections between agents (edges) | Yes ("Connect terminals together") | Not shown | Spike |
| Agent prompts another agent directly | Yes | Not shown | Spike (`hubcodes send`) |
| Message animation / activity on edges | Implied by canvas | No | Spike (message pulses) |
| Live terminal preview per node | Yes (native terminal) | Yes (full terminal) | Spike (tmux `capture-pane` preview; focus = real pane) |
| Providers: Claude Code | Yes | Yes | Spike |
| Providers: Codex | Yes | Yes | Spike |
| Providers: OpenCode | Yes | Yes | Spike |
| Providers: Cursor, Grok, Antigravity | No | Yes | Phase 2 if CLI available; adapter model allows any command |
| Providers: pi, omp | Not listed | Not listed | Spike (generic command adapter) |
| Plain shell node | Yes | Not stated | Spike |
| Pan / zoom canvas | Yes | No | Spike (pan + two zoom levels: full / compact) |
| Roles (reusable instructions) | Yes | Not shown | Phase 2 |
| Bands / grouped chat view | Yes | Threads list | Phase 2 (chat room) |
| Notes written by agents | Yes | No | Phase 2 |
| Status detection (working / waiting / idle) | Yes ("who's working and who needs you") | Yes ("Worked for 20s") | Spike (`capture-pane` heuristic); Phase 2 (provider hooks) |
| Isolated workspace copies | Floors (APFS) | Worktree per thread ("Setup Worktree") | Phase 1 (`hubcodes spawn --worktree`); spike supports `--cwd` only |
| One-button commit / push / PR | No | Yes | Phase 2+ (not in spike) |
| Remote control from another device | "Maestri Remote" (not captured) | Web + iOS + Android | Phase 3 (Tailscale SSH read-only → tailscaled LocalAPI `whois` identity → ACL → approve-input) |
| Multi-user shared session with identity | Not stated | Not stated | Phase 3 (federation between daemons) |
| Daemon / headless core | Not stated | Yes (local server) | Phase 2 |
| Sketching / diagrams on canvas | Yes | No | Out of scope |
| Portals (browser, iOS Simulator, Android emulator, device) | Yes | No | Out of scope |
| Computer Use (agents drive other apps) | Yes | No | Out of scope |
| On-device LLM assistant (Ombro) | Yes | No | Out of scope |
| Open source | No | Yes (MIT) | hubcodes: to be decided by the owner |
| Platforms | macOS (Apple Silicon); Windows/Linux pages exist, content unknown | macOS, Windows, Linux (grade B), iOS, Android, web | macOS and Linux via tmux; Windows only through WSL |
| Price | Free / Pro $18 one-time / Teams | Free | n/a |

## 6. Unknowns

The following were not captured and must not be stated as fact:

1. Content of Maestri's "Windows" (`/en/windows`), "Linux" (`/en/linux`), and "Maestri Remote" (`/en/remote`) pages. Only the links exist in evidence.
2. Content of Maestri's "Docs", "Learn", and "Changelog" pages.
3. Whether the Maestri macOS app itself sends any network traffic. The "Zero telemetry" claim is vendor copy; the PostHog observation applies to the website only.
4. Content of T3 Code's Privacy Policy, Security Policy, and Terms pages.
5. Content of T3 Code's `/download` page and the actual Windows/Linux install paths (README summary only, grade B).
6. The "Tolerated by over 500,000 devs" figure. No source is given on the page.
7. Whether "Muse" is a shipping harness. It appears only in an image alt text.
8. Any internal architecture of either product. Nothing beyond marketing copy and one README summary was examined; no binaries, bundles, or runtime behaviour were analysed.
9. The legal entity and location behind Maestri.

## 7. Method notes

- Capture tool: REA `capture_browser_scenario`, launch mode, Chromium 157.0.8103.50 headless, viewport 1440×900, dark colour scheme, locale `en-US`, timezone `Asia/Jakarta`, reduced motion, service workers blocked.
- Actions: one `wait_for_timeout` of 4000 ms. REA performed no clicks, scrolling, or JavaScript evaluation.
- Artifacts: URL, accessibility tree, screenshot at end; network and console event families enabled; no header values or bodies retained.
- The Maestri capture is marked `incomplete` by REA because the `events` section was missing from the completeness check and six media requests had failed or not finished. Network origin counts above are therefore a lower bound for that site.
- Earlier WebFetch summaries (grade B) were produced by a small model over fetched Markdown and are not verbatim; where they conflict with grade A text, grade A wins.
