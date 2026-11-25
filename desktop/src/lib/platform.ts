/**
 * The desktop's operating system, for the few words that differ: where
 * tokens are kept, what runs the daemon, how SSH is let in. Everything else
 * says "this device", which is right on macOS and Linux alike.
 */
const ua = typeof navigator === "undefined" ? "" : `${navigator.platform ?? ""} ${navigator.userAgent ?? ""}`;
export const isMac = /Mac/i.test(ua);
/** Where secrets go: the macOS Keychain, or the Linux login keyring. */
export const SECRET_STORE = isMac ? "Keychain" : "keyring";
/** What keeps hubcodesd running. */
export const SERVICE_MANAGER = isMac ? "launchd" : "systemd";

/** Is `seen` (`claude-opus-5-5`) the model that was `picked` (`opus` or a full id)? Unknown counts as a match. Same rule as the CLI's. */
export function modelMatches(picked: string | null | undefined, seen: string | null | undefined): boolean {
  const norm = (s: string) => s.trim().toLowerCase().split("[")[0];
  const p = norm(picked ?? "");
  const s = norm(seen ?? "");
  if (!p || !s || p === "default" || p === "inherit") return true;
  return s === p || s.split(/[-/.:]/).includes(p) || s.endsWith(p) || p.endsWith(s);
}

/** This build's version, from package.json. */
export const APP_VERSION: string = typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "dev";
