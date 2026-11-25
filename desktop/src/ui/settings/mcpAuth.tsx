import { Button, Input, Segmented, Typography } from "antd";
import { LoginOutlined } from "@ant-design/icons";
import { backend } from "../../lib/backend";
import { useStore } from "../../store";
import { Field, Row } from "./primitives";
import { SecretInput } from "./connect";
import { code } from "./shared";
import { SECRET_STORE } from "../../lib/platform";
import type { McpTarget } from "../../lib/types";

const { Text } = Typography;

/**
 * Signing in to a remote MCP server, the same way everywhere it is used
 * (knowledge sources, team memory, trackers): a token sent as a Bearer
 * header, or a browser sign-in (OAuth, e.g. Google) through the pinned
 * `mcp-remote` bridge, which keeps the login in ~/.mcp-auth. Mirrors
 * `hubcodes_core::mcp::oauth_bridge`.
 */
export const BRIDGE = "mcp-remote@0.16.7";
export const bridgeArgs = (url: string) => ["-y", BRIDGE, url.trim(), "31419", "--host", "localhost", "--callback-path", "/callback", "--auth-timeout", "180"];

/** The server a bridge command signs in to, or null when the target is not one. */
export function bridgedUrl(t: Partial<McpTarget>): string | null {
  if (!(t.command ?? "").trim().endsWith("npx")) return null;
  const args = t.args ?? [];
  const i = args.findIndex((a) => a.startsWith("mcp-remote"));
  const u = i >= 0 ? args[i + 1] : undefined;
  return u && u.startsWith("http") ? u : null;
}

/**
 * Open a terminal sign-in (gh, twg) as a pane on the canvas. Settings stays
 * open with whatever is being edited; the pane is there once it is closed.
 */
export function useSignIn() {
  const notify = useStore((s) => s.notify);
  return async (kind: "github" | "twg" | `provider:${string}`) => {
    try {
      const pane = await backend.loginTerminal(kind);
      notify(`${pane} is open on the canvas: finish the sign-in there (close Settings to see it), then Check again here.`);
    } catch (e) {
      notify(String(e).replace(/^Error: /, ""), "error");
    }
  };
}

/** The address a target talks to, whichever way it signs in. */
export const serverOf = (t: Partial<McpTarget>) => bridgedUrl(t) ?? t.url ?? "";

/**
 * Server URL plus how to sign in. `as="field"` for dialogs, `as="row"` for
 * settings pages. A target that runs some other command (stdio) is left to
 * the caller's own fields.
 */
export function McpServerFields({ target, onChange, placeholder, as = "field", signIn }: {
  target: Partial<McpTarget>;
  onChange: (p: Partial<McpTarget>) => void;
  placeholder: string;
  as?: "field" | "row";
  /** Reach the server and read its tools: in browser mode this is the sign-in (the browser opens when needed). */
  signIn?: { run: () => Promise<unknown>; busy: boolean; done?: string | null };
}) {
  const viaBrowser = bridgedUrl(target);
  const url = viaBrowser ?? target.url ?? "";
  const token = (target.headers?.Authorization ?? "").replace(/^Bearer /, "");
  const setUrl = (u: string) => onChange(viaBrowser !== null ? { args: bridgeArgs(u) } : { url: u });
  const setMode = (m: string) => (m === "browser" ? onChange({ command: "npx", args: bridgeArgs(url), url: "" }) : onChange({ command: "", args: [], url }));
  const setToken = (v: string) => {
    const h = { ...(target.headers ?? {}) };
    if (v) h.Authorization = v.startsWith("${") ? v : `Bearer ${v}`;
    else delete h.Authorization;
    onChange({ headers: h });
  };
  const hint = viaBrowser !== null
    ? <>The first use opens your browser to sign in (Google and the like); the login stays on this device in {code("~/.mcp-auth")}. Needs Node.js ({code("npx")}).</>
    : <>Optional. Sent as Authorization: Bearer and kept in the {SECRET_STORE}. A server that answers 401 and signs in with a browser: pick Browser.</>;
  const urlInput = <Input className="mono" value={url} onChange={(e) => setUrl(e.target.value)} placeholder={placeholder} spellCheck={false} aria-label="Server URL" />;
  const signInBlock = (
    <div className="kb-signin">
      <Segmented size="small" value={viaBrowser !== null ? "browser" : "token"} onChange={(v) => setMode(String(v))} options={[{ value: "token", label: "Token or none" }, { value: "browser", label: "Browser (OAuth)" }]} aria-label="Sign in" />
      {viaBrowser === null ? <SecretInput value={token} onChange={setToken} label="Token" /> : (
        <span className="kb-signin-now">
          {signIn && <Button size="small" icon={<LoginOutlined />} loading={signIn.busy} disabled={!/^https?:\/\//.test(url)} onClick={() => void signIn.run()}>{signIn.busy ? "Waiting for the browser…" : signIn.done ? "Sign in again" : "Sign in now"}</Button>}
          <Text type={signIn?.done ? "success" : "secondary"} className="kb-signin-note">
            {signIn?.busy ? "Finish the sign-in in the browser window (up to 3 minutes)." : signIn?.done ? signIn.done : "A browser window opens if this device is not signed in yet."}
          </Text>
        </span>
      )}
    </div>
  );
  if (as === "row") {
    return (
      <>
        <Row label="Server URL" stack>{urlInput}</Row>
        <Row label="Sign in" hint={hint} stack>{signInBlock}</Row>
      </>
    );
  }
  return (
    <>
      <Field span label="Server URL">{urlInput}</Field>
      <Field span label="Sign in" hint={hint}>{signInBlock}</Field>
    </>
  );
}
