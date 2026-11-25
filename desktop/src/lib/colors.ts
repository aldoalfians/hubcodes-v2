// xterm 256-colour index → hex, so provider colours from hubcodes.toml render
// the same in the TUI and here.
const base16 = [
  "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
  "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff",
];
export function color256(i: number | null | undefined): string {
  if (i == null) return "#8A918D";
  if (i < 16) return base16[i];
  if (i < 232) {
    const n = i - 16;
    const steps = [0, 95, 135, 175, 215, 255];
    const r = steps[Math.floor(n / 36)], g = steps[Math.floor((n % 36) / 6)], b = steps[n % 6];
    return `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
  }
  const v = 8 + (i - 232) * 10;
  return `#${[v, v, v].map((x) => x.toString(16).padStart(2, "0")).join("")}`;
}
