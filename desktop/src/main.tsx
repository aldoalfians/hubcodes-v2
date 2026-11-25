import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import App from "./App";
import { ConfigProvider, theme as antTheme, type ThemeConfig } from "antd";
import { useStore } from "./store";
import { alpha, applyTheme, savedTheme, useTheme, type Theme } from "./lib/theme";
import { setTerminalTheme } from "./terminal/host";

// Dev only: lets UI tests read state (`window.hubcodesStore.getState()`).
if (import.meta.env.DEV) (window as unknown as { hubcodesStore: typeof useStore }).hubcodesStore = useStore;

// A unicode-range face only loads for text inside its range, so pass a sample.
const fonts: [string, string][] = [
  ['12px "JetBrains Mono"', "abc"],
  ['500 12px "JetBrains Mono"', "abc"],
  ['12px "Nerd Symbols Mono"', "\ue0b0\uf179\uf126"],
  ['13px "IBM Plex Sans"', "abc"],
];
const ready = Promise.all(fonts.map(([f, t]) => document.fonts.load(f, t).catch(() => null)));
const timeout = new Promise((r) => setTimeout(r, 1500));
/** Ant Design tokens for a palette: every antd control follows the theme. */
const antFor = (t: Theme): ThemeConfig => {
  const p = t.p;
  return {
    algorithm: antTheme.darkAlgorithm,
    token: {
      colorPrimary: p.primary,
      colorInfo: p.info,
      colorSuccess: p.green,
      colorWarning: p.yellow,
      colorError: p.red,
      colorBgBase: p.bg,
      colorBgContainer: p.mantle,
      colorBgElevated: p.surface,
      colorBgLayout: p.bg,
      colorBorder: p.line,
      colorBorderSecondary: p.surface2,
      colorText: p.text,
      colorTextSecondary: p.muted,
      colorTextTertiary: p.muted,
      colorLink: p.primary,
      borderRadius: 8,
      fontSize: 13,
      fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif",
      fontFamilyCode: "ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace",
      controlHeight: 38,
      boxShadowSecondary: "0 8px 24px rgba(0, 0, 0, 0.45)",
    },
    components: {
      Button: { defaultBg: p.surface2, defaultBorderColor: p.line, defaultHoverBg: p.line, defaultHoverBorderColor: p.muted, primaryShadow: "none", primaryColor: p.onPrimary, fontWeight: 500 },
      // Every field looks the same: 38 px, no fill, a thin line, the accent on focus.
      Input: { paddingBlock: 8, paddingInline: 12, colorBgContainer: "transparent", activeBg: "transparent", hoverBg: "transparent", colorBorder: alpha(p.text, 0.14), hoverBorderColor: alpha(p.text, 0.28), activeBorderColor: p.primary, activeShadow: `0 0 0 3px ${alpha(p.primary, 0.14)}` },
      InputNumber: { paddingBlock: 8, paddingInline: 12, colorBgContainer: "transparent", activeBg: "transparent", hoverBg: "transparent", colorBorder: alpha(p.text, 0.14), hoverBorderColor: alpha(p.text, 0.28), activeBorderColor: p.primary, activeShadow: `0 0 0 3px ${alpha(p.primary, 0.14)}` },
      Select: { selectorBg: "transparent", colorBgContainer: "transparent", colorBorder: alpha(p.text, 0.14), hoverBorderColor: alpha(p.text, 0.28), activeBorderColor: p.primary, activeOutlineColor: alpha(p.primary, 0.14), optionSelectedBg: alpha(p.text, 0.1), optionHeight: 34, optionPadding: "7px 12px" },
      Menu: { itemBg: "transparent", itemSelectedBg: alpha(p.primary, 0.15), itemSelectedColor: p.primary, itemHoverBg: alpha(p.text, 0.06), groupTitleColor: p.muted, itemHeight: 32, iconSize: 14 },
      Table: { headerBg: p.surface, headerColor: p.muted, rowHoverBg: alpha(p.text, 0.05), cellPaddingBlock: 8, cellPaddingInline: 12 },
      Card: { headerBg: p.surface },
      Segmented: { itemSelectedBg: p.surface2, trackBg: p.mantle },
      Switch: { handleBg: "#fff" },
    },
  };
};

/** Terminal colours from the palette, so panes match the theme. */
const terminalFor = (p: Theme["p"]) => ({
  background: p.surface, cursorAccent: p.surface, foreground: p.text, cursor: p.primary, selectionBackground: alpha(p.primary, 0.28),
  black: p.surface2, red: p.red, green: p.green, yellow: p.yellow, blue: p.primary, magenta: p.violet, cyan: p.cyan, white: p.text,
  brightBlack: p.muted, brightRed: p.red, brightGreen: p.green, brightYellow: p.yellow, brightBlue: p.info, brightMagenta: p.violet, brightCyan: p.cyan, brightWhite: p.text,
});

function Themed({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  const cfg = React.useMemo(() => antFor(t), [t]);
  React.useEffect(() => {
    // Static modals (Modal.confirm, askText) render outside the tree; give them the same theme.
    ConfigProvider.config({ theme: cfg });
    setTerminalTheme(terminalFor(t.p));
  }, [cfg, t]);
  return <ConfigProvider theme={cfg}>{children}</ConfigProvider>;
}

applyTheme(savedTheme().id);
setTerminalTheme(terminalFor(savedTheme().p));

Promise.race([ready, timeout]).then(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <Themed>
        <App />
      </Themed>
    </React.StrictMode>,
  );
});
