import { useEffect, useRef } from "react";
import { paneHost } from "./host";

/** Mounts the shared xterm for a pane into this component's box. */
export function PaneTerminal({ id, autoFocus = false }: { id: string; autoFocus?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const host = paneHost(id);
    host.mount(el);
    if (autoFocus) window.setTimeout(() => host.focus(), 60);
    const ro = new ResizeObserver(() => host.refit());
    ro.observe(el);
    return () => {
      ro.disconnect();
      host.unmount(el);
    };
  }, [id, autoFocus]);
  return <div ref={ref} className="pane-box nodrag nowheel" />;
}
