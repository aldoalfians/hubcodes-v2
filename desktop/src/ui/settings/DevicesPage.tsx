import { useState } from "react";
import { Button, Input, Select, Space, Tag } from "antd";
import { ApiOutlined, DesktopOutlined, PlusOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import type { Peer } from "../../lib/types";
import { DevicePanel } from "./DevicePanel";
import { PairingGroup } from "./PairingGroup";
import { EditModal, Field, Fields, FormGroup, Group, ItemList, ItemRow, Result, Row, Section, type Check } from "./primitives";
import { OptionalNumber, code, type Editing } from "./shared";
import { useSettings } from "./context";

const newPeer = (): Peer => ({ name: "", host: "", user: null, session: "hubcodes", hubcodes: "hubcodes", max_agents: 0, allow: [] });
const PEER_ACTIONS = [
  { value: "send", label: "send · message agents here" },
  { value: "task", label: "task · add and report tasks" },
  { value: "spawn", label: "spawn · start agents here" },
  { value: "approve", label: "approve · answer agents' permission prompts" },
  { value: "kill", label: "kill · stop agents" },
  { value: "restart", label: "restart · restart agents" },
];

export function DevicesPage() {
  const { cfg, edit, setListItem, removeListItem, nameTaken } = useSettings();
  const session = useStore((s) => s.session);
  const notify = useStore((s) => s.notify);
  const [peerResult, setPeerResult] = useState<Record<string, Check>>({});
  const [peerEdit, setPeerEdit] = useState<Editing<Peer>>(null);
  const testPeer = async (p: Peer) => {
    setPeerResult((r) => ({ ...r, [p.name]: { ok: true, text: "Checking…", busy: true } }));
    try {
      const text = await backend.checkPeer(p);
      setPeerResult((r) => ({ ...r, [p.name]: { ok: true, text } }));
    } catch (e) {
      setPeerResult((r) => ({ ...r, [p.name]: { ok: false, text: String(e) } }));
    }
  };
  const setPeer = (patch: Partial<Peer>) => setPeerEdit((cur) => (cur ? { ...cur, draft: { ...cur.draft, ...patch } } : cur));
  const applyPeer = () => {
    if (!cfg || !peerEdit) return;
    const d: Peer = { ...peerEdit.draft, name: peerEdit.draft.name.trim(), host: peerEdit.draft.host.trim(), session: peerEdit.draft.session.trim() || "hubcodes", hubcodes: peerEdit.draft.hubcodes.trim() || "hubcodes", user: peerEdit.draft.user?.trim() || null };
    if (!d.name || !d.host) return notify("A device needs a name and a host", "error");
    if (nameTaken(cfg.peers, d.name, peerEdit.index)) return notify(`A device called ${d.name} already exists`, "error");
    setListItem("peers", peerEdit.index, d);
    setPeerEdit(null);
  };
  return (
    <Section
      title="Devices"
      tagline="Agents on another computer, on this canvas."
      help={<>They appear as {code("device/agent")} and can be linked and looped like local ones. Each device runs its own hubcodes and agents; they talk over SSH. <b>Pair with another device</b> below walks through it. Try it on one machine first: host {code("local")} with another session name shows that session's agents as a device.</>}
      extra={<Button type="primary" icon={<PlusOutlined />} onClick={() => setPeerEdit({ index: null, draft: newPeer() })}>Add device</Button>}
    >
      <Group title="This device">
        <Row label="Name" hint={<>One short word. Other devices add this one under this name, and see messages from here as {code(`${cfg.selfName || "mac"}/planner`)}.</>}>
          <Input className="mono st-narrow" value={cfg.selfName} onChange={(e) => edit({ selfName: e.target.value })} placeholder="mac" spellCheck={false} aria-label="This device's name" />
        </Row>
      </Group>
      <PairingGroup selfName={cfg.selfName} paired={cfg.peers.length} onAdd={() => setPeerEdit({ index: null, draft: newPeer() })} />
      <Group title={cfg.peers.length ? `Other devices · ${cfg.peers.length}` : "Other devices"} desc="Add this device on each of them too, under the name above, so their agents can answer.">
        <ItemList empty="No devices yet. Add one to see its agents on this canvas." onAdd={() => setPeerEdit({ index: null, draft: newPeer() })} addLabel="Add device">
          {cfg.peers.map((p, i) => (
            <ItemRow
              key={i}
              icon={<DesktopOutlined />}
              name={<span className="mono">{p.name}</span>}
              meta={p.max_agents ? <Tag variant="filled">up to {p.max_agents} agents</Tag> : undefined}
              summary={<span className="mono">{p.user ? `${p.user}@` : ""}{p.host} · tmux {p.session}</span>}
              status={<Result r={peerResult[p.name]} />}
              actions={<Button size="small" type="text" icon={<ApiOutlined />} onClick={() => void testPeer(p)}>Test</Button>}
              onOpen={() => setPeerEdit({ index: i, draft: { ...p } })}
              onRemove={() => removeListItem("peers", i)}
              removeTitle={`Remove ${p.name}?`}
            />
          ))}
        </ItemList>
      </Group>
      <EditModal
        title={peerEdit?.index === null ? "Add device" : `Edit ${peerEdit?.draft.name ?? ""}`}
        open={!!peerEdit}
        onClose={() => setPeerEdit(null)}
        onApply={applyPeer}
        applyLabel={peerEdit?.index === null ? "Add device" : "Apply"}
        width={620}
        extra={peerEdit && <Space><Button icon={<ApiOutlined />} onClick={() => void testPeer(peerEdit.draft)} disabled={!peerEdit.draft.host.trim() || !peerEdit.draft.name.trim()}>Test</Button><Result r={peerResult[peerEdit.draft.name]} /></Space>}
      >
        {peerEdit && (
          <Fields cols={1}>
            <FormGroup title="Connection">
              <Fields>
                <Field label="Name" hint={<>The name that device gives itself in its Settings → Devices; its agents show here as {code(`${peerEdit.draft.name || "mini"}/tester`)}.</>} required><Input className="mono" value={peerEdit.draft.name} onChange={(e) => setPeer({ name: e.target.value })} placeholder="mini" spellCheck={false} /></Field>
                <Field label="SSH host" hint={<>As that device shows it under Pair with another device (its Tailscale name), an IP, or a ~/.ssh/config Host. {code("local")} is this device.</>} required><Input className="mono" value={peerEdit.draft.host} onChange={(e) => setPeer({ host: e.target.value })} placeholder="mac-mini" spellCheck={false} /></Field>
                <Field label="User" hint="Its login name. Empty: same as here."><Input className="mono" value={peerEdit.draft.user ?? ""} onChange={(e) => setPeer({ user: e.target.value || null })} placeholder={session?.home?.split("/").pop() ?? ""} spellCheck={false} /></Field>
                <Field label="tmux session" hint="Where its agents run."><Input className="mono" value={peerEdit.draft.session} onChange={(e) => setPeer({ session: e.target.value })} placeholder="hubcodes" spellCheck={false} /></Field>
                <Field label="Reach it through" hint={peerEdit.draft.transport === "auto" ? "Its hubcodesd listener ([sharing] listen = true there); SSH while that is not there." : peerEdit.draft.transport === "http" ? "Its hubcodesd listener only; fails when that is off." : "One SSH session per request, as before."}><Select className="st-wide" value={peerEdit.draft.transport || "ssh"} onChange={(v) => setPeer({ transport: v === "ssh" ? "" : v })} options={[{ value: "ssh", label: "SSH" }, { value: "auto", label: "Listener, SSH as fallback" }, { value: "http", label: "Listener only" }]} aria-label="Reach it through" /></Field>
                {(peerEdit.draft.transport === "auto" || peerEdit.draft.transport === "http") && <Field label="Listener port" hint="Default 7434."><OptionalNumber value={peerEdit.draft.port ?? 0} onChange={(v) => setPeer({ port: v || null })} unit="" empty="7434" /></Field>}
                <Field label="hubcodes path" hint="Leave as hubcodes: ~/.local/bin, Homebrew and ~/.cargo/bin are searched there." span><Input className="mono" value={peerEdit.draft.hubcodes} onChange={(e) => setPeer({ hubcodes: e.target.value })} placeholder="hubcodes" spellCheck={false} /></Field>
              </Fields>
            </FormGroup>
            <FormGroup title="Limits">
              <Fields>
                <Field label="Most agents" hint="Starting one more there from here is refused, so a small machine is not overloaded."><OptionalNumber value={peerEdit.draft.max_agents ?? 0} onChange={(v) => setPeer({ max_agents: v })} unit="agents" empty="No limit" /></Field>
                <Field label="May do here" hint="What it may run on this device. Empty: send, task and spawn for your own devices; view only for a device on another Tailscale login (its messages wait in the Inbox). Never approve, kill or restart unless listed."><Select mode="multiple" className="st-wide" value={peerEdit.draft.allow ?? []} options={PEER_ACTIONS} onChange={(v) => setPeer({ allow: v })} placeholder="send, task, spawn" aria-label="May do here" maxTagCount="responsive" /></Field>
              </Fields>
            </FormGroup>
            {peerEdit.index !== null && peerEdit.draft.name.trim() && <DevicePanel name={peerEdit.draft.name.trim()} />}
          </Fields>
        )}
      </EditModal>
    </Section>
  );
}
