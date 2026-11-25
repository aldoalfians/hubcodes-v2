// One xterm.js instance per tmux pane, kept alive across React remounts so
// the same terminal moves from the node card into the open sheet and back.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { backend, log } from "../lib/backend";

export const termTheme = {
  background: "#232428",
  foreground: "#EDEEF0",
  cursor: "#2DE1C2",
  cursorAccent: "#232428",
  selectionBackground: "rgba(45,225,194,0.25)",
  black: "#2C2D31",
  red: "#FF5C5C",
  green: "#2DE1C2",
  yellow: "#F2B33D",
  blue: "#5AA9FF",
  magenta: "#C58AD4",
  cyan: "#6CC4C4",
  white: "#D6D8DE",
  brightBlack: "#6B6E78",
  brightRed: "#F58B82",
  brightGreen: "#8EDBA6",
  brightYellow: "#F7C96B",
  brightBlue: "#A3C1E6",
  brightMagenta: "#D7AAE2",
  brightCyan: "#92D6D6",
  brightWhite: "#F7F8FA",
};

export class PaneHost {
  readonly term: Terminal;
  readonly fit = new FitAddon();
  readonly el: HTMLDivElement;
  private unlisten: (() => void) | null = null;
  private attached = false;
  private fitTimer: number | undefined;
  /** Boxes this terminal has been mounted in, most recent last: the sheet
   *  on top of the node card. Unmounting the top one hands the element
   *  back to the one below, so the card is not left empty after "Back". */
  private boxes: HTMLElement[] = [];

  constructor(readonly id: string) {
    this.term = new Terminal({
      // Both bundled: JetBrains Mono for text, the Nerd symbols subset for the
      // private-use glyphs agent prompts use. Same look on every machine.
      fontFamily: '"JetBrains Mono", "Nerd Symbols Mono", Menlo, monospace',
      fontSize: 12,
      lineHeight: 1.2,
      theme: termTheme,
      cursorBlink: true,
      cursorStyle: "bar",
      scrollback: 3000,
      allowProposedApi: true,
      macOptionIsMeta: true,
    });
    this.term.loadAddon(this.fit);
    // Match zsh/tmux wcwidth for prompt glyphs; xterm's default table is Unicode 6.
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = "11";
    this.el = document.createElement("div");
    this.el.className = "pane-host";
    this.term.open(this.el);
    this.term.onData((d) => backend.write(this.id, d).catch(() => {}));
    this.term.onResize(({ cols, rows }) => backend.resize(this.id, cols, rows).catch(() => {}));
    void this.loadWebgl();
  }

  private async loadWebgl() {
    try {
      const { WebglAddon } = await import("@xterm/addon-webgl");
      const addon = new WebglAddon();
      addon.onContextLoss(() => addon.dispose());
      this.term.loadAddon(addon);
      // Glyphs rasterised before the symbol font arrived are tofu in the
      // atlas; drop the atlas once the font is in.
      document.fonts.load('12px "Nerd Symbols Mono"', "\ue0b0").then(() => addon.clearTextureAtlas()).catch(() => {});
    } catch {
      /* canvas renderer is fine */
    }
  }

  mount(container: HTMLElement) {
    log(`host.mount ${this.id} attached=${this.attached} size=${container.clientWidth}x${container.clientHeight}`);
    this.boxes = this.boxes.filter((b) => b !== container);
    this.boxes.push(container);
    if (this.el.parentElement !== container) container.appendChild(this.el);
    this.refit();
    void this.attach();
  }

  /** The box went away (sheet closed, card left the canvas): move the
   *  terminal back into the most recent box that is still in the page. */
  unmount(container: HTMLElement) {
    this.boxes = this.boxes.filter((b) => b !== container && b.isConnected);
    if (this.el.parentElement !== container) return;
    const back = this.boxes[this.boxes.length - 1];
    if (!back) return;
    back.appendChild(this.el);
    this.refit();
    // A redraw after the move; the resize alone is a no-op when both boxes
    // happen to hold the same number of columns and rows.
    this.term.refresh(0, this.term.rows - 1);
  }

  refit() {
    window.clearTimeout(this.fitTimer);
    this.fitTimer = window.setTimeout(() => {
      try {
        this.fit.fit();
      } catch {
        /* not visible yet */
      }
    }, 30);
  }

  private async attach() {
    if (this.attached) return;
    this.attached = true;
    log(`host.attach ${this.id} listening`);
    this.unlisten = await backend.onPane(this.id, (bytes) => this.term.write(bytes));
    log(`host.attach ${this.id} listener ready cols=${this.term.cols} rows=${this.term.rows}`);
    try {
      this.fit.fit();
    } catch {
      /* ignore */
    }
    try {
      await backend.attach(this.id, this.term.cols, this.term.rows);
      log(`host.attach ${this.id} ok`);
    } catch (e) {
      this.attached = false;
      this.term.write(`\r\n\x1b[31mCould not attach to this terminal: ${String(e)}\x1b[0m\r\n`);
      console.error("attach failed", this.id, e);
    }
  }

  focus() {
    this.term.focus();
  }

  dispose() {
    this.unlisten?.();
    backend.detach(this.id).catch(() => {});
    this.term.dispose();
    this.el.remove();
  }
}

const hosts = new Map<string, PaneHost>();
export function paneHost(id: string): PaneHost {
  let h = hosts.get(id);
  if (!h) {
    h = new PaneHost(id);
    hosts.set(id, h);
  }
  return h;
}
export function disposePane(id: string) {
  hosts.get(id)?.dispose();
  hosts.delete(id);
}
export function pruneHosts(liveIds: Set<string>) {
  for (const id of [...hosts.keys()]) if (!liveIds.has(id)) disposePane(id);
}

/** Recolour every open terminal (theme switch); new ones read termTheme. */
export function setTerminalTheme(patch: Partial<typeof termTheme>) {
  Object.assign(termTheme, patch);
  for (const h of hosts.values()) h.term.options.theme = { ...termTheme };
}
