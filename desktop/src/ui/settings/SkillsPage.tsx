import { useCallback, useEffect, useMemo, useState } from "react";
import { Button, Input, Segmented, Space, Switch, Tag, Tooltip, Typography } from "antd";
import { CloudDownloadOutlined, LinkOutlined, PlusOutlined, ReloadOutlined, SearchOutlined, SyncOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import type { Skill } from "../../lib/types";
import { Group, ItemList, ItemRow, ListInput, Row, Section } from "./primitives";
import { useSettings } from "./context";

const { Text } = Typography;

/** Column order: the shared folder first, then the tools. */
const ORDER = ["shared", "claude", "codex", "opencode", "pi", "omp", "gemini"];
const LABEL: Record<string, string> = { shared: "~/.agents" };

/**
 * Settings → Skills. Each tool loads skills from its own folder; with sync
 * on, every skill on this Mac is linked into all of them (and the shared
 * `~/.agents/skills`), so one added anywhere, or pulled into a source
 * folder, reaches every tool.
 */
export function SkillsPage() {
  const { cfg, edit, dirty } = useSettings();
  const notify = useStore((s) => s.notify);
  const skills = cfg.skills ?? { sync: false, sources: [] };
  const devices = cfg.peers.map((p) => p.name);
  const [device, setDevice] = useState("");
  const [here, setHere] = useState<Skill[]>([]);
  const [there, setThere] = useState<Skill[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [source, setSource] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setHere(await backend.skillsList());
      setThere(device ? await backend.skillsList(device) : null);
    } catch (e) {
      setError(String(e));
      setThere(device ? [] : null);
    }
    setLoading(false);
  }, [device]);
  useEffect(() => { void load(); }, [load]);
  // A file or folder dropped from Finder on the Add row.
  useEffect(() => {
    const onDrop = (e: Event) => setSource((e as CustomEvent<string[]>).detail[0] ?? "");
    window.addEventListener("hubcodes:skill-files", onDrop);
    return () => window.removeEventListener("hubcodes:skill-files", onDrop);
  }, []);

  const run = async (key: string, p: Promise<string[]>, done: (lines: string[]) => string) => {
    setBusy(key);
    try {
      notify(done(await p));
      await load();
    } catch (e) {
      notify(String(e), "error");
    }
    setBusy(null);
  };
  const linked = (lines: string[]) => (lines.length ? `Linked: ${lines.slice(0, 4).join(", ")}${lines.length > 4 ? ` and ${lines.length - 4} more` : ""}` : "Every tool has every skill");

  const list = device ? there ?? [] : here;
  const tools = useMemo(() => [...new Set(list.flatMap((s) => s.providers))].sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b)), [list]);
  const needle = q.trim().toLowerCase();
  const shown = list.filter((s) => !needle || s.name.includes(needle) || s.description.toLowerCase().includes(needle));
  const incomplete = here.filter((s) => tools.some((t) => !s.providers.includes(t))).length;

  return (
    <Section
      title="Skills"
      tagline="Each coding tool loads skills from its own folder. Keep them in sync here so a skill added once works in every tool, and a tracker's /pr-review {url} works whichever agent picks it up."
      extra={<Button icon={<ReloadOutlined />} loading={loading} onClick={() => void load()}>Refresh</Button>}
    >
      <Group title="On this device">
        <Row label="Keep every tool in sync" hint="Each minute, every skill found here or in a folder below is linked into every installed tool. One real folder, so an edit or a git pull reaches all of them; your own folders are never overwritten.">
          <Switch checked={skills.sync} onChange={(v) => edit({ skills: { ...skills, sync: v } })} aria-label="Keep every tool in sync" />
        </Row>
        <Row label="Skill folders" hint="More folders of skills to link in, such as a git checkout." stack>
          <ListInput value={skills.sources ?? []} onChange={(v) => edit({ skills: { ...skills, sources: v } })} placeholder="~/.hub-skills/skills" mono label="Skill folders" />
        </Row>
        <Row label="Add a skill" hint="A SKILL.md, .zip, .skill or folder (drop it on the box), or a link to one skill or a whole repository. Add only skills you trust: they tell agents what to do." stack>
          <Space.Compact className="sk-add sk-add-zone">
            <Input value={source} onChange={(e) => setSource(e.target.value)} placeholder="Drop a file here, or paste a path or link" aria-label="Skill to add" onPressEnter={() => source.trim() && void run("add", backend.skillAdd(source.trim()), (n) => `Added ${n.join(", ")}`).then(() => setSource(""))} />
            <Button type={source.trim() ? "primary" : "default"} icon={<PlusOutlined />} disabled={!source.trim()} loading={busy === "add"} onClick={() => void run("add", backend.skillAdd(source.trim()), (n) => `Added ${n.join(", ")}`).then(() => setSource(""))}>Add</Button>
          </Space.Compact>
        </Row>
        <Row label="Now" hint={dirty ? "Save first: these use the saved settings." : incomplete ? `${incomplete} skill${incomplete === 1 ? " is" : "s are"} missing from some tool.` : "Every tool has every skill."}>
          <Space>
            <Button icon={<SyncOutlined />} disabled={dirty} loading={busy === "sync"} onClick={() => void run("sync", backend.skillsSync(), linked)}>Sync now</Button>
            <Button icon={<CloudDownloadOutlined />} disabled={dirty} loading={busy === "pull"} onClick={() => void run("pull", backend.skillsPull(), (l) => l.join(" · ") || "No git sources")}>Pull sources</Button>
          </Space>
        </Row>
      </Group>

      <Group
        title={device ? `On ${device}` : "Skills"}
        desc={device ? "Copy one to this device: it goes to ~/.agents/skills and into every tool here." : "Solid: the tool has it. Dashed: it does not."}
        extra={
          <div className="sk-tools">
            {devices.length > 0 && <Segmented size="small" value={device} options={[{ value: "", label: "This device" }, ...devices.map((d) => ({ value: d, label: d }))]} onChange={(v) => setDevice(String(v))} />}
            <Input size="small" allowClear prefix={<SearchOutlined />} placeholder="Find a skill" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Find a skill" className="sk-search" />
          </div>
        }
      >
        {error && <Text type="danger" className="sk-error">{error}</Text>}
        <ItemList empty={loading ? "Reading…" : needle ? "No skill matches." : device ? `No skills on ${device}, or it did not answer.` : "No skills yet. Add one above."}>
          {shown.map((s) => {
            const missing = tools.filter((t) => !s.providers.includes(t));
            const local = here.find((x) => x.name === s.name);
            const action = device ? (
              local ? (
                <Tooltip title={`On this device for ${local.providers.join(", ")}`}><Tag variant="filled">On this device</Tag></Tooltip>
              ) : (
                <Button size="small" icon={<CloudDownloadOutlined />} loading={busy === s.name} onClick={() => void run(s.name, backend.skillCopy(s.name, device), linked)}>Copy to this device</Button>
              )
            ) : missing.length ? (
              <Tooltip title={`Links it into ${missing.map((m) => LABEL[m] ?? m).join(", ")}`}>
                <Button size="small" icon={<LinkOutlined />} loading={busy === s.name} onClick={() => void run(s.name, backend.skillCopy(s.name), linked)}>Link to all tools</Button>
              </Tooltip>
            ) : null;
            return (
              <ItemRow
                key={s.name}
                name={<span className="mono">{s.name}</span>}
                summary={s.description || undefined}
                status={
                  <span className="sk-chips" aria-label={`Tools with ${s.name}`}>
                    {tools.map((t) => <span key={t} className={`sk-chip${s.providers.includes(t) ? " is-on" : ""}`} title={s.providers.includes(t) ? `${LABEL[t] ?? t} has it` : `${LABEL[t] ?? t} does not have it`}>{LABEL[t] ?? t}</span>)}
                  </span>
                }
                actions={action}
              />
            );
          })}
        </ItemList>
      </Group>
    </Section>
  );
}
