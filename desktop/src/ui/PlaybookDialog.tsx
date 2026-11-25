import { useEffect, useState } from "react";
import { Alert, Button, Form, Input, Select, Space, Steps, Typography } from "antd";
import { PlayCircleOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import { Dialog } from "./Dialog";
import type { Playbook } from "../lib/types";

const { Text } = Typography;

/** Start a playbook run: pick the playbook, state the goal, choose the folder. */
export function PlaybookDialog() {
  const setDialog = useStore((s) => s.setDialog);
  const notify = useStore((s) => s.notify);
  const session = useStore((s) => s.session);
  const [list, setList] = useState<Playbook[]>([]);
  const [name, setName] = useState("");
  const [goal, setGoal] = useState("");
  const [cwd, setCwd] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    backend.playbooks(cwd.trim() || null).then((p) => { setList(p); if (!name && p[0]) setName(p[0].name); }).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd]);
  const pb = list.find((p) => p.name === name);
  const submit = async () => {
    if (!name || !goal.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const r = await backend.playbookStart(name, goal.trim(), cwd.trim() || null);
      notify(`Playbook run #${r.id} started (${r.playbook})`);
      setDialog("none");
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };
  return (
    <Dialog title="Run a playbook" onClose={() => setDialog("none")} width={620}>
      <Form layout="vertical" onFinish={() => void submit()}>
        <Form.Item label="Playbook" extra={pb?.description}>
          <Select value={name || undefined} onChange={setName} options={list.map((p) => ({ value: p.name, label: p.name }))} placeholder="Pick a playbook" />
        </Form.Item>
        {pb && (
          <Steps
            size="small"
            className="pb-steps"
            items={pb.step.map((s) => ({ title: s.name, description: s.action ? `action: ${s.action}` : `${s.agent || (s.role ? `role ${s.role}` : "any idle agent")}${s.gate && s.gate !== "none" ? ` · gate ${s.gate}` : ""}` }))}
          />
        )}
        <Form.Item label="Goal" extra="Replaces {goal} in every step's prompt. Be concrete about what finished looks like.">
          <Input.TextArea rows={3} autoFocus value={goal} onChange={(e) => setGoal(e.target.value)} placeholder="Add refresh tokens to the auth API with tests" />
        </Form.Item>
        <Form.Item label="Folder" extra="Tests run here; the repository's own playbooks and policy apply.">
          <Input className="mono" value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder={session?.defaultFolder ?? session?.home ?? "repository folder"} spellCheck={false} />
        </Form.Item>
        {error && <Alert type="error" showIcon title={error} style={{ marginBottom: 12 }} />}
        <div className="dialog-actions">
          <Text type="secondary">Progress shows on the Board (⌘B).</Text>
          <span className="grow" />
          <Space>
            <Button onClick={() => setDialog("none")}>Cancel</Button>
            <Button type="primary" htmlType="submit" icon={<PlayCircleOutlined />} loading={busy} disabled={!name || !goal.trim()}>Start run</Button>
          </Space>
        </div>
      </Form>
    </Dialog>
  );
}
