import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AutoComplete, Button, Input, Popconfirm, Select, Tooltip, Typography } from "antd";
import { CloseOutlined, DeleteOutlined, PlusOutlined, SendOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import type { RoomInfo, RoomMessage } from "../lib/types";
import { Dialog } from "./Dialog";

const { Text } = Typography;
const VALID = /^[a-z0-9][a-z0-9_-]{0,31}$/;

/** "3m", "2h", "yesterday": short enough for a chat column. */
function ago(at: number) {
  const s = Math.max(0, Date.now() / 1000 - at);
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return s < 172800 ? "yesterday" : `${Math.floor(s / 86400)}d`;
}

/** `mini/tester` shows its device quietly; `human:aldo` is a person. */
function Who({ name }: { name: string }) {
  if (name.startsWith("human:")) return <span className="rm-who is-human">{name.slice(6)}</span>;
  const i = name.indexOf("/");
  if (i < 0) return <span className="rm-who mono">{name}</span>;
  return (
    <span className="rm-who mono">
      <span className="rm-device">{name.slice(0, i + 1)}</span>
      {name.slice(i + 1)}
    </span>
  );
}

/**
 * Rooms that live on this Mac: agents here and on other devices hear each
 * other's posts. The transcript stays here; members elsewhere get the posts
 * addressed to them.
 */
export function RoomsDialog() {
  const setDialog = useStore((s) => s.setDialog);
  const notify = useStore((s) => s.notify);
  const agents = useStore((s) => s.agents);
  const [rooms, setRooms] = useState<RoomInfo[] | null>(null);
  const [current, setCurrent] = useState<string | null>(null);
  const [log, setLog] = useState<RoomMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [posting, setPosting] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [newMembers, setNewMembers] = useState<string[]>([]);
  const [adding, setAdding] = useState<string | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  const names = useMemo(() => agents.filter((a) => !a.dead && a.provider !== "shell").map((a) => a.name).sort(), [agents]);
  const room = rooms?.find((r) => r.name === current) ?? null;

  const loadRooms = useCallback(async () => {
    try {
      const list = await backend.roomList();
      setRooms(list);
      setCurrent((c) => (c && list.some((r) => r.name === c) ? c : list[0]?.name ?? null));
    } catch (e) {
      notify(String(e), "error");
      setRooms([]);
    }
  }, [notify]);
  const loadLog = useCallback(async (name: string) => {
    try {
      setLog(await backend.roomLog(name, 200));
    } catch {
      setLog([]);
    }
  }, []);

  useEffect(() => { void loadRooms(); }, [loadRooms]);
  useEffect(() => {
    if (!current) { setLog([]); return; }
    void loadLog(current);
    const t = window.setInterval(() => { void loadLog(current); void loadRooms(); }, 2500);
    return () => window.clearInterval(t);
  }, [current, loadLog, loadRooms]);
  useEffect(() => { endRef.current?.scrollIntoView({ block: "end" }); }, [log.length, current]);

  const nameProblem = !newName ? null : !VALID.test(newName) ? "Lower case letters, digits, - and _ (up to 32)." : rooms?.some((r) => r.name === newName) ? "A room with this name exists." : null;
  const create = async () => {
    if (!newName || nameProblem) return;
    try {
      await backend.roomCreate(newName, newMembers);
      notify(`Created room ${newName}`);
      setCreating(false);
      setNewName("");
      setNewMembers([]);
      await loadRooms();
      setCurrent(newName);
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const post = async () => {
    const text = draft.trim();
    if (!room || !text || posting) return;
    setPosting(true);
    try {
      await backend.roomPost(room.name, text);
      setDraft("");
      await loadLog(room.name);
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setPosting(false);
    }
  };
  const members = async (add: string[], remove: string[]) => {
    if (!room) return;
    try {
      await backend.roomMembers(room.name, add, remove);
      await loadRooms();
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const remove = async () => {
    if (!room) return;
    try {
      await backend.roomDelete(room.name);
      notify(`Deleted room ${room.name}`);
      setCurrent(null);
      await loadRooms();
    } catch (e) {
      notify(String(e), "error");
    }
  };

  const createForm = (
    <div className="rm-create">
      <Input
        autoFocus
        value={newName}
        status={nameProblem ? "error" : undefined}
        onChange={(e) => setNewName(e.target.value.toLowerCase().replace(/\s+/g, "-"))}
        onPressEnter={() => void create()}
        placeholder="Room name, e.g. auth"
        aria-label="New room name"
        spellCheck={false}
      />
      <Select mode="multiple" value={newMembers} onChange={setNewMembers} options={names.map((n) => ({ value: n, label: n }))} placeholder="Members (agents here or on a device)" aria-label="New room members" maxTagCount="responsive" />
      {nameProblem && <Text type="danger" className="rm-err">{nameProblem}</Text>}
      <div className="rm-create-actions">
        {rooms && rooms.length > 0 && <Button type="text" onClick={() => setCreating(false)}>Cancel</Button>}
        <Button type="primary" disabled={!newName || !!nameProblem} onClick={() => void create()}>Create room</Button>
      </div>
    </div>
  );

  return (
    <Dialog title="Rooms" onClose={() => setDialog("none")} width={rooms && rooms.length === 0 ? 520 : 1000} className="rooms-dialog">
      {rooms === null ? (
        <div className="rm-loading"><Text type="secondary">Loading…</Text></div>
      ) : rooms.length === 0 ? (
        <div className="rm-first">
          <h3 className="rm-first-title">Start a room</h3>
          <p className="rm-first-text">Agents here and on your other devices hear each other's posts. The room and its transcript stay on this device.</p>
          {createForm}
        </div>
      ) : (
        <div className="rm-layout">
          <nav className="rm-list" aria-label="Rooms">
            {rooms.map((r) => (
              <button key={r.name} type="button" className={`rm-item${r.name === current ? " is-on" : ""}`} aria-current={r.name === current} onClick={() => setCurrent(r.name)}>
                <span className="rm-item-name mono">{r.name}</span>
                <span className="rm-item-count">{r.members.length}</span>
              </button>
            ))}
            {creating ? createForm : (
              <button type="button" className="rm-new" onClick={() => setCreating(true)}><PlusOutlined /> New room</button>
            )}
          </nav>

          {room && (
            <section className="rm-room" aria-label={`Room ${room.name}`}>
              <header className="rm-head">
                <div className="rm-title">
                  <span className="mono">{room.name}</span>
                  <Text type="secondary" className="rm-sub">on this device · {room.members.length} {room.members.length === 1 ? "member" : "members"}</Text>
                </div>
                <Popconfirm title={`Delete room ${room.name}?`} description="Its transcript on this device is deleted too." okText="Delete" okButtonProps={{ danger: true }} onConfirm={() => void remove()}>
                  <Button type="text" size="small" icon={<DeleteOutlined />} aria-label={`Delete room ${room.name}`} className="rm-delete" />
                </Popconfirm>
              </header>

              <div className="rm-members" role="list" aria-label="Members">
                {room.members.map((m) => (
                  <span key={m} className="rm-member" role="listitem">
                    <Who name={m} />
                    <Tooltip title={`Remove ${m}`}>
                      <button type="button" className="rm-member-x" aria-label={`Remove ${m}`} onClick={() => void members([], [m])}><CloseOutlined /></button>
                    </Tooltip>
                  </span>
                ))}
                {adding === room.name ? (
                  <AutoComplete
                    autoFocus
                    size="small"
                    className="rm-add-input"
                    options={names.filter((n) => !room.members.includes(n)).map((n) => ({ value: n }))}
                    onSelect={(v: string) => { setAdding(null); void members([v], []); }}
                    onBlur={() => setAdding(null)}
                    placeholder="agent or device/agent"
                    onKeyDown={(e) => {
                      const v = (e.target as HTMLInputElement).value.trim();
                      if (e.key === "Enter" && v) { setAdding(null); void members([v], []); }
                      if (e.key === "Escape") { e.stopPropagation(); setAdding(null); }
                    }}
                  />
                ) : (
                  <button type="button" className="rm-member-add" onClick={() => setAdding(room.name)}><PlusOutlined /> Add</button>
                )}
              </div>

              <div className="rm-log" aria-live="polite">
                {log.length === 0 && <Text type="secondary" className="rm-empty">Nothing posted yet. What you post goes to every member.</Text>}
                {log.map((m, i) => {
                  const same = i > 0 && log[i - 1].from === m.from && m.at - log[i - 1].at < 300;
                  return (
                    <article key={m.id} className={`rm-msg${same ? " is-cont" : ""}${m.from.startsWith("human:") ? " is-human" : ""}`}>
                      {!same && (
                        <header className="rm-msg-head">
                          <Who name={m.from} />
                          <time className="rm-time" dateTime={new Date(m.at * 1000).toISOString()} title={new Date(m.at * 1000).toLocaleString()}>{ago(m.at)}</time>
                        </header>
                      )}
                      <p className="rm-text">{m.text}</p>
                    </article>
                  );
                })}
                <div ref={endRef} />
              </div>

              <div className="rm-compose">
                <Input.TextArea
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onPressEnter={(e) => { if (!e.shiftKey) { e.preventDefault(); void post(); } }}
                  autoSize={{ minRows: 1, maxRows: 6 }}
                  placeholder={`Post to ${room.name} (Shift+Enter for a new line)`}
                  aria-label={`Post to ${room.name}`}
                />
                <Button type="primary" icon={<SendOutlined />} loading={posting} disabled={!draft.trim()} onClick={() => void post()}>Post</Button>
              </div>
            </section>
          )}
        </div>
      )}
    </Dialog>
  );
}
