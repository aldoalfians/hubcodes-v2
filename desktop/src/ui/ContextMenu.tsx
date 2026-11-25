import { useEffect, useRef } from "react";
import { Menu, type MenuProps } from "antd";
import { DeleteOutlined, ExportOutlined, LinkOutlined, RetweetOutlined, SendOutlined, DisconnectOutlined } from "@ant-design/icons";
import { useStore } from "../store";

/** Right-click menu for an agent (node or rail item). */
export function ContextMenu() {
  const menu = useStore((s) => s.menu);
  const agents = useStore((s) => s.agents);
  const edges = useStore((s) => s.edges);
  const closeMenu = useStore((s) => s.closeMenu);
  const open = useStore((s) => s.open);
  const startLink = useStore((s) => s.startLink);
  const askStop = useStore((s) => s.askStop);
  const disconnect = useStore((s) => s.disconnect);
  const openLoopDialog = useStore((s) => s.openLoopDialog);
  const openFanout = useStore((s) => s.openFanout);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) closeMenu();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeMenu();
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [menu, closeMenu]);
  if (!menu) return null;
  const a = agents.find((x) => x.id === menu.agentId);
  if (!a) return null;
  const links = edges.filter((e) => e.from === a.name || e.to === a.name);
  const x = Math.min(menu.x, window.innerWidth - 260);
  const y = Math.min(menu.y, window.innerHeight - 60 - 36 * (4 + links.length * 2));
  const items: MenuProps["items"] = [
    { key: "title", type: "group", label: `${a.name}${a.role ? ` · ${a.role}` : ""}` },
    { type: "divider" },
    { key: "open", icon: <ExportOutlined />, label: "Open", extra: <kbd>Enter</kbd> },
    { key: "link", icon: <LinkOutlined />, label: "Link to another agent…" },
    { key: "fanout", icon: <SendOutlined />, label: "Fan out a prompt from here…" },
    ...links.flatMap((e) => {
      const other = e.from === a.name ? e.to : e.from;
      return [
        { key: `loop:${e.from}>${e.to}`, icon: <RetweetOutlined />, label: `Loop with ${other}…` },
        { key: `unlink:${e.from}>${e.to}`, icon: <DisconnectOutlined />, label: `Remove link with ${other}` },
      ];
    }),
    ...(!a.peer ? [{ type: "divider" as const }, { key: "remove", icon: <DeleteOutlined />, danger: true, label: "Remove agent", extra: <kbd>⌫</kbd> }] : []),
  ];
  const onClick: MenuProps["onClick"] = ({ key }) => {
    if (key === "open") { open(a.id); closeMenu(); return; }
    if (key === "link") { startLink(a.id); return; }
    if (key === "fanout") { openFanout([a.name]); return; }
    if (key === "remove") { askStop(a.id); return; }
    const [kind, pair] = key.split(":");
    const [from, to] = (pair ?? "").split(">");
    if (kind === "loop") openLoopDialog(a.name, from === a.name ? to : from);
    if (kind === "unlink") { void disconnect(from, to); closeMenu(); }
  };
  return (
    <div ref={ref} className="menu" role="menu" style={{ left: x, top: y }} aria-label={`${a.name} menu`}>
      <Menu items={items} onClick={onClick} selectable={false} />
    </div>
  );
}
