import { useEffect, useRef } from "react";
import { Input, Modal } from "antd";
import type { InputRef } from "antd";

type Opts = { placeholder?: string; initial?: string; hint?: string; multiline?: boolean; okText?: string };

/**
 * A small text prompt as an Ant Design modal. `window.prompt` is a no-op in
 * Tauri's WebKit view (returns null), so every "name this" goes through here.
 */
export function askText(title: string, opts: Opts = {}): Promise<string | null> {
  return new Promise((resolve) => {
    let value = opts.initial ?? "";
    let done = false;
    const finish = (v: string | null) => {
      if (done) return;
      done = true;
      resolve(v);
    };
    const m = Modal.confirm({
      title,
      icon: null,
      okText: opts.okText ?? "OK",
      cancelText: "Cancel",
      className: "ask-modal",
      autoFocusButton: null,
      content: <Prompt opts={opts} onChange={(v) => { value = v; }} onSubmit={() => { m.destroy(); finish(value.trim() || null); }} />,
      onOk: () => finish(value.trim() || null),
      onCancel: () => finish(null),
      afterClose: () => finish(null),
    });
  });
}

/** The field, focused once the modal's own focus trap has settled. */
function Prompt({ opts, onChange, onSubmit }: { opts: Opts; onChange: (v: string) => void; onSubmit: () => void }) {
  const ref = useRef<InputRef>(null);
  useEffect(() => {
    const t = window.setTimeout(() => ref.current?.focus({ cursor: "end" }), 60);
    return () => window.clearTimeout(t);
  }, []);
  // Keep the app's global shortcuts (Enter opens the selected card, Esc, ⌘…) out of the prompt.
  const stop = (e: React.KeyboardEvent) => e.stopPropagation();
  return (
    <div className="ask-body" onKeyDown={stop} onKeyUp={stop}>
      {opts.multiline ? (
        <Input.TextArea ref={ref} rows={8} defaultValue={opts.initial} placeholder={opts.placeholder} onChange={(e) => onChange(e.target.value)} spellCheck={false} />
      ) : (
        <Input ref={ref} defaultValue={opts.initial} placeholder={opts.placeholder} onChange={(e) => onChange(e.target.value)} onPressEnter={onSubmit} spellCheck={false} />
      )}
      {opts.hint && <div className="ask-hint">{opts.hint}</div>}
    </div>
  );
}
