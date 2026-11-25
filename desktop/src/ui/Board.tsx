import { useEffect, useMemo, useState } from "react";
import { Button, Dropdown, Input, InputNumber, Modal, Popconfirm, Segmented, Select, Tooltip, Typography, type MenuProps } from "antd";
import { ArrowDownOutlined, ArrowUpOutlined, CheckOutlined, DeleteOutlined, LinkOutlined, LockOutlined, PlayCircleOutlined, PlusOutlined, RollbackOutlined, SearchOutlined, SettingOutlined, StopOutlined } from "@ant-design/icons";
import { Dialog } from "./Dialog";
import { useStore, statusGlyph } from "../store";
import { backend } from "../lib/backend";
import type { BoardColumn, PlaybookRun, Task, TaskHistoryItem } from "../lib/types";

const { Text } = Typography;

/** What the four built-in columns mean to the engine; the user's own columns are holding places. */
const BUILTIN: Record<string, { hint: string; empty: string }> = {
  todo: { hint: "Unowned tasks go to an idle agent with the role, else to any idle agent.", empty: "Nothing queued" },
  doing: { hint: "An agent owns it; the result comes back with hubcodes task done.", empty: "Nobody is on a task" },
  review: { hint: "Waiting for your approval, or for the reviewer's.", empty: "Nothing to review" },
  done: { hint: "Finished tasks.", empty: "No finished tasks yet" },
};
const DEFAULT_COLUMNS: BoardColumn[] = [{ id: "todo", label: "To do" }, { id: "doing", label: "Doing" }, { id: "review", label: "Review" }, { id: "done", label: "Done" }];
const isBuiltin = (id: string) => id in BUILTIN;

const ago = (t: number) => {
  const s = Math.max(0, Date.now() / 1000 - t);
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
};

/**
 * The goal board: tasks the orchestrator (or you) created, who holds them,
 * and what came back. Unowned tasks go to idle agents by role; dragging a
 * card to a column changes its status.
 */
export function Board() {
  const tasks = useStore((s) => s.tasks);
  const agents = useStore((s) => s.agents);
  const session = useStore((s) => s.session);
  const notify = useStore((s) => s.notify);
  const toggleBoard = useStore((s) => s.toggleBoard);
  const runs = useStore((s) => s.runs);
  const setDialog = useStore((s) => s.setDialog);
  const [title, setTitle] = useState("");
  const [role, setRole] = useState("");
  const [view, setView] = useState<"open" | "all">("open");
  const [q, setQ] = useState("");
  // Narrow the board: who holds it, which role it wants, which device the holder is on.
  const [fOwner, setFOwner] = useState<string>("");
  const [fRole, setFRole] = useState<string>("");
  const [fDevice, setFDevice] = useState<string>("");
  const [overCard, setOverCard] = useState<number | null>(null);
  const [detail, setDetail] = useState<Task | null>(null);
  const peers = useStore((s) => s.peers);
  const selfName = useStore((s) => s.selfName);
  const [done, setDone] = useState<Task[]>([]);
  const [drag, setDrag] = useState<number | null>(null);
  const [over, setOver] = useState<Task["status"] | null>(null);
  const [changes, setChanges] = useState<{ task: Task; note: string } | null>(null);
  const [columns, setColumns] = useState<BoardColumn[]>(DEFAULT_COLUMNS);
  const [editCols, setEditCols] = useState<BoardColumn[] | null>(null);
  useEffect(() => {
    backend.boardColumns().then((c) => c.length && setColumns(c)).catch(() => {});
  }, []);
  const add = async () => {
    if (!title.trim()) return;
    try {
      await backend.taskAdd(title.trim(), "", role.trim(), "");
      setTitle("");
    } catch (e) {
      notify(`Could not add the task: ${e}`, "error");
    }
  };
  const move = async (id: number, status: Task["status"]) => {
    try {
      await backend.taskSet(id, status);
      if (status === "done") void loadDone();
    } catch (e) {
      notify(`Could not move the task: ${e}`, "error");
    }
  };
  const loadDone = async () => setDone((await backend.tasksAll(true)).filter((t) => t.status === "done"));
  // Drop on a card: before it; drop on the column: at the end.
  const place = async (id: number, column: string, before: number | null) => {
    if (before === id) return;
    try {
      await backend.taskPlace(id, column, before);
      if (column === "done") void loadDone();
    } catch (e) {
      notify(`Could not move the task: ${e}`, "error");
    }
  };
  const all = useMemo(() => {
    const base = view === "all" ? [...tasks, ...done.filter((d) => !tasks.some((t) => t.id === d.id))] : tasks;
    const needle = q.trim().toLowerCase();
    const deviceOf = (owner: string) => (owner.includes("/") ? owner.split("/")[0] : owner ? "here" : "");
    return base.filter((t) => {
      if (needle && !`${t.id} ${t.title} ${t.owner} ${t.role} ${t.external} ${t.result}`.toLowerCase().includes(needle)) return false;
      if (fOwner && (fOwner === "-" ? t.owner !== "" : t.owner !== fOwner)) return false;
      if (fRole && (fRole === "-" ? t.role !== "" : t.role !== fRole)) return false;
      if (fDevice && deviceOf(t.owner) !== fDevice) return false;
      return true;
    });
  }, [tasks, done, view, q, fOwner, fRole, fDevice]);
  const filtered = !!(fOwner || fRole || fDevice);
  const ownerOptions = useMemo(() => {
    const names = new Set<string>(agents.map((a) => a.name));
    for (const t of tasks) if (t.owner) names.add(t.owner);
    return [{ value: "-", label: "unowned" }, ...Array.from(names).sort().map((n) => ({ value: n, label: n }))];
  }, [agents, tasks]);
  const deviceOptions = useMemo(() => [{ value: "here", label: selfName ? `${selfName} (here)` : "here" }, ...peers.map((p) => ({ value: p.name, label: p.name }))], [peers, selfName]);
  const roleOptions = useMemo(() => {
    const list = [...(session?.roles ?? [])];
    for (const a of agents) if (a.role && !list.includes(a.role)) list.push(a.role);
    return list.map((r) => ({ value: r, label: r }));
  }, [session, agents]);
  const open = tasks.filter((t) => t.status !== "done").length;
  const review = tasks.filter((t) => t.status === "review").length;
  const cols = view === "all" ? columns : columns.filter((c) => c.id !== "done");
  // A task whose column no longer exists shows in the first column, with its raw status.
  const first = cols[0]?.id ?? "todo";
  const columnOf = (t: Task) => (cols.some((c) => c.id === t.status) ? t.status : view === "all" || t.status !== "done" ? first : t.status);
  const saveColumns = async () => {
    if (!editCols) return;
    try {
      setColumns(await backend.boardColumnsSet(editCols));
      setEditCols(null);
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const byId = new Map(agents.map((a) => [a.name, a]));
  const assignItems = (id: number): MenuProps["items"] =>
    agents.length
      ? agents.map((a) => ({
          key: a.name,
          label: <span className="assign-item"><span className={`task-avatar st-${a.status}`}>{statusGlyph[a.status]}</span><span className="mono">{a.name}</span>{a.role && <Text type="secondary">{a.role}</Text>}</span>,
          onClick: () => void backend.taskAssign(id, a.name).catch((err) => notify(String(err), "error")),
        }))
      : [{ key: "none", label: "No agents running", disabled: true }];

  return (
    <Dialog
      title={<span className="board-title"><span className="board-name">Board</span><Text type="secondary" className="board-sub">{open === 0 ? "nothing open" : `${open} open`}{review ? <> · <span className="board-review">{review} to review</span></> : null}</Text></span>}
      onClose={toggleBoard}
      width={1120}
      className="board-dialog"
      extra={<Button size="small" type="text" icon={<PlayCircleOutlined />} onClick={() => setDialog("playbook")} className="board-playbook">Run playbook</Button>}
    >
    <div className="board" aria-label="Goal board">
      <div className="board-tools">
        <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Add a task; an idle agent with the role picks it up" spellCheck={false} onPressEnter={() => void add()} aria-label="New task" prefix={<PlusOutlined className="board-add-plus" />} className="board-add-input" />
        <Select className="board-role" allowClear showSearch value={role || undefined} onChange={(v) => setRole(v ?? "")} options={roleOptions} placeholder="any role" aria-label="Role for the task" popupMatchSelectWidth={220} />
        <Button type="primary" onClick={() => void add()} disabled={!title.trim()}>Add</Button>
        <span className="grow" />
        <Input allowClear prefix={<SearchOutlined />} value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter" className="board-search" aria-label="Filter tasks" />
        <Select allowClear showSearch className="board-filter-sel" value={fOwner || undefined} onChange={(v) => setFOwner(v ?? "")} options={ownerOptions} placeholder="owner" aria-label="Filter by owner" popupMatchSelectWidth={200} />
        <Select allowClear showSearch className="board-filter-sel" value={fRole || undefined} onChange={(v) => setFRole(v ?? "")} options={[{ value: "-", label: "no role" }, ...roleOptions]} placeholder="role" aria-label="Filter by role" popupMatchSelectWidth={200} />
        {peers.length > 0 && <Select allowClear className="board-filter-sel" value={fDevice || undefined} onChange={(v) => setFDevice(v ?? "")} options={deviceOptions} placeholder="device" aria-label="Filter by device" popupMatchSelectWidth={180} />}
        {filtered && <Button type="text" size="small" onClick={() => { setFOwner(""); setFRole(""); setFDevice(""); }}>Clear</Button>}
        <Segmented value={view} options={[{ value: "open", label: "Open" }, { value: "all", label: "With done" }]} onChange={(v) => { setView(v as "open" | "all"); if (v === "all") void loadDone(); }} />
        <Tooltip title="Columns: add your own, rename, reorder"><Button type="text" icon={<SettingOutlined />} aria-label="Board columns" onClick={() => setEditCols(columns.map((c) => ({ ...c })))} /></Tooltip>
      </div>

      {runs.length > 0 && (
        <section className="board-runs" aria-label="Playbook runs">
          {runs.map((r) => <Run key={r.id} r={r} />)}
        </section>
      )}

      <div className={`board-cols cols-${cols.length}`}>
        {cols.map((c) => {
          const items = all.filter((t) => columnOf(t) === c.id);
          const meta = BUILTIN[c.id] ?? { hint: "A column of yours: tasks wait here until someone moves them. Idle agents pick from To do only.", empty: "Nothing here" };
          return (
            <section
              key={c.id}
              className={`board-col col-${c.id}${isBuiltin(c.id) ? "" : " col-custom"}${over === c.id ? " is-over" : ""}`}
              aria-label={c.label}
              onDragOver={(e) => { e.preventDefault(); if (over !== c.id) setOver(c.id); }}
              onDragLeave={() => setOver(null)}
              onDrop={() => { if (drag !== null) { void place(drag, c.id, overCard); setDrag(null); } setOver(null); setOverCard(null); }}
            >
              <header className={`board-col-head${c.wip && items.length >= c.wip ? " is-full" : ""}`} title={c.wip ? `${meta.hint} Limit ${c.wip}: dispatch stops handing out tasks while it is full.` : meta.hint}>
                <span className="board-col-label">{c.label}</span>
                {(items.length > 0 || !!c.wip) && <span className="board-col-count">{items.length}{c.wip ? `/${c.wip}` : ""}</span>}
              </header>
              <div className="board-col-body">
                {items.length === 0 && <div className="board-col-empty">{over === c.id ? "Drop here" : meta.empty}</div>}
                {items.map((t) => {
                  const owner = t.owner ? byId.get(t.owner) : undefined;
                  return (
                    <article
                      key={t.id}
                      className={`task st-${t.status}${overCard === t.id && drag !== null && drag !== t.id ? " is-before" : ""}`}
                      draggable
                      onDragStart={() => setDrag(t.id)}
                      onDragEnd={() => { setDrag(null); setOver(null); setOverCard(null); }}
                      onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); if (over !== c.id) setOver(c.id); if (overCard !== t.id) setOverCard(t.id); }}
                      onDragLeave={() => { if (overCard === t.id) setOverCard(null); }}
                      onDrop={(e) => { e.stopPropagation(); if (drag !== null) void place(drag, c.id, t.id); setDrag(null); setOver(null); setOverCard(null); }}
                      title={t.detail || "Click the title for details"}
                    >
                      <button type="button" className="task-title task-open" onClick={() => setDetail(t)}>{t.title}</button>
                      {t.result && <div className="task-result">{t.result}</div>}
                      <div className="task-meta">
                        <span className="task-id">#{t.id}</span>
                        {t.external && <button type="button" className="task-ext" title={t.externalUrl || t.external} onClick={() => t.externalUrl && void backend.openPath(t.externalUrl)}><LinkOutlined /> {t.external.split(":").slice(1).join(":")}</button>}
                        {t.run > 0 && <span className="task-chip" title={`Playbook run #${t.run}`}>run #{t.run}</span>}
                        {t.role && <span className="task-chip task-role">{t.role}</span>}
                        {columnOf(t) !== t.status && <span className="task-chip" title="This column was removed; move the task or it stays here">{t.status}</span>}
                        <span className="grow" />
                        <Text type="secondary" className="task-when">{ago(t.updated || t.at)}</Text>
                      </div>
                      <div className="task-foot">
                        {t.owner && !owner && t.status !== "done" ? (
                          // The owner is not on the canvas: its device is unreachable or the agent is gone.
                          <Dropdown menu={{ items: assignItems(t.id) }} trigger={["click"]}>
                            <button type="button" className="task-assign task-assign-away" title={`${t.owner} is not reachable now; pick someone else or wait`}>
                              <span className="task-avatar">·</span> <span className="mono">{t.owner}</span> <span className="task-away">away · reassign…</span>
                            </button>
                          </Dropdown>
                        ) : t.owner ? (
                          <span className={`task-owner${owner ? ` st-${owner.status}` : ""}`} title={owner ? `${t.owner}: ${owner.status}` : t.owner}>
                            <span className="task-avatar">{owner ? statusGlyph[owner.status] : "·"}</span>
                            <span className="mono">{t.owner}</span>
                          </span>
                        ) : t.status !== "done" ? (
                          <Dropdown menu={{ items: assignItems(t.id) }} trigger={["click"]}>
                            <button type="button" className="task-assign">Assign…</button>
                          </Dropdown>
                        ) : <span />}
                        <span className="grow" />
                        <span className="task-actions">
                          {t.status === "review" ? (
                            <>
                              <Button size="small" type="primary" icon={<CheckOutlined />} onClick={() => void backend.taskApprove(t.id).then(() => notify(`Approved #${t.id}`)).catch((err) => notify(String(err), "error"))}>Approve</Button>
                              <Button size="small" onClick={() => setChanges({ task: t, note: "" })}>Changes</Button>
                            </>
                          ) : t.status !== "done" ? (
                            <Tooltip title="Mark done"><Button size="small" type="text" className="task-quiet" icon={<CheckOutlined />} aria-label="Mark done" onClick={() => void move(t.id, "done")} /></Tooltip>
                          ) : (
                            <Tooltip title="Reopen"><Button size="small" type="text" className="task-quiet" icon={<RollbackOutlined />} aria-label="Reopen" onClick={() => void move(t.id, "todo")} /></Tooltip>
                          )}
                          <Popconfirm title={`Remove task #${t.id}?`} okText="Remove" okButtonProps={{ danger: true }} onConfirm={() => void backend.taskRemove(t.id).catch((err) => notify(String(err), "error"))}>
                            <Tooltip title="Remove"><Button size="small" type="text" className="task-quiet task-remove" icon={<DeleteOutlined />} aria-label="Remove task" /></Tooltip>
                          </Popconfirm>
                        </span>
                      </div>
                    </article>
                  );
                })}
              </div>
            </section>
          );
        })}
      </div>
      {detail && <TaskDetail task={tasks.find((t) => t.id === detail.id) ?? done.find((t) => t.id === detail.id) ?? detail} columns={columns} roleOptions={roleOptions} onClose={() => setDetail(null)} onMove={(col) => void place(detail.id, col, null)} />}
      <Modal
        open={!!editCols}
        title="Board columns"
        okText="Save columns"
        onCancel={() => setEditCols(null)}
        onOk={() => void saveColumns()}
        destroyOnHidden
        width={520}
      >
        {editCols && <ColumnsEditor cols={editCols} onChange={setEditCols} />}
      </Modal>
      <Modal
        open={!!changes}
        title={changes ? `Request changes on #${changes.task.id}` : ""}
        okText="Send back"
        okButtonProps={{ disabled: !changes?.note.trim() }}
        onCancel={() => setChanges(null)}
        onOk={() => { if (changes) void backend.taskRequestChanges(changes.task.id, changes.note.trim()).catch((err) => notify(String(err), "error")); setChanges(null); }}
        destroyOnHidden
      >
        <Input.TextArea rows={4} autoFocus value={changes?.note ?? ""} onChange={(e) => changes && setChanges({ ...changes, note: e.target.value })} placeholder={`What should ${changes?.task.owner || "the owner"} change?`} />
      </Modal>
    </div>
    </Dialog>
  );
}

/** One task in full: edit what it says, see what came back and what happened to it, move it. */
function TaskDetail({ task, columns, roleOptions, onClose, onMove }: { task: Task; columns: BoardColumn[]; roleOptions: { value: string; label: string }[]; onClose: () => void; onMove: (column: string) => void }) {
  const notify = useStore((s) => s.notify);
  const agents = useStore((s) => s.agents);
  const [title, setTitle] = useState(task.title);
  const [detailText, setDetailText] = useState(task.detail);
  const [role, setRole] = useState(task.role);
  const [history, setHistory] = useState<TaskHistoryItem[] | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    backend.taskHistory(task.id).then(setHistory).catch(() => setHistory([]));
  }, [task.id]);
  const dirty = title.trim() !== task.title || detailText.trim() !== task.detail || role.trim() !== task.role;
  const save = async () => {
    setSaving(true);
    try {
      await backend.taskUpdate(task.id, title, detailText, role);
      notify(`Saved #${task.id}`);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setSaving(false);
    }
  };
  const owner = agents.find((a) => a.name === task.owner);
  const when = (t: number) => new Date(t * 1000).toLocaleString(undefined, { hour12: false, month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  return (
    <Modal
      open
      title={<span className="task-detail-title"><span className="task-id">#{task.id}</span> {task.title}</span>}
      onCancel={onClose}
      width={640}
      footer={[
        <Popconfirm key="rm" title={`Remove task #${task.id}?`} okText="Remove" okButtonProps={{ danger: true }} onConfirm={() => void backend.taskRemove(task.id).then(onClose).catch((e) => notify(String(e), "error"))}><Button danger type="text">Remove</Button></Popconfirm>,
        <span key="grow" className="grow" />,
        <Button key="close" onClick={onClose}>Close</Button>,
        <Button key="save" type="primary" loading={saving} disabled={!dirty || !title.trim()} onClick={() => void save()}>Save</Button>,
      ]}
      className="task-detail"
      destroyOnHidden
    >
      <div className="task-detail-grid">
        <label className="task-detail-label">Title</label>
        <Input value={title} onChange={(e) => setTitle(e.target.value)} aria-label="Title" />
        <label className="task-detail-label">Role</label>
        <Select allowClear showSearch value={role || undefined} onChange={(v) => setRole(v ?? "")} options={roleOptions} placeholder="any role" aria-label="Role" />
        <label className="task-detail-label">Detail</label>
        <Input.TextArea rows={4} value={detailText} onChange={(e) => setDetailText(e.target.value)} placeholder="Files, constraints, how we know it is done" aria-label="Detail" />
        <label className="task-detail-label">Column</label>
        <Select value={task.status} onChange={onMove} options={columns.map((c) => ({ value: c.id, label: c.label }))} aria-label="Column" />
        <label className="task-detail-label">Owner</label>
        <div className="task-detail-value">{task.owner ? <span className={`task-owner${owner ? ` st-${owner.status}` : ""}`}><span className="task-avatar">{owner ? statusGlyph[owner.status] : "·"}</span><span className="mono">{task.owner}</span>{!owner && <Text type="secondary"> · not on the canvas now</Text>}</span> : <Text type="secondary">nobody yet</Text>}</div>
        {task.result && <><label className="task-detail-label">Result</label><div className="task-detail-value task-result">{task.result}</div></>}
        <label className="task-detail-label">About</label>
        <div className="task-detail-value">
          <Text type="secondary">
            created by {task.createdBy || "?"} · {when(task.at)}{task.updated !== task.at ? ` · updated ${when(task.updated)}` : ""}
            {task.external && <> · <button type="button" className="task-ext" onClick={() => task.externalUrl && void backend.openPath(task.externalUrl)}><LinkOutlined /> {task.external}</button></>}
            {task.run > 0 && <> · playbook run #{task.run}</>}
            {task.gate === "review" && <> · needs a reviewer's approval</>}
          </Text>
        </div>
      </div>
      <div className="task-history">
        <Text strong className="task-history-head">What happened</Text>
        {history === null ? <Text type="secondary">Loading…</Text> : history.length === 0 ? <Text type="secondary">Nothing recorded yet.</Text> : (
          <ol className="task-history-list">
            {history.map((h, i) => (
              <li key={i} className={`task-history-item kind-${h.kind}`}>
                <span className="task-history-when">{when(h.at)}</span>
                <span className="task-history-who mono">{h.who}</span>
                <span className="task-history-text">{h.text}</span>
              </li>
            ))}
          </ol>
        )}
      </div>
    </Modal>
  );
}

/** Add, rename, reorder; the four built-in columns can be renamed but not removed. */
function ColumnsEditor({ cols, onChange }: { cols: BoardColumn[]; onChange: (c: BoardColumn[]) => void }) {
  const [label, setLabel] = useState("");
  const slug = (x: string) => x.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  const add = () => {
    const id = slug(label);
    if (!id || cols.some((c) => c.id === id)) return;
    onChange([...cols, { id, label: label.trim(), wip: 0 }]);
    setLabel("");
  };
  const swap = (i: number, j: number) => {
    if (j < 0 || j >= cols.length) return;
    const next = [...cols];
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };
  return (
    <div className="cols-editor">
      <Text type="secondary" className="cols-editor-hint">To do, Doing, Review and Done are what the agents use: rename or move them, but they stay. A column you add is a waiting place; idle agents pick tasks from To do only.</Text>
      {cols.map((c, i) => (
        <div className="cols-row" key={c.id}>
          <Input value={c.label} onChange={(e) => onChange(cols.map((x, k) => (k === i ? { ...x, label: e.target.value } : x)))} aria-label={`Column ${c.id}`} />
          <span className="cols-id mono">{c.id}</span>
          <Tooltip title="Most tasks here (0 = no limit). On Doing, dispatch stops handing out tasks while it is full."><InputNumber min={0} max={99} className="cols-wip" value={c.wip ?? 0} onChange={(v) => onChange(cols.map((x, k) => (k === i ? { ...x, wip: Number(v ?? 0) } : x)))} aria-label={`WIP limit for ${c.label}`} /></Tooltip>
          <Button type="text" size="small" icon={<ArrowUpOutlined />} disabled={i === 0} aria-label="Move up" onClick={() => swap(i, i - 1)} />
          <Button type="text" size="small" icon={<ArrowDownOutlined />} disabled={i === cols.length - 1} aria-label="Move down" onClick={() => swap(i, i + 1)} />
          {isBuiltin(c.id) ? (
            <Tooltip title="Built in: the agents move tasks here"><Button type="text" size="small" icon={<LockOutlined />} disabled aria-label="Built in" /></Tooltip>
          ) : (
            <Tooltip title="Remove; its tasks go to To do"><Button type="text" size="small" danger icon={<DeleteOutlined />} aria-label="Remove column" onClick={() => onChange(cols.filter((_, k) => k !== i))} /></Tooltip>
          )}
        </div>
      ))}
      <div className="cols-row cols-add">
        <Input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="New column, e.g. Backlog or Blocked" onPressEnter={add} aria-label="New column" />
        <Button icon={<PlusOutlined />} onClick={add} disabled={!slug(label) || cols.some((c) => c.id === slug(label))}>Add</Button>
      </div>
    </div>
  );
}

function Run({ r }: { r: PlaybookRun }) {
  const notify = useStore((s) => s.notify);
  const last = r.log.trim().split("\n").pop()?.replace(/^\[\d+\] /, "") ?? "";
  return (
    <div className={`board-run st-${r.status}`}>
      <span className="board-run-dot" aria-hidden />
      <div className="board-run-main">
        <div className="board-run-head">
          <Text strong>#{r.id} {r.playbook}</Text>
          <Text type="secondary">{r.status} · step {r.step + 1}{r.task ? ` · task #${r.task}` : ""}{r.attempt ? ` · retry ${r.attempt}` : ""}</Text>
        </div>
        <Tooltip title={<pre className="board-run-log">{r.log.split("\n").slice(-8).join("\n")}</pre>}><Text type="secondary" className="board-run-last" title={r.goal}>{r.goal}{last ? ` — ${last}` : ""}</Text></Tooltip>
      </div>
      <span className="board-run-actions">
        {r.status === "paused" && <Button size="small" type="primary" icon={<CheckOutlined />} onClick={() => void backend.playbookApprove(r.id).then(() => notify(`Run #${r.id} continues`)).catch((e) => notify(String(e), "error"))}>Approve</Button>}
        <Popconfirm title={`Abort run #${r.id}?`} okText="Abort" okButtonProps={{ danger: true }} onConfirm={() => void backend.playbookAbort(r.id).catch((e) => notify(String(e), "error"))}>
          <Button size="small" type="text" className="task-quiet" icon={<StopOutlined />} aria-label="Abort run" />
        </Popconfirm>
      </span>
    </div>
  );
}
