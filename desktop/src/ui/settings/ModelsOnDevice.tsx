import { useEffect, useState } from "react";
import { Button, Tag, Tooltip, Typography } from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { readStatus } from "../ConnectPanel";
import { useStore } from "../../store";
import { color256 } from "../../lib/colors";
import type { ProviderStatus } from "../../lib/types";
import { Group } from "./primitives";

const { Text } = Typography;
/** Shown before the rest; long catalogs fold behind "+N more". */
const SHOWN = 12;

/**
 * Settings → Providers → Models on this device: what each installed tool
 * offers here, read from the tool itself (Claude's picker cache, Codex's
 * model cache, `opencode models`, pi and omp's own files, a provider's
 * `models_command`), with its current default marked. The same list New
 * agent, Teams and Switch offer, so what is picked is what the tool has.
 */
export function ModelsOnDevice() {
  const providers = useStore((s) => s.providers);
  const names = providers.filter((p) => p.installed && p.name !== "shell").map((p) => p.name);
  const key = names.join(",");
  const [rows, setRows] = useState<Record<string, ProviderStatus | "error">>({});
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const load = (refresh: boolean) => {
    setBusy(true);
    void Promise.all(names.map((n) => readStatus(n, refresh).then((s) => [n, s] as const).catch(() => [n, "error"] as const)))
      .then((r) => setRows(Object.fromEntries(r)))
      .finally(() => setBusy(false));
  };
  useEffect(() => { load(false); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [key]);
  return (
    <Group
      title="Models on this device"
      desc="What each installed tool offers here, read from the tool itself. New agent, Teams and Switch pick from these lists."
      extra={<Button size="small" icon={<ReloadOutlined />} loading={busy} onClick={() => load(true)}>Read again</Button>}
    >
      <div className="md-list" role="list" aria-label="Models per provider">
        {names.map((n) => {
          const p = providers.find((x) => x.name === n);
          const st = rows[n];
          const models = st && st !== "error" ? st.models : [];
          const all = open[n];
          return (
            <div className="md-row" role="listitem" key={n}>
              <div className="md-tool">
                <span style={{ color: color256(p?.color) }}>{p?.glyph}</span>
                <span className="md-name">{n}</span>
                <Text type="secondary" className="md-count">{st === undefined ? "reading…" : st === "error" ? "could not read" : `${models.length} model${models.length === 1 ? "" : "s"}`}</Text>
              </div>
              <div className="md-models">
                {models.length === 0 && st && st !== "error" && <Text type="secondary" className="md-none">{st.setup.modelsFrom || "None listed; type a model id when you start an agent."}</Text>}
                {(all ? models : models.slice(0, SHOWN)).map((m) => (
                  <Tooltip key={m.id} title={[m.label !== m.id ? m.label : "", m.note].filter(Boolean).join(" · ") || undefined}>
                    <Tag variant="filled" color={m.current ? "processing" : undefined} className="md-model mono">{m.id}{m.current ? " · default" : ""}</Tag>
                  </Tooltip>
                ))}
                {models.length > SHOWN && <Button size="small" type="link" className="md-more" onClick={() => setOpen((o) => ({ ...o, [n]: !all }))}>{all ? "Show fewer" : `+${models.length - SHOWN} more`}</Button>}
              </div>
              <Text type="secondary" className="md-src">{st && st !== "error" && st.source ? `from ${st.source}` : ""}</Text>
            </div>
          );
        })}
        {names.length === 0 && <Text type="secondary" className="md-none">No coding tool is installed on this device yet.</Text>}
      </div>
    </Group>
  );
}
