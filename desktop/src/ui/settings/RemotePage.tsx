import { Button, Input, InputNumber, Popconfirm, Segmented, Select, Typography } from "antd";
import { CopyOutlined, DeleteOutlined, PlusOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend } from "../../lib/backend";
import { AuditGroup } from "./ThisMac";
import { Group, Row, Section } from "./primitives";
import { code } from "./shared";
import { useSettings } from "./context";

const { Text } = Typography;

const BIND_HINT: Record<string, string> = {
  tailscale: "This device's Tailscale address: only your tailnet can reach it.",
  local: "127.0.0.1: only this device. Good for trying it out.",
  custom: "Any address of this device, such as a LAN IP. Anyone who can reach it can try the page, so set a token.",
};

export function RemotePage() {
  const { cfg, editServe, editShare, tsIp } = useSettings();
  const notify = useStore((s) => s.notify);
  const serving = useStore((s) => s.serving);
  const startServe = async () => {
    try {
      const addr = await backend.serveStart();
      notify(`Serving at http://${addr}/`);
    } catch (e) {
      notify(String(e), "error");
    }
  };
  return (
    <Section
      title="Remote access"
      tagline="A small web page for your phone and your teammates."
      help={<>Who needs you, approve or deny, send a message, read the notes and the activity, and the canvas itself. It listens on this device's Tailscale address, so only your tailnet can reach it. Teammates open {code(`http://${tsIp ?? "<tailscale-ip>"}:${cfg.serve.port}/`)}; hubcodes asks Tailscale who they are. From a terminal: {code("hubcodes serve --web")}, or {code("hubcodes daemon run --serve")} to serve it from the daemon.</>}
    >
      <div className={`srv-status${serving ? " is-on" : ""}`}>
        <span className="srv-dot" aria-hidden />
        <div className="srv-text">
          <div className="srv-title">{serving ? "Running" : "Not running"}</div>
          <div className="srv-sub">
            {serving ? <>Open <span className="mono">http://{serving}/</span> on your phone; the canvas is at <span className="mono">/canvas/</span>.</>
              : tsIp ? <>This device is <span className="mono">{tsIp}</span> on your tailnet.</>
              : "Tailscale is not running here. Choose This device only to try it locally."}
          </div>
        </div>
        {serving && <Button icon={<CopyOutlined />} onClick={() => { void navigator.clipboard?.writeText(`http://${serving}/`); notify("Link copied"); }}>Copy link</Button>}
        <Button type="primary" onClick={() => void startServe()} disabled={!!serving}>{serving ? "Running" : "Start server"}</Button>
      </div>
      <Group title="Server">
        <Row label="Listen on" hint={BIND_HINT[(cfg.serve.bind === "tailscale" || !cfg.serve.bind ? "tailscale" : cfg.serve.bind === "127.0.0.1" ? "local" : "custom")]}>
          <Segmented value={(cfg.serve.bind === "tailscale" || !cfg.serve.bind ? "tailscale" : cfg.serve.bind === "127.0.0.1" ? "local" : "custom")} options={[{ value: "tailscale", label: "Tailnet" }, { value: "local", label: "This device only" }, { value: "custom", label: "Custom address" }]} onChange={(v) => editServe({ bind: v === "tailscale" ? "tailscale" : v === "local" ? "127.0.0.1" : ((cfg.serve.bind === "tailscale" || !cfg.serve.bind ? "tailscale" : cfg.serve.bind === "127.0.0.1" ? "local" : "custom") === "custom" ? cfg.serve.bind : "0.0.0.0") })} />
          {(cfg.serve.bind === "tailscale" || !cfg.serve.bind ? "tailscale" : cfg.serve.bind === "127.0.0.1" ? "local" : "custom") === "custom" && <Input className="mono st-narrow" value={cfg.serve.bind} onChange={(e) => editServe({ bind: e.target.value })} spellCheck={false} placeholder="192.168.1.20" aria-label="Address" />}
        </Row>
        <Row label="Port"><InputNumber min={1} max={65535} value={cfg.serve.port} onChange={(n) => editServe({ port: Number(n) || 7433 })} className="st-num" /></Row>
        <Row label="Access token" hint="Optional. Lets people outside your tailnet in when the link ends with ?token=…; they count as operators. Empty: your tailnet only.">
          <Input.Password className="mono" value={cfg.serve.token} onChange={(e) => editServe({ token: e.target.value })} placeholder="No token" />
        </Row>
      </Group>
      <Group title="Who may open it" desc="People on your tailnet, by login, or * for everyone. You are always the owner from this device." extra={<Button size="small" icon={<PlusOutlined />} onClick={() => editServe({ shares: [...(cfg.serve.shares ?? []), { user: "", role: "viewer" }] })}>Share with someone</Button>}>
        {(cfg.serve.shares ?? []).length === 0 ? (
          <div className="share-empty"><Text type="secondary">Only you, from this device.</Text></div>
        ) : (
          <div className="share-list">
            {cfg.serve.shares.map((sh, i) => (
              <div className="share-row" key={i}>
                <Input className="mono" value={sh.user} onChange={(e) => editShare(i, { user: e.target.value })} placeholder="name@example.com or *" spellCheck={false} aria-label="Tailnet login" />
                <Select className="share-role" value={sh.role} onChange={(v) => editShare(i, { role: v })} aria-label="May" options={[{ value: "viewer", label: "Viewer · sees everything" }, { value: "contributor", label: "Contributor · sends, you approve" }, { value: "operator", label: "Operator · sends, approves, denies" }]} />
                <Popconfirm title={`Stop sharing with ${sh.user || "this person"}?`} okText="Remove" okButtonProps={{ danger: true }} onConfirm={() => editServe({ shares: cfg.serve.shares.filter((_, j) => j !== i) })}>
                  <Button type="text" className="share-x" icon={<DeleteOutlined />} aria-label="Remove" />
                </Popconfirm>
              </div>
            ))}
          </div>
        )}
      </Group>
      <AuditGroup />
    </Section>
  );
}
