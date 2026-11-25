// Window themes. Graphite is hubcodes' own; the rest follow the official
// palettes of Neovim colorschemes, so the app can match the editor beside it.
// A theme sets CSS variables (backdrop, panels, accent, status colours), the
// Ant Design tokens and the terminal colours. The choice is per Mac.
import { useSyncExternalStore } from "react";

export type Palette = {
  bg: string; mantle: string; surface: string; surface2: string; line: string;
  text: string; muted: string;
  primary: string; onPrimary: string; info: string;
  green: string; yellow: string; red: string; orange: string; violet: string; cyan: string;
  glowA: string; glowB: string; glowC: string;
};
export type Theme = { id: string; name: string; source: string; p: Palette };

const P = (p: Palette) => p;
export const THEMES: Theme[] = [
  { id: "graphite", name: "Graphite", source: "hubcodes", p: P({ bg: "#18191c", mantle: "#131416", surface: "#26272b", surface2: "#2f3035", line: "#3a3b40", text: "#f4f5f7", muted: "#8e9099", primary: "#2de1c2", onPrimary: "#0f1a18", info: "#9b87ff", green: "#2de1c2", yellow: "#f5b83d", red: "#ff5c5c", orange: "#f5a524", violet: "#8b6cff", cyan: "#22c7e8", glowA: "#2de1c2", glowB: "#22c7e8", glowC: "#8b6cff" }) },
  { id: "tokyonight", name: "Tokyo Night", source: "folke/tokyonight", p: P({ bg: "#1a1b26", mantle: "#16161e", surface: "#1f2335", surface2: "#292e42", line: "#3b4261", text: "#c0caf5", muted: "#737aa2", primary: "#7aa2f7", onPrimary: "#16161e", info: "#7dcfff", green: "#9ece6a", yellow: "#e0af68", red: "#f7768e", orange: "#ff9e64", violet: "#bb9af7", cyan: "#7dcfff", glowA: "#bb9af7", glowB: "#7dcfff", glowC: "#7aa2f7" }) },
  { id: "kanagawa", name: "Kanagawa Wave", source: "rebelot/kanagawa", p: P({ bg: "#1f1f28", mantle: "#16161d", surface: "#2a2a37", surface2: "#363646", line: "#54546d", text: "#dcd7ba", muted: "#727169", primary: "#7e9cd8", onPrimary: "#16161d", info: "#7fb4ca", green: "#98bb6c", yellow: "#e6c384", red: "#e46876", orange: "#ffa066", violet: "#957fb8", cyan: "#7aa89f", glowA: "#957fb8", glowB: "#7aa89f", glowC: "#7e9cd8" }) },
  { id: "kanagawa-dragon", name: "Kanagawa Dragon", source: "rebelot/kanagawa", p: P({ bg: "#181616", mantle: "#12120f", surface: "#282727", surface2: "#393836", line: "#625e5a", text: "#c5c9c5", muted: "#737c73", primary: "#8ba4b0", onPrimary: "#12120f", info: "#8ea4a2", green: "#87a987", yellow: "#c4b28a", red: "#c4746e", orange: "#b6927b", violet: "#8992a7", cyan: "#8ea4a2", glowA: "#c4746e", glowB: "#8ea4a2", glowC: "#8ba4b0" }) },
  { id: "rose-pine", name: "Rosé Pine", source: "rose-pine/neovim", p: P({ bg: "#191724", mantle: "#13111e", surface: "#1f1d2e", surface2: "#26233a", line: "#403d52", text: "#e0def4", muted: "#908caa", primary: "#c4a7e7", onPrimary: "#191724", info: "#9ccfd8", green: "#9ccfd8", yellow: "#f6c177", red: "#eb6f92", orange: "#ebbcba", violet: "#c4a7e7", cyan: "#9ccfd8", glowA: "#eb6f92", glowB: "#31748f", glowC: "#c4a7e7" }) },
  { id: "catppuccin", name: "Catppuccin Mocha", source: "catppuccin/nvim", p: P({ bg: "#1e1e2e", mantle: "#181825", surface: "#313244", surface2: "#45475a", line: "#585b70", text: "#cdd6f4", muted: "#a6adc8", primary: "#cba6f7", onPrimary: "#1e1e2e", info: "#89b4fa", green: "#a6e3a1", yellow: "#f9e2af", red: "#f38ba8", orange: "#fab387", violet: "#b4befe", cyan: "#94e2d5", glowA: "#fab387", glowB: "#89b4fa", glowC: "#cba6f7" }) },
  { id: "everforest", name: "Everforest", source: "sainnhe/everforest", p: P({ bg: "#2d353b", mantle: "#232a2e", surface: "#343f44", surface2: "#3d484d", line: "#475258", text: "#d3c6aa", muted: "#859289", primary: "#a7c080", onPrimary: "#232a2e", info: "#7fbbb3", green: "#83c092", yellow: "#dbbc7f", red: "#e67e80", orange: "#e69875", violet: "#d699b6", cyan: "#83c092", glowA: "#e69875", glowB: "#7fbbb3", glowC: "#a7c080" }) },
  { id: "oxocarbon", name: "Oxocarbon", source: "nyoom-engineering/oxocarbon", p: P({ bg: "#161616", mantle: "#0f0f0f", surface: "#262626", surface2: "#393939", line: "#525252", text: "#f2f4f8", muted: "#8d8d8d", primary: "#78a9ff", onPrimary: "#0f0f0f", info: "#33b1ff", green: "#42be65", yellow: "#f1c21b", red: "#ee5396", orange: "#ff832b", violet: "#be95ff", cyan: "#3ddbd9", glowA: "#ff7eb6", glowB: "#3ddbd9", glowC: "#78a9ff" }) },
  { id: "eldritch", name: "Eldritch", source: "eldritch-theme/eldritch.nvim", p: P({ bg: "#212337", mantle: "#171928", surface: "#292e42", surface2: "#323449", line: "#3b4261", text: "#ebfafa", muted: "#7081d0", primary: "#37f499", onPrimary: "#171928", info: "#04d1f9", green: "#37f499", yellow: "#f1fc79", red: "#f16c75", orange: "#f7c67f", violet: "#a48cf2", cyan: "#04d1f9", glowA: "#a48cf2", glowB: "#04d1f9", glowC: "#37f499" }) },
  { id: "nordic", name: "Nordic", source: "AlexvZyl/nordic.nvim", p: P({ bg: "#242933", mantle: "#191d24", surface: "#2e3440", surface2: "#3b4252", line: "#434c5e", text: "#d8dee9", muted: "#7b88a1", primary: "#88c0d0", onPrimary: "#191d24", info: "#81a1c1", green: "#a3be8c", yellow: "#ebcb8b", red: "#bf616a", orange: "#d08770", violet: "#b48ead", cyan: "#8fbcbb", glowA: "#b48ead", glowB: "#5e81ac", glowC: "#88c0d0" }) },
];

const KEY = "hubcodes.theme";
let current: Theme = THEMES[0];
const listeners = new Set<() => void>();

/** `#rrggbb` → `rgba(r, g, b, a)`. */
export const alpha = (hex: string, a: number) => {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
};

export function savedTheme(): Theme {
  try {
    const id = localStorage.getItem(KEY);
    const t = THEMES.find((x) => x.id === id);
    if (t) return t;
  } catch { /* private mode: default */ }
  return THEMES[0];
}

/** Write the palette into CSS variables; the stylesheet only reads them. */
function paint(p: Palette) {
  const v: Record<string, string> = {
    "--bg": p.bg, "--surface": p.surface, "--surface-2": p.surface2, "--card": p.surface, "--card-2": p.surface2,
    "--chrome": p.mantle, "--field": p.mantle, "--field-hover": p.surface2, "--active-bar": p.primary, "--line": p.line, "--text": p.text, "--muted": p.muted,
    "--mint": p.primary, "--mint-soft": alpha(p.primary, 0.15), "--accent": p.primary, "--signal": p.primary, "--signal-soft": alpha(p.primary, 0.15),
    "--on-accent": p.onPrimary, "--ring": `0 0 0 2px ${alpha(p.primary, 0.35)}`,
    "--working": p.green, "--success": p.green, "--success-emphasis": p.green,
    "--attention": p.yellow, "--attention-soft": alpha(p.yellow, 0.15), "--orange": p.yellow,
    "--error": p.red, "--danger": p.red, "--red": p.red,
    "--cyan": p.cyan, "--violet": p.violet,
    "--dot-waiting": p.yellow, "--dot-working": p.green, "--dot-error": p.red, "--dot-idle": p.muted, "--dot-offline": alpha(p.muted, 0.5),
    "--bd-1": p.bg, "--bd-2": p.mantle, "--bd-3": p.mantle,
    "--glow-a": alpha(p.glowA, 0.18), "--glow-b": alpha(p.glowB, 0.14), "--glow-c": alpha(p.glowC, 0.08),
    "--glass": alpha(p.surface, 0.8), "--glass-2": alpha(p.surface2, 0.86), "--glass-hi": alpha(p.text, 0.07),
    "--stage-bg": alpha(p.bg, 0.86), "--node-bg": alpha(p.surface, 0.96), "--node-head": alpha(p.surface2, 0.94), "--dialog-bg": alpha(p.bg, 0.97),
  };
  const s = document.documentElement.style;
  for (const [k, val] of Object.entries(v)) s.setProperty(k, val);
}

export function applyTheme(id: string) {
  current = THEMES.find((t) => t.id === id) ?? THEMES[0];
  document.documentElement.dataset.theme = current.id;
  paint(current.p);
  try { localStorage.setItem(KEY, current.id); } catch { /* not remembered */ }
  listeners.forEach((l) => l());
}

export function useTheme(): Theme {
  return useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, () => current);
}
