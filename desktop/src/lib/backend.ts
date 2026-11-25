import type { Backend } from "./types";
import { tauriBackend } from "./tauri";
import { mockBackend } from "./mock";
import { httpBackend, isRemoteUi } from "./http";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
/** Served by `hubcodes serve` at /canvas/: the JSON API stands in for Tauri. */
export { isRemoteUi };
export const backend: Backend = isTauri ? tauriBackend : isRemoteUi ? httpBackend : mockBackend;

/** Mirror important frontend logs to the Rust stderr (dev diagnostics). */
export function log(msg: string) {
  console.log(msg);
  if (isTauri) {
    import("@tauri-apps/api/core").then(({ invoke }) => invoke("log_js", { msg })).catch(() => {});
  }
}
if (typeof window !== "undefined") {
  window.addEventListener("error", (e) => log(`window.error ${e.message} @${e.filename}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => log(`unhandledrejection ${String((e as PromiseRejectionEvent).reason)}`));
}
