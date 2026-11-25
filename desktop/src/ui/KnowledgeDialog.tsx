import { useEffect, useRef, useState } from "react";
import { Alert, Button, Input, List, Select, Space, Typography, type InputRef } from "antd";
import { ArrowLeftOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { backend } from "../lib/backend";
import type { KbHit, MemoryItem } from "../lib/types";
import { Dialog } from "./Dialog";

const { Text, Paragraph } = Typography;

/** Search every knowledge source and the team memory; send a hit to an agent. */
export function KnowledgeDialog() {
  const setDialog = useStore((s) => s.setDialog);
  const agents = useStore((s) => s.agents);
  const instruct = useStore((s) => s.instruct);
  const notify = useStore((s) => s.notify);
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<KbHit[]>([]);
  const [mem, setMem] = useState<MemoryItem[]>([]);
  const [errors, setErrors] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<{ hit: KbHit; text: string } | null>(null);
  const input = useRef<InputRef>(null);
  useEffect(() => input.current?.focus(), []);
  const search = async () => {
    if (!q.trim()) return;
    setBusy(true);
    try {
      const [k, m] = await Promise.all([backend.kbSearch(q.trim(), null, 8), backend.memoryRecall(q.trim(), 5).catch(() => [])]);
      setHits(k.hits);
      setErrors(k.errors);
      setMem(m);
    } catch (e) {
      setErrors([String(e)]);
    } finally {
      setBusy(false);
    }
  };
  const openHit = async (h: KbHit) => {
    if (!h.reference) { setOpen({ hit: h, text: h.snippet }); return; }
    try {
      setOpen({ hit: h, text: await backend.kbGet(h.source, h.reference) });
    } catch (e) {
      notify(String(e), "error");
    }
  };
  const sendTo = (agentId: string, h: KbHit) => {
    void instruct(agentId, `Read this before continuing (hubcodes kb get ${h.source} "${h.reference}"): ${h.title} — ${h.snippet}`);
    notify(`Sent “${h.title}” to ${agents.find((a) => a.id === agentId)?.name ?? agentId}`);
  };
  return (
    <Dialog title="Knowledge and memory" onClose={() => setDialog("none")} width={780}>
      <Input.Search ref={input} value={q} onChange={(e) => setQ(e.target.value)} onSearch={() => void search()} loading={busy} enterButton="Search" placeholder="Search every source: docs, Confluence, Jira, team memory…" spellCheck={false} className="kb-search" />
      {errors.map((e) => <Alert key={e} type="error" showIcon title={e} style={{ marginTop: 8 }} />)}
      {open ? (
        <div className="kb-open">
          <Space><Button type="text" size="small" icon={<ArrowLeftOutlined />} onClick={() => setOpen(null)}>Results</Button><Text strong>{open.hit.title}</Text><Text type="secondary">{open.hit.source}</Text></Space>
          <pre className="kb-text">{open.text}</pre>
        </div>
      ) : (
        <>
          {mem.length > 0 && (
            <List
              className="kb-section"
              size="small"
              header={<Text strong>Team memory</Text>}
              dataSource={mem}
              renderItem={(m) => <List.Item><div><div>{m.text}</div><Text type="secondary">{m.source}</Text></div></List.Item>}
            />
          )}
          {hits.length > 0 && (
            <List
              className="kb-section"
              size="small"
              header={<Text strong>Knowledge</Text>}
              dataSource={hits}
              renderItem={(h) => (
                <List.Item actions={[<Select<string> key="send" size="small" value={undefined} placeholder="Send to…" aria-label="Send to agent" style={{ minWidth: 130 }} options={agents.map((a) => ({ value: a.id, label: a.role ? `${a.name} · ${a.role}` : a.name }))} onChange={(v) => sendTo(v, h)} />]}>
                  <List.Item.Meta
                    title={<Space><Button type="link" size="small" style={{ paddingInline: 0 }} onClick={() => void openHit(h)}>{h.title}</Button><Text type="secondary">{h.source}</Text></Space>}
                    description={<><div className="kb-snippet">{h.snippet}</div>{h.reference && <Text type="secondary" className="kb-ref mono">{h.reference}</Text>}</>}
                  />
                </List.Item>
              )}
            />
          )}
          {!busy && hits.length === 0 && mem.length === 0 && <Paragraph type="secondary" style={{ marginTop: 12 }}>Agents use the same sources with <code>hubcodes kb search "…"</code> and <code>hubcodes memory recall "…"</code>. Configure them in Settings → Knowledge and Settings → Memory.</Paragraph>}
        </>
      )}
    </Dialog>
  );
}
