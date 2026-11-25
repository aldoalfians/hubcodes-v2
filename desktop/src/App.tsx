import { useEffect, useState } from "react";
import { FixActions } from "./ui/FixActions";
import { backend, isTauri } from "./lib/backend";
import { ReactFlowProvider, useReactFlow } from "@xyflow/react";
import { useStore } from "./store";
import { Canvas } from "./canvas/Canvas";
import { Rail } from "./ui/Rail";
import { Boot } from "./ui/Boot";
import { UpdateBar } from "./ui/UpdateBar";
import { TopBar } from "./ui/TopBar";
import { Activity } from "./ui/Activity";
import { Sheet } from "./ui/Sheet";
import { CostDialog } from "./ui/CostDialog";
import { RoomsDialog } from "./ui/RoomsDialog";
import { askText } from "./ui/ask";
import { NewAgentDialog } from "./ui/NewAgentDialog";
import { SettingsDialog } from "./ui/SettingsDialog";
import { Palette } from "./ui/Palette";
import { MessageDock } from "./ui/MessageDock";
import { EmptyState } from "./ui/EmptyState";
import { ContextMenu } from "./ui/ContextMenu";
import { ConfirmDialog } from "./ui/ConfirmDialog";
import { LoopDialog } from "./ui/LoopDialog";
import { Inbox } from "./ui/Inbox";
import { Board } from "./ui/Board";
import { FanoutDialog } from "./ui/FanoutDialog";
import { KnowledgeDialog } from "./ui/KnowledgeDialog";
import { PlaybookDialog } from "./ui/PlaybookDialog";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import { Alert, Button, Space } from "antd";

function Shortcuts() {
  const rf = useReactFlow();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useStore.getState();
      const meta = e.metaKey || e.ctrlKey;
      const inField = (e.target as HTMLElement)?.closest?.("input, textarea, .xterm");
      if (meta && e.shiftKey && e.key.toLowerCase() === "n") { e.preventDefault(); s.toggleNotes(); return; }
      if (meta && e.key.toLowerCase() === "n") { e.preventDefault(); s.setDialog("new-agent"); return; }
      if (meta && e.shiftKey && e.key.toLowerCase() === "k") { e.preventDefault(); s.setDialog(s.dialog === "knowledge" ? "none" : "knowledge"); return; }
      if (meta && e.key.toLowerCase() === "k") { e.preventDefault(); s.setDialog(s.dialog === "palette" ? "none" : "palette"); return; }
      if (meta && e.key === ",") { e.preventDefault(); s.setDialog("settings"); return; }
      if (meta && e.key.toLowerCase() === "j") { e.preventDefault(); s.toggleActivity(); return; }
      if (meta && e.shiftKey && e.key.toLowerCase() === "g") { e.preventDefault(); newBand(rf); return; }
      if (meta && e.shiftKey && e.key.toLowerCase() === "u") { e.preventDefault(); s.setDialog(s.dialog === "cost" ? "none" : "cost"); return; }
      if (meta && e.key.toLowerCase() === "i") { e.preventDefault(); s.setDialog(s.dialog === "inbox" ? "none" : "inbox"); return; }
      if (meta && e.key.toLowerCase() === "b") { e.preventDefault(); s.toggleBoard(); return; }
      // ⌘L only: Ctrl+L stays the terminal's clear-screen.
      if (e.metaKey && !e.shiftKey && e.key.toLowerCase() === "l") { e.preventDefault(); window.dispatchEvent(new Event("hubcodes:focus-dock")); return; }
      if (meta && e.shiftKey && (e.key === "]" || e.key === "}")) { e.preventDefault(); jumpNeedsYou(rf, 1); return; }
      if (meta && e.shiftKey && (e.key === "[" || e.key === "{")) { e.preventDefault(); jumpNeedsYou(rf, -1); return; }
      if (meta && e.key === "0") { e.preventDefault(); rf.fitView({ padding: 0.15, duration: 200, maxZoom: 1 }); return; }
      if (meta && (e.key === "=" || e.key === "+")) { e.preventDefault(); rf.zoomIn({ duration: 120 }); return; }
      if (meta && e.key === "-") { e.preventDefault(); rf.zoomOut({ duration: 120 }); return; }
      if (meta && /^[1-9]$/.test(e.key)) {
        const a = s.agents[Number(e.key) - 1];
        if (a) { e.preventDefault(); s.select(a.id); }
        return;
      }
      if (inField || s.dialog !== "none" || s.openId || s.confirm || s.menu) return;
      if (e.key === "Enter" && s.selectedId) { e.preventDefault(); s.open(s.selectedId); }
      if ((e.key === "Backspace" || e.key === "Delete") && !s.selectedEdge && s.selectedId) { e.preventDefault(); s.askStop(s.selectedId); }
      if (e.key === "Escape") {
        if (s.linkingFrom) s.startLink(null);
        else s.select(null);
      }
    };
    // The webview's own menu (Reload…) is never what the user wants here.
    const onMenu = (e: MouseEvent) => {
      if (!(e.target as HTMLElement)?.closest?.("input, textarea")) e.preventDefault();
    };
    const onJump = (e: Event) => jumpNeedsYou(rf, (e as CustomEvent<number>).detail || 1);
    const onBand = () => newBand(rf);
    window.addEventListener("hubcodes:new-band", onBand);
    const onFocus = (e: Event) => focusAgent(rf, (e as CustomEvent<string>).detail);
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onMenu);
    window.addEventListener("hubcodes:jump-needs", onJump);
    window.addEventListener("hubcodes:focus-agent", onFocus);
    // Files dropped from Finder: the paths go into the terminal of the card
    // under the pointer, or its composer when it has no terminal here.
    let unlisten: (() => void) | undefined;
    if (isTauri) {
      import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((ev) => {
          if (ev.payload.type !== "drop") return;
          const { x, y } = ev.payload.position;
          const scale = window.devicePixelRatio || 1;
          const under = document.elementFromPoint(x / scale, y / scale);
          // Settings → Skills: a SKILL.md, .zip or .skill file, or a folder, to add.
          if (under?.closest?.(".sk-add-zone")) {
            window.dispatchEvent(new CustomEvent("hubcodes:skill-files", { detail: ev.payload.paths }));
            return;
          }
          const el = under?.closest?.(".react-flow__node");
          const agentId = el?.getAttribute("data-id");
          // The message dock, or empty canvas: the files go with the next message.
          if (under?.closest?.(".dock") || (!agentId && under?.closest?.(".react-flow__pane"))) {
            window.dispatchEvent(new CustomEvent("hubcodes:dock-files", { detail: ev.payload.paths }));
            return;
          }
          if (!agentId || agentId.startsWith("note:")) return;
          const agent = useStore.getState().agents.find((a) => a.id === agentId);
          if (agent && !agent.peer) {
            const quoted = ev.payload.paths.map((p) => (/[\s"']/.test(p) ? `'${p.replace(/'/g, "'\\''")}'` : p)).join(" ");
            void backend.write(agentId, `${quoted} `).catch(() => {});
            return;
          }
          window.dispatchEvent(new CustomEvent("hubcodes:drop-paths", { detail: { agentId, paths: ev.payload.paths } }));
        }),
      ).then((u) => { unlisten = u; }).catch(() => {});
    }
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onMenu);
      window.removeEventListener("hubcodes:jump-needs", onJump);
      window.removeEventListener("hubcodes:new-band", onBand);
      window.removeEventListener("hubcodes:focus-agent", onFocus);
      unlisten?.();
    };
  }, [rf]);
  return null;
}

/** A new band at the middle of the view, named by the user. */
function newBand(rf: ReturnType<typeof useReactFlow>) {
  void askText("New band", { placeholder: "backend, review, …", hint: "A titled box: cards inside move with it; collapse it, send to all, link a lead to all." }).then((name) => {
    if (!name) return;
    placeBand(rf, name);
  });
}
function placeBand(rf: ReturnType<typeof useReactFlow>, name: string) {
  const s = useStore.getState();
  if (s.groups[name]) { s.notify(`A band called ${name} already exists`, "error"); return; }
  const stage = document.querySelector<HTMLElement>(".stage");
  const w = 1000;
  const h = 480;
  const c = rf.screenToFlowPosition({ x: (stage?.clientWidth ?? 1200) / 2 + (stage?.getBoundingClientRect().left ?? 0), y: (stage?.clientHeight ?? 800) / 2 + (stage?.getBoundingClientRect().top ?? 0) });
  s.addGroup(name, { x: Math.round(c.x - w / 2), y: Math.round(c.y - h / 2), w, h, color: null, collapsed: false });
}

/** Select an agent and bring its card into view. */
export function focusAgent(rf: ReturnType<typeof useReactFlow>, id: string) {
  const s = useStore.getState();
  if (!s.agents.some((a) => a.id === id)) return;
  s.select(id);
  if (s.openId) s.open(null);
  void rf.fitView({ nodes: [{ id }], duration: 240, maxZoom: 1, padding: 0.3 });
}

/** ⌘⇧] / ⌘⇧[: the next / previous agent that needs you, from the selected one. */
function jumpNeedsYou(rf: ReturnType<typeof useReactFlow>, dir: number) {
  const s = useStore.getState();
  const waiting = s.agents.filter((a) => a.status === "waiting");
  if (waiting.length === 0) { s.notify("Nobody needs you right now"); return; }
  const at = waiting.findIndex((a) => a.id === s.selectedId);
  const next = waiting[((at < 0 ? (dir > 0 ? -1 : 0) : at) + dir + waiting.length) % waiting.length];
  focusAgent(rf, next.id);
}

/** Saved agents that are not running: offer to bring them back. */
function RestoreBanner() {
  const restorable = useStore((s) => s.restorable);
  const notify = useStore((s) => s.notify);
  const [busy, setBusy] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  if (dismissed || restorable.length === 0) return null;
  // "Start fresh" forgets the saved agents: the canvas stays as it is now.
  const fresh = async () => {
    setBusy(true);
    try {
      await backend.discardSavedSession();
      notify(`Forgot ${restorable.length} saved agent${restorable.length === 1 ? "" : "s"}`);
      setDismissed(true);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const go = async (resume: boolean) => {
    setBusy(true);
    try {
      const r = await backend.restoreSession(resume);
      const started = r.started.length ? `Restored ${r.started.join(", ")}` : "Nothing restored";
      notify(r.errors.length ? `${started} · ${r.errors[0]}` : started, r.errors.length ? "error" : "info");
      setDismissed(true);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="restore-banner" role="status">
      <Alert
        type="warning"
        showIcon
        closable
        onClose={() => setDismissed(true)}
        title={<>{restorable.length} saved agent{restorable.length === 1 ? "" : "s"} not running: <b>{restorable.join(", ")}</b></>}
        action={
          <Space size={6}>
            <Button size="small" type="primary" loading={busy} onClick={() => void go(true)} title="Start them again and continue their last conversation where the provider can (claude -c, codex resume --last)">Restore and continue</Button>
            <Button size="small" disabled={busy} onClick={() => void fresh()} title="Forget them; they will not be offered again">Start fresh</Button>
          </Space>
        }
      />
    </div>
  );
}

/** Agents started with an older team brief keep the old text until restarted. */
function StaleBriefBanner() {
  const stale = useStore((s) => s.staleBrief);
  const restorable = useStore((s) => s.restorable);
  const notify = useStore((s) => s.notify);
  const [busy, setBusy] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  if (dismissed || stale.length === 0 || restorable.length > 0) return null;
  const go = async () => {
    setBusy(true);
    try {
      const r = await backend.restartAgents(stale, true);
      notify(r.errors.length ? `Restarted ${r.started.join(", ")} · ${r.errors[0]}` : `Restarted ${r.started.join(", ")} with the current brief`, r.errors.length ? "error" : "info");
      setDismissed(true);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="restore-banner" role="status">
      <Alert
        type="warning"
        showIcon
        closable
        onClose={() => setDismissed(true)}
        title={<>{stale.length} agent{stale.length === 1 ? "" : "s"} started with an older team brief: <b>{stale.join(", ")}</b></>}
        action={<Button size="small" type="primary" loading={busy} onClick={() => void go()} title="Stop and start them again with the current brief; links, roles and folders are kept and the conversation continues where the provider can">Restart with the current brief</Button>}
      />
    </div>
  );
}

/** First run: something in the environment would keep agents from showing up. */
function SetupBanner() {
  const report = useStore((s) => s.doctor);
  const dismissed = useStore((s) => s.doctorDismissed);
  const dismiss = useStore((s) => s.dismissDoctor);
  const openSettings = useStore((s) => s.openSettings);
  const restorable = useStore((s) => s.restorable);
  if (!report || dismissed || report.fail === 0 || restorable.length > 0) return null;
  const failed = report.checks.filter((c) => c.level === "fail");
  return (
    <div className="restore-banner setup-banner" role="status">
      <Alert
        type="error"
        showIcon
        closable
        onClose={dismiss}
        title={<>Setup needs attention: <b>{failed.map((c) => c.title).join(", ")}</b></>}
        description={failed[0] && <>{failed[0].fix}{" "}<FixActions command={failed[0].command} open={failed[0].open} /></>}
        action={<Button size="small" type="primary" onClick={() => { dismiss(); openSettings("doctor"); }}>Open Doctor</Button>}
      />
    </div>
  );
}

function Toast() {
  const toast = useStore((s) => s.toast);
  if (!toast) return null;
  return <div className="toast" role="status"><Alert type={toast.kind === "error" ? "error" : "info"} showIcon title={toast.text} /></div>;
}

export default function App() {
  const ready = useStore((s) => s.ready);
  const boot = useStore((s) => s.boot);
  const agents = useStore((s) => s.agents);
  const dialog = useStore((s) => s.dialog);
  const openId = useStore((s) => s.openId);
  const activityOpen = useStore((s) => s.activityOpen);
  const linking = useStore((s) => s.linkingFrom !== null);
  const boardOpen = useStore((s) => s.boardOpen);
  useEffect(() => {
    void boot();
  }, [boot]);
  // Release check: a few seconds after launch, then every 6 hours. Nothing
  // installs on its own; the bar at the bottom asks first.
  useEffect(() => {
    if (!ready) return;
    const check = () => void useStore.getState().checkUpdate();
    const first = window.setTimeout(check, 4000);
    const every = window.setInterval(check, 6 * 60 * 60 * 1000);
    return () => { window.clearTimeout(first); window.clearInterval(every); };
  }, [ready]);

  const setDialog = useStore((s) => s.setDialog);
  const closeDialog = () => setDialog("none");
  return (
    <ErrorBoundary label="hubcodes">
    <ReactFlowProvider>
      <div className={`app${activityOpen ? " has-activity" : ""}${linking ? " is-linking" : ""}`}>
        <TopBar />
        <Rail />
        <main className="stage">
          <Canvas />
          {ready && <RestoreBanner />}
          {ready && <StaleBriefBanner />}
          {ready && <SetupBanner />}
          {ready && agents.length === 0 && <EmptyState />}
          {ready && agents.length > 0 && <MessageDock />}
          {openId && <Sheet />}
        </main>
        {boardOpen && <Board />}
        {activityOpen && <Activity />}
        {dialog !== "none" && (
          <ErrorBoundary label="This dialog" onReset={closeDialog} key={dialog}>
            {dialog === "new-agent" && <NewAgentDialog />}
            {dialog === "settings" && <SettingsDialog />}
            {dialog === "palette" && <Palette />}
            {dialog === "loop" && <LoopDialog />}
            {dialog === "inbox" && <Inbox />}
            {dialog === "fanout" && <FanoutDialog />}
            {dialog === "knowledge" && <KnowledgeDialog />}
            {dialog === "playbook" && <PlaybookDialog />}
            {dialog === "cost" && <CostDialog />}
            {dialog === "rooms" && <RoomsDialog />}
          </ErrorBoundary>
        )}
        <ContextMenu />
        <ConfirmDialog />
        <Toast />
        <Shortcuts />
        <UpdateBar />
        <Boot />
      </div>
    </ReactFlowProvider>
    </ErrorBoundary>
  );
}
