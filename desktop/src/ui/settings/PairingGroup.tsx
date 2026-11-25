import { useEffect, useState, type ReactNode } from "react";
import { Button, Space, Tag, Typography } from "antd";
import { CheckOutlined, CopyOutlined, KeyOutlined, PlusOutlined, ReloadOutlined, SettingOutlined } from "@ant-design/icons";
import { backend } from "../../lib/backend";
import { useStore } from "../../store";
import type { ThisDevice } from "../../lib/types";
import { Group } from "./primitives";
import { FixActions } from "../FixActions";
import { code } from "./shared";
import { isMac } from "../../lib/platform";

const { Text } = Typography;
const SHARING = "x-apple.systempreferences:com.apple.Sharing-Settings.extension";

/**
 * Settings → Devices: pairing two Macs, step by step. Each step says what
 * to do and shows whether this Mac is already there, so nobody has to know
 * about SSH keys or Remote Login up front.
 */
export function PairingGroup({ selfName, onAdd, paired = 0 }: { selfName: string; onAdd: () => void; paired?: number }) {
  const notify = useStore((s) => s.notify);
  const [me, setMe] = useState<ThisDevice | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => backend.thisDevice().then(setMe).catch(() => setMe(null));
  useEffect(() => { void load(); }, []);
  const copy = (text: string, what: string) => {
    void navigator.clipboard.writeText(text).then(() => notify(`${what} copied`), () => notify("Could not copy", "error"));
  };
  const createKey = async () => {
    setBusy(true);
    try {
      setMe(await backend.createSshKey());
      notify("SSH key created");
    } catch (e) {
      notify(String(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const name = selfName || "mac";
  const backLine = me ? `hubcodes peer add ${name} ${me.host || "<this device's host>"} --user ${me.user}` : "";
  const [open, setOpen] = useState(paired === 0);
  const steps: { title: string; done: boolean | null; text: ReactNode; action?: ReactNode }[] = [
    {
      title: "Same tailnet",
      done: me ? me.tailscale : null,
      text: !me || me.tailscale ? "Tailscale on both devices, same account: each reaches the other by its short name." : me.tailscaleInstalled ? "Tailscale is installed but not connected. Sign in with the same account as the other device." : "Install Tailscale on both devices and sign in with one account. On one local network a .local name works too.",
      action: me && !me.tailscale ? (me.tailscaleInstalled
        ? (isMac ? <FixActions open="/Applications/Tailscale.app" openLabel="Open Tailscale" /> : <FixActions command="sudo tailscale up" />)
        : isMac ? <FixActions command="brew install --cask tailscale" open="https://tailscale.com/download/mac" openLabel="Download" /> : <FixActions command="curl -fsSL https://tailscale.com/install.sh | sh" open="https://tailscale.com/download/linux" openLabel="Download" />) : undefined,
    },
    {
      title: isMac ? "Remote Login on" : "SSH server on",
      done: me ? me.remoteLogin : null,
      text: isMac ? "System Settings → General → Sharing → Remote Login, on both devices." : <>Run {code("sudo systemctl enable --now sshd")} on both devices (Ubuntu: install {code("openssh-server")}).</>,
      action: me && !me.remoteLogin && isMac ? <Button size="small" icon={<SettingOutlined />} onClick={() => void backend.openPath(SHARING)}>Open Sharing</Button> : undefined,
    },
    {
      title: "Let this device in over there",
      done: null,
      text: me?.authorizeCommand ? <>Run the copied line once in a terminal on the other device: it adds this device's public key to its {code("~/.ssh/authorized_keys")}.</> : "This device has no SSH key yet. Create one, then copy the line.",
      action: me?.authorizeCommand
        ? <Space size={4}><Button size="small" icon={<CopyOutlined />} onClick={() => copy(me.authorizeCommand!, "Command")}>Copy command</Button><Button size="small" type="text" onClick={() => copy(me.publicKey!, "Public key")}>Key only</Button></Space>
        : <Button size="small" icon={<KeyOutlined />} loading={busy} disabled={!me} onClick={() => void createKey()}>Create key</Button>,
    },
    {
      title: "Add the other device here",
      done: paired > 0,
      text: <>The name, SSH host and user it shows in its own Settings → Devices, then <b>Test</b>.</>,
      action: <Button size="small" icon={<PlusOutlined />} onClick={onAdd}>Add device</Button>,
    },
    {
      title: "Add this device over there",
      done: null,
      text: me ? <>On the other device add {code(name)}, host {code(me.host || "?")}, user {code(me.user)}, or run:</> : "Checking…",
      action: me ? <Text className="mono pair-line" copyable={{ text: backLine, tooltips: ["Copy", "Copied"] }}>{backLine}</Text> : undefined,
    },
  ];
  const doneCount = steps.filter((s) => s.done).length;
  return (
    <Group
      title="Pair with another device"
      desc={open ? "Once on each device. Agents travel over SSH, so each device lets the other in without a password." : `Paired with ${paired} device${paired === 1 ? "" : "s"}. The steps are here for the next one.`}
      extra={open ? <Button size="small" type="text" icon={<ReloadOutlined />} onClick={() => void load()}>Check again</Button> : <Button size="small" onClick={() => setOpen(true)}>Pair another device</Button>}
    >
      {open && (
        <ol className="pair-steps" aria-label={`Pairing steps, ${doneCount} done`}>
          {steps.map((s, i) => (
            <li key={s.title} className={`pair-step${s.done ? " is-done" : s.done === false ? " is-todo" : ""}`}>
              <span className="pair-n" aria-hidden>{s.done ? <CheckOutlined /> : i + 1}</span>
              <div className="pair-body">
                <div className="pair-title">{s.title}{s.done === false && <Tag variant="filled" color="warning">to do</Tag>}</div>
                <Text type="secondary" className="pair-text">{s.text}</Text>
              </div>
              {s.action && !s.done && <div className="pair-action">{s.action}</div>}
            </li>
          ))}
        </ol>
      )}
    </Group>
  );
}
