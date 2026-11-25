import { memo, useState } from "react";
import { Button, Dropdown, Input, Popconfirm, Space, Tooltip, Typography, type MenuProps } from "antd";
import { DeleteOutlined, DownOutlined, LinkOutlined, RightOutlined, SendOutlined } from "@ant-design/icons";
import { NodeResizer, type Node, type NodeProps } from "@xyflow/react";
import { useStore } from "../store";
import { resizeEnd } from "./Canvas";

const { Text } = Typography;

export type BandNodeType = Node<{ name: string; members: string[]; collapsed: boolean; color: string | null }, "band">;

export const BAND_COLORS = ["#22c7e8", "#1fb89f", "#f5a524", "#8957e5", "#db6d28", "#ff5c5c", "#8e9099"];

/**
 * A band: a titled box behind the cards. Cards inside it (by position) are
 * its members; drag the band and they come along; collapse it to hide them.
 * Send to all and Link from… act on every member.
 */
export const BandNode = memo(function BandNode({ data, selected }: NodeProps<BandNodeType>) {
  const agents = useStore((s) => s.agents);
  const setGroup = useStore((s) => s.setGroup);
  const renameGroup = useStore((s) => s.renameGroup);
  const removeGroup = useStore((s) => s.removeGroup);
  const openFanout = useStore((s) => s.openFanout);
  const connect = useStore((s) => s.connect);
  const notify = useStore((s) => s.notify);
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(data.name);
  const members = data.members;
  const others = agents.filter((a) => !members.includes(a.name));
  const color = data.color ?? BAND_COLORS[0];
  const linkItems: MenuProps["items"] = others.length
    ? others.map((a) => ({ key: a.id, label: <span className="mono">{a.name}{a.role ? <Text type="secondary"> · {a.role}</Text> : null}</span>, onClick: () => { for (const m of members) void connect(a.name, m); notify(`${a.name} linked to ${members.length} agent${members.length === 1 ? "" : "s"} in ${data.name}`); } }))
    : [{ key: "none", label: "No agent outside this band", disabled: true }];
  const colorItems: MenuProps["items"] = BAND_COLORS.map((c) => ({ key: c, label: <span className="band-swatch-row"><span className="band-swatch" style={{ background: c }} /> {c}</span>, onClick: () => setGroup(data.name, { color: c }) }));
  return (
    <div className={`band${selected ? " is-selected" : ""}${data.collapsed ? " is-collapsed" : ""}`} style={{ borderColor: color, "--band": color } as React.CSSProperties}>
      <NodeResizer isVisible={selected && !data.collapsed} minWidth={240} minHeight={120} lineClassName="resize-line" handleClassName="resize-handle" onResizeEnd={(_, p) => resizeEnd.current?.(`band:${data.name}`, p.x, p.y, p.width, p.height)} />
      <header className="band-head" style={{ background: color }}>
        <Button size="small" type="text" className="band-toggle" icon={data.collapsed ? <RightOutlined /> : <DownOutlined />} onClick={() => setGroup(data.name, { collapsed: !data.collapsed })} aria-label={data.collapsed ? "Expand" : "Collapse"} />
        {editing ? (
          <Input
            size="small"
            className="band-title-input nodrag"
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onBlur={() => { setEditing(false); if (title.trim() && title !== data.name) renameGroup(data.name, title.trim()); }}
            onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); if (e.key === "Escape") { setTitle(data.name); setEditing(false); } e.stopPropagation(); }}
          />
        ) : (
          <span className="band-title" onDoubleClick={() => { setTitle(data.name); setEditing(true); }} title="Double-click to rename">{data.name}</span>
        )}
        <Text className="band-count">{members.length} agent{members.length === 1 ? "" : "s"}{data.collapsed && members.length ? `: ${members.join(", ")}` : ""}</Text>
        <span className="grow" />
        <Space size={2} className="band-actions nodrag">
          <Tooltip title="One prompt to every agent in the band"><Button size="small" type="text" icon={<SendOutlined />} disabled={members.length === 0} onClick={() => openFanout(members)} aria-label="Send to all" /></Tooltip>
          <Dropdown menu={{ items: linkItems }} trigger={["click"]}><Tooltip title="Link an agent (a lead) to every member"><Button size="small" type="text" icon={<LinkOutlined />} disabled={members.length === 0} aria-label="Link from" /></Tooltip></Dropdown>
          <Dropdown menu={{ items: colorItems }} trigger={["click"]}><Button size="small" type="text" aria-label="Colour"><span className="band-swatch" style={{ background: color }} /></Button></Dropdown>
          <Popconfirm title={`Remove band ${data.name}? The cards stay.`} okText="Remove" okButtonProps={{ danger: true }} onConfirm={() => removeGroup(data.name)}>
            <Button size="small" type="text" icon={<DeleteOutlined />} aria-label="Remove band" />
          </Popconfirm>
        </Space>
      </header>
    </div>
  );
});
