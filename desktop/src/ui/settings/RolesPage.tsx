import { useCallback, useEffect, useState } from "react";
import { Button, Input, Space, Tag, Typography } from "antd";
import { EditOutlined, PlusOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import type { RoleDetail, RoleEntry } from "../../lib/types";
import { EditModal, Group, ItemList, ItemRow, Row, Section } from "./primitives";

const { Text } = Typography;
const MODE: Record<string, string> = { generated: "default", extends: "edited", replaces: "edited" };
const KIND: Record<string, string> = { orchestrator: "coordinates", reviewer: "reviews", tester: "tests", worker: "builds" };

/**
 * Settings → Roles: what an agent with a role is told about its job when it
 * starts. The editor opens on the text hubcodes uses today; a changed text is
 * saved as ~/.config/hubcodes/roles/<role>.md, and the live team facts (who
 * runs, the lead, canvas links) are still added after it.
 */
export function RolesPage() {
  const notify = useStore((s) => s.notify);
  const [roles, setRoles] = useState<RoleEntry[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [detail, setDetail] = useState<RoleDetail | null>(null);
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  const [newRole, setNewRole] = useState("");

  const load = useCallback(() => backend.rolesList().then(setRoles).catch((e) => notify(String(e), "error")), [notify]);
  useEffect(() => { void load(); }, [load]);

  const edit = async (role: string) => {
    const d = await backend.roleGet(role);
    setOpen(role);
    setDetail(d);
    setText(d.text);
  };
  const changed = detail !== null && text.trim() !== detail.text.trim();
  const isDefault = detail !== null && text.trim() === detail.default.trim();

  const save = async (value: string) => {
    if (!open) return;
    setSaving(true);
    try {
      const file = await backend.roleSave(open, value);
      notify(file ? `Saved the ${open} role. Restart its agents to use it.` : `The ${open} role uses the default again.`);
      await load();
      setOpen(null);
      setDetail(null);
    } catch (e) {
      notify(String(e), "error");
    }
    setSaving(false);
  };

  const add = () => {
    const r = newRole.trim();
    if (!r) return;
    setNewRole("");
    void edit(r);
  };

  const close = () => { setOpen(null); setDetail(null); };
  return (
    <Section
      title="Roles"
      tagline="What an agent with each role is told about its job when it starts. The team, its lead and the links are added after it."
    >
      <Group title="Roles" desc="The name picks the starting text: lead, orchestrator or planner coordinate; review reviews; test or qa tests; anything else builds.">
        <ItemList empty="No roles yet.">
          {roles.map((r) => (
            <ItemRow
              key={r.role}
              name={<span>{r.role}</span>}
              summary={`${KIND[r.kind] ?? r.kind}${r.file ? ` · ${r.file}` : ""}`}
              status={<Tag variant="filled" color={r.mode === "generated" ? undefined : "processing"}>{MODE[r.mode] ?? r.mode}</Tag>}
              actions={<Button size="small" icon={<EditOutlined />} onClick={() => void edit(r.role)} aria-label={`Edit the ${r.role} role`}>Edit</Button>}
              onOpen={() => void edit(r.role)}
            />
          ))}
        </ItemList>
        <Row label="New role" hint="Any name; it starts from the text for its kind.">
          <Space.Compact>
            <Input value={newRole} onChange={(e) => setNewRole(e.target.value)} placeholder="e.g. security reviewer" onPressEnter={add} aria-label="New role name" />
            <Button icon={<PlusOutlined />} disabled={!newRole.trim()} onClick={add}>Add</Button>
          </Space.Compact>
        </Row>
      </Group>

      <EditModal
        title={open ? <span>Role: {open} {detail?.file ? <Tag variant="filled" color="processing">edited</Tag> : <Tag variant="filled">default</Tag>}</span> : ""}
        open={!!open && !!detail}
        onClose={close}
        onApply={() => (changed ? void save(text) : close())}
        applyLabel={changed ? "Save role" : "Done"}
        width={820}
        extra={detail && !isDefault ? <Button type="link" onClick={() => (detail.file ? void save("") : setText(detail.default))} disabled={saving}>Back to the default</Button> : null}
      >
        {detail && (
          <div className="role-editor">
            <Input.TextArea
              className="mono"
              value={text}
              onChange={(e) => setText(e.target.value)}
              autoSize={{ minRows: 12, maxRows: 26 }}
              aria-label={`Text for the ${open} role`}
              spellCheck={false}
            />
            <div className="role-editor-help">
              <Button size="small" onClick={() => setText((v) => `${v}${v.endsWith(" ") || !v ? "" : " "}{lead}`)}>Insert {"{lead}"}</Button>
              <Text type="secondary">{"{lead}"} becomes the orchestrator's name. Say what the job is, what it must not do, and how it reports (<code>hubcodes send</code>, <code>hubcodes task done</code>).</Text>
            </div>
            <Text type="secondary" className="role-editor-note">Saved to {detail.file ?? `~/.config/hubcodes/roles/<role>.md`}. Running agents keep what they were told until restarted from their card (Restart with the current brief) or with <code>hubcodes restart &lt;name&gt;</code>.</Text>
          </div>
        )}
      </EditModal>
    </Section>
  );
}
