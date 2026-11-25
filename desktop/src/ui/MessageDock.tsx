import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent } from "react";
import { Select, Tooltip } from "antd";
import { CloseOutlined, DownOutlined, FileOutlined, LoadingOutlined, MessageOutlined, PaperClipOutlined, SendOutlined } from "@ant-design/icons";
import { backend } from "../lib/backend";
import { routeLead } from "../lib/route";
import { statusGlyph, useStore } from "../store";
import type { Agent } from "../lib/types";

/** One file on its way to the agents: saved on this Mac first, then sent by path. */
interface Attachment {
  key: string;
  name: string;
  /** Path on this Mac once saved (dropped files have one at once). */
  path: string | null;
  /** Thumbnail for pasted or picked images. */
  preview: string | null;
  error: string | null;
}

const baseName = (p: string) => p.split("/").pop() || p;
let seq = 0;
const nextKey = () => `a${++seq}`;

/** Base64 of a File, without the `data:…;base64,` head. */
const toBase64 = (f: File) =>
  new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result).replace(/^data:[^,]*,/, ""));
    r.onerror = () => reject(r.error ?? new Error("could not read the file"));
    r.readAsDataURL(f);
  });

/** Open or hidden, remembered per viewer (storage may be off: then it starts open). */
const OPEN_KEY = "hubcodes.dockOpen";
const wasOpen = () => { try { return localStorage.getItem(OPEN_KEY) !== "0"; } catch { return true; } };
const rememberOpen = (on: boolean) => { try { localStorage.setItem(OPEN_KEY, on ? "1" : "0"); } catch { /* not kept */ } };

/** "dev" or "mini · tester": the device part is quieter. */
function AgentLabel({ a }: { a: Agent }) {
  const [device, name] = a.peer && a.name.includes("/") ? a.name.split("/", 2) : [null, a.name];
  return (
    <span className="dock-agent">
      <span className={`glyph st-${a.status}`}>{statusGlyph[a.status]}</span>
      {device && <span className="dock-device">{device}</span>}
      <span>{name}</span>
    </span>
  );
}

/**
 * Talk to any agent as yourself, without a loop: pick who it goes to, type,
 * paste a screenshot or drop files. Files are written on this Mac and, for
 * an agent on another device, copied there; the agent gets the paths after
 * the text and opens them with its own tools. The card terminals stay for
 * the CLI's own keys; this is the one place for a person's message.
 */
export function MessageDock() {
  const agents = useStore((s) => s.agents);
  const edges = useStore((s) => s.edges);
  const selectedId = useStore((s) => s.selectedId);
  const routeOn = useStore((s) => s.routeThroughOrchestrator);
  const instruct = useStore((s) => s.instruct);
  const notify = useStore((s) => s.notify);
  const [text, setText] = useState("");
  const [files, setFiles] = useState<Attachment[]>([]);
  // Who it goes to: follows the selected card until you pick by hand.
  const [picked, setPicked] = useState<string[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const [open, setOpenState] = useState(wasOpen);
  const setOpen = (on: boolean) => { setOpenState(on); rememberOpen(on); };
  // Toasts and the update bar also sit at the bottom centre: tell them how much room the dock takes.
  const root = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const css = document.documentElement.style;
    // Room above the dock, and its centre: the canvas is beside the rail, so
    // the window's centre is not the dock's, and notices would sit askew.
    const set = () => {
      const r = el.getBoundingClientRect();
      css.setProperty("--dock-space", `${Math.ceil(r.height) + 12}px`);
      css.setProperty("--dock-x", `${Math.round(r.left + r.width / 2)}px`);
      css.setProperty("--dock-w", `${Math.round(r.width)}px`);
    };
    set();
    const ro = new ResizeObserver(set);
    ro.observe(el);
    if (el.parentElement) ro.observe(el.parentElement);
    window.addEventListener("resize", set);
    return () => { ro.disconnect(); window.removeEventListener("resize", set); ["--dock-space", "--dock-x", "--dock-w"].forEach((v) => css.removeProperty(v)); };
  }, [open]);
  const area = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);

  const live = useMemo(() => agents.filter((a) => !a.dead && a.provider !== "proxy"), [agents]);
  const fallback = useMemo(() => {
    const sel = live.find((a) => a.id === selectedId);
    if (sel) return [sel.id];
    const lead = live.find((a) => !a.peer && /orchestrat|lead/i.test(a.role ?? ""));
    return lead ? [lead.id] : [];
  }, [live, selectedId]);
  const to = (picked ?? fallback).filter((id) => live.some((a) => a.id === id));
  const targets = to.map((id) => live.find((a) => a.id === id)!).filter(Boolean);

  // Free the thumbnails when they go.
  const previews = useRef(new Set<string>());
  useEffect(() => () => previews.current.forEach((u) => URL.revokeObjectURL(u)), []);

  const addFiles = (list: File[]) => {
    for (const f of list) {
      const key = nextKey();
      const preview = f.type.startsWith("image/") ? URL.createObjectURL(f) : null;
      if (preview) previews.current.add(preview);
      const name = f.name && f.name !== "image.png" ? f.name : `pasted-${new Date().toISOString().slice(11, 19).replace(/:/g, "")}.${(f.type.split("/")[1] || "png").replace("jpeg", "jpg")}`;
      setFiles((fs) => [...fs, { key, name, path: null, preview, error: null }]);
      void toBase64(f)
        .then((b64) => backend.saveAttachment(name, b64))
        .then((path) => setFiles((fs) => fs.map((x) => (x.key === key ? { ...x, path } : x))))
        .catch((e) => setFiles((fs) => fs.map((x) => (x.key === key ? { ...x, error: String(e).replace(/^Error: /, "") } : x))));
    }
  };
  const addPaths = (paths: string[]) => setFiles((fs) => [...fs, ...paths.map((p) => ({ key: nextKey(), name: baseName(p), path: p, preview: null, error: null }))]);
  const remove = (key: string) =>
    setFiles((fs) => {
      const gone = fs.find((x) => x.key === key);
      if (gone?.preview) { URL.revokeObjectURL(gone.preview); previews.current.delete(gone.preview); }
      return fs.filter((x) => x.key !== key);
    });

  // ⌘L focuses; files dropped from Finder onto the dock or empty canvas land here.
  useEffect(() => {
    // Opening renders the textarea first; focus it on the next frame.
    const focus = () => { setOpen(true); requestAnimationFrame(() => area.current?.focus()); };
    const dropped = (e: Event) => { addPaths((e as CustomEvent<string[]>).detail); focus(); };
    window.addEventListener("hubcodes:focus-dock", focus);
    window.addEventListener("hubcodes:dock-files", dropped);
    return () => { window.removeEventListener("hubcodes:focus-dock", focus); window.removeEventListener("hubcodes:dock-files", dropped); };
  }, []);

  useEffect(() => {
    if (!receipt) return;
    const id = window.setTimeout(() => setReceipt(null), 5000);
    return () => window.clearTimeout(id);
  }, [receipt]);

  const saving = files.some((f) => !f.path && !f.error);
  const ready = files.filter((f) => f.path && !f.error);
  const canSend = !busy && !saving && targets.length > 0 && (text.trim().length > 0 || ready.length > 0);

  const send = async (direct: boolean) => {
    if (!canSend) return;
    const paths = ready.map((f) => f.path!);
    const sent: string[] = [];
    for (const a of targets) {
      try {
        if (paths.length && a.peer) setBusy(`Copying ${paths.length === 1 ? "the file" : `${paths.length} files`} to ${a.peer}…`);
        else setBusy(`Sending to ${a.name}…`);
        const message = paths.length ? await backend.composeMessage(a.id, text, paths) : text.trim();
        if (await instruct(a.id, message, direct)) sent.push(a.name);
      } catch (e) {
        notify(`${a.name}: ${String(e).replace(/^Error: /, "")}`, "error");
      }
    }
    setBusy(null);
    if (!sent.length) return;
    setReceipt(`Sent to ${sent.join(", ")}${paths.length ? ` with ${paths.length === 1 ? "1 file" : `${paths.length} files`}` : ""}`);
    setText("");
    files.forEach((f) => f.preview && URL.revokeObjectURL(f.preview));
    setFiles([]);
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    e.stopPropagation();
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      // ⌥Enter: straight to the agent, past the check-in with its lead.
      void send(e.altKey);
    } else if (e.key === "Escape") {
      // Esc leaves the field; on an empty dock it also tucks the dock away.
      (e.target as HTMLTextAreaElement).blur();
      if (!text.trim() && !files.length) setOpen(false);
    }
  };
  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const pasted = Array.from(e.clipboardData.files);
    if (!pasted.length) return;
    e.preventDefault();
    addFiles(pasted);
  };
  // In the browser (and the mock) files arrive here; in the app Tauri hands over paths (App.tsx).
  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    setOver(false);
    const dropped = Array.from(e.dataTransfer.files);
    if (!dropped.length) return;
    e.preventDefault();
    addFiles(dropped);
  };

  const leads = targets.map((a) => ({ a, lead: routeLead(agents, edges, a, routeOn) })).filter((x) => x.lead);
  const hint = busy ?? receipt ?? (leads.length ? `${leads.map((x) => x.a.name).join(", ")} ${leads.length === 1 ? "checks" : "check"} with ${Array.from(new Set(leads.map((x) => x.lead))).join(", ")} first · ⌥↩ skips that` : "↩ send · ⇧↩ new line · / saved prompt");
  const placeholder = targets.length ? `Message ${targets.map((a) => a.name).join(", ")}… paste or drop files` : "Pick who this goes to";

  if (!open) {
    const draft = [text.trim() ? "draft" : "", files.length ? `${files.length} ${files.length === 1 ? "file" : "files"}` : ""].filter(Boolean).join(" · ");
    return (
      <button ref={(el) => { root.current = el; }} type="button" className="dock-pill nodrag" onClick={() => { setOpen(true); requestAnimationFrame(() => area.current?.focus()); }} aria-label="Show the message dock">
        <MessageOutlined />
        <span>Message an agent</span>
        {draft && <span className="dock-pill-draft">{draft}</span>}
        <span className="dock-kbd">⌘L</span>
      </button>
    );
  }

  return (
    <div
      ref={(el) => { root.current = el; }}
      className={`dock nodrag${over ? " is-over" : ""}`}
      role="region"
      aria-label="Message agents"
      onDragOver={(e) => { if (e.dataTransfer.types.includes("Files")) { e.preventDefault(); setOver(true); } }}
      onDragLeave={() => setOver(false)}
      onDrop={onDrop}
    >
      <div className="dock-to">
        <span className="dock-label">To</span>
        <Select
          mode="multiple"
          variant="borderless"
          size="small"
          className="dock-select"
          value={to}
          onChange={(v: string[]) => setPicked(v)}
          placeholder="an agent"
          maxTagCount="responsive"
          suffix={null}
          optionFilterProp="search"
          options={live.map((a) => ({ value: a.id, search: `${a.name} ${a.role ?? ""} ${a.provider}`, label: <AgentLabel a={a} /> }))}
          aria-label="Send to"
        />
        {picked && (
          <Tooltip title="Send to whichever card is selected again">
            <button type="button" className="dock-reset" onClick={() => setPicked(null)}>Follow selection</button>
          </Tooltip>
        )}
        <Tooltip title="Hide (⌘L brings it back)">
          <button type="button" className="dock-icon dock-hide" aria-label="Hide the message dock" onClick={() => setOpen(false)}><DownOutlined /></button>
        </Tooltip>
      </div>
      {files.length > 0 && (
        <ul className="dock-files" aria-label="Attachments">
          {files.map((f) => (
            <li key={f.key} className={`dock-file${f.error ? " is-error" : ""}`} title={f.error ?? f.path ?? f.name}>
              {f.preview ? <img src={f.preview} alt="" /> : <span className="dock-file-icon">{!f.path && !f.error ? <LoadingOutlined /> : <FileOutlined />}</span>}
              <span className="dock-file-name">{f.error ? `${f.name}: ${f.error}` : f.name}</span>
              {f.preview && !f.path && !f.error && <LoadingOutlined className="dock-file-spin" />}
              <button type="button" className="dock-file-x" aria-label={`Remove ${f.name}`} onClick={() => remove(f.key)}><CloseOutlined /></button>
            </li>
          ))}
        </ul>
      )}
      <div className="dock-row">
        <Tooltip title="Attach files">
          <button type="button" className="dock-icon" aria-label="Attach files" onClick={() => picker.current?.click()}><PaperClipOutlined /></button>
        </Tooltip>
        <input
          ref={picker}
          type="file"
          multiple
          hidden
          onChange={(e) => { addFiles(Array.from(e.target.files ?? [])); e.target.value = ""; }}
        />
        <textarea
          ref={area}
          className="dock-input"
          rows={1}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            const el = e.target;
            el.style.height = "auto";
            el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
          }}
          onKeyDown={onKey}
          onPaste={onPaste}
          placeholder={placeholder}
          spellCheck={false}
          aria-label="Message"
        />
        <button type="button" className="dock-send" disabled={!canSend} onClick={() => void send(false)} aria-label="Send">
          {busy ? <LoadingOutlined /> : <SendOutlined />}
        </button>
      </div>
      <div className={`dock-hint${receipt ? " is-done" : ""}`} aria-live="polite">{hint}<span className="dock-kbd">⌘L</span></div>
    </div>
  );
}
