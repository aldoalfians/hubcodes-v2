# Desktop design plan (Tauri + React)

Brief: a desktop canvas for orchestrating 2–8 AI coding agents (Claude Code, Codex, OpenCode, …) that run as tmux panes. Audience: one developer at a time, in a dark room, eight hours a day. Primary job of the screen: show in one glance **who needs you**, let you **type into any agent without ceremony**, and show **who is talking to whom**. The terminals are the content; the chrome must recede.

References: Maestri (native canvas, roles, bands, "the list shows who's working and who needs you") and T3 Code (threads, status like "Worked for 20s", one-button PR). See [reference-teardown.md](reference-teardown.md) and [reference-ui-teardown.md](reference-ui-teardown.md).

## Tokens

| Token | Value | Role |
|---|---|---|
| `--bg` | `#121517` | canvas and window background (graphite, not black, so terminal black still reads) |
| `--surface` | `#181C20` | node bodies, panels |
| `--surface-2` | `#1F2429` | hover, inputs |
| `--line` | `#2A3137` | borders, grid dots (at 40 % alpha) |
| `--text` | `#E8E4DC` | primary text, warm paper white |
| `--muted` | `#8A918D` | secondary text |
| `--signal` | `#F2B33D` | **the one accent**: needs-you, focus ring, selection, message pulses |
| `--working` | `#6CCB8A` | status only |
| `--error` | `#F06A5E` | status only |

Provider colours come from the existing 256-colour indexes in `hubcodes.toml`, converted to RGB at runtime, and are used only for the provider glyph and the node's top rule. One accent everywhere else.

Type: **IBM Plex Sans** for UI (bundled), **JetBrains Mono** for terminals, agent names and anything that is literally a command. Scale: 11 / 12 / 13 / 15 / 20 px. No all-caps labels, no eyebrows.

## Layout

```
┌ ● ● ●  hubcodes · session hubcodes            ◐ 1 needs you   ⌘K   ⚙ ┐
│ rail │                                                                 │
│ ◐ a  │       ┌ ◐ planner · ✦claude ─────────┐                         │
│ ● b  │       │ live terminal (xterm)        │══ task ══▶ ┌ ● builder ┐ │
│ ○ c  │       │                              │            │           │ │
│      │       │ › type to instruct…          │            └───────────┘ │
│ +    │       └──────────────────── idle ────┘                         │
└──────┴─────────────────────────────────────────────── activity ▸ ─────┘
```

- Left rail (56 px, expandable): one row per agent, status glyph first. Click selects and pans to it; `+` at the bottom. The rail is the "who needs you" list; the canvas is the map.
- Canvas: infinite, dotted grid, pan with space/drag or trackpad, zoom with pinch or ⌘±. Nodes are cards with a live xterm, a composer row and a status footer. Edges are orthogonal with a label at the midpoint; a message sends a dot along the edge.
- Open: `Enter` or double-click grows the selected node into a sheet that fills the canvas (same terminal instance, re-fitted). `Esc` returns to the map. No mode switch, no tmux knowledge.
- Activity drawer on the right, toggled; newest first.
- New agent is a small dialog: name, provider (only installed ones are enabled), folder. Settings is a dialog with Providers / Session / Shortcuts.

Alignment: everything left-aligned; numbers right-aligned in tables.

## Principles

1. Terminals are the hero; UI chrome is 1 px lines and 13 px text. Nothing glows.
2. One accent (`--signal`) and it always means "attention here": needs-you, selection, a message in flight.
3. Motion only answers an action or a message: node open/close, a pulse along an edge, a toast. No entrance animations, no hover lifts.
4. Words name what the user does: "Open", "Back", "Send", "New agent", "Needs you". Errors say what happened and what to do.
5. Keyboard first: `⌘N` new agent, `Enter` open, `Esc` back, `⌘K` command palette, `⌘1…9` select agent, `⌘,` settings. Every shortcut is listed in the palette.

## Review against the generic default

A default pass for "dark developer dashboard" would give: near-black background, electric blue or green accent, Inter, identical rounded cards with soft shadows, ALL-CAPS section labels. Changes made here: graphite with warm paper-white text instead of near-black with cool grey; amber as the single meaning-bearing accent instead of a decorative blue; IBM Plex Sans / JetBrains Mono instead of Inter; node cards are the *only* cards and their shape comes from the terminal they hold (no shadow, 1 px line, 6 px radius); status is a glyph and a word, never a coloured pill.
