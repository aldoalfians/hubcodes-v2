import { Button, Descriptions, Space, Tag, Typography } from "antd";
import { FolderOpenOutlined } from "@ant-design/icons";
import { useStore } from "../../store";
import { backend, isTauri } from "../../lib/backend";
import { Group, Section } from "./primitives";
import { useMaybeSettings } from "./context";

const { Text } = Typography;

export function SessionPage() {
  const { cfg, configDir } = useMaybeSettings();
  const session = useStore((s) => s.session);
  return (
    <Section title="Session" tagline="What this app is attached to and where it writes.">
      <Group>
        <Descriptions column={1} bordered size="small" className="st-desc-list" styles={{ label: { width: 180 } }}>
          <Descriptions.Item label="tmux session"><code>{session?.session}</code></Descriptions.Item>
          <Descriptions.Item label="tmux">{session?.tmuxAvailable ? <code>{session.tmuxVersion}</code> : <Text type="danger">not found. Install with brew install tmux</Text>}</Descriptions.Item>
          <Descriptions.Item label="CLI for agents">
            {session?.cliPath ? <code>{session.cliPath}</code> : <Text type="danger">not found: agents cannot run hubcodes send</Text>}
            <br />
            <Text type="secondary">{session?.cliNote}{session && !session.cliOnPath ? " Not on your login PATH: add ~/.local/bin to PATH so agents started from a plain shell find it (agents started here always do)." : ""}</Text>
          </Descriptions.Item>
          <Descriptions.Item label="Config file">
            <Space><code>{cfg?.path}</code>{cfg && !cfg.exists && <Tag variant="filled">not written yet</Tag>}{isTauri && configDir && <Button size="small" icon={<FolderOpenOutlined />} onClick={() => void backend.openPath(configDir)}>Open folder</Button>}</Space>
          </Descriptions.Item>
          <Descriptions.Item label="From any shell"><pre className="st-pre">{"hubcodes spawn reviewer -p claude\nhubcodes send reviewer \"diff is ready\""}</pre></Descriptions.Item>
        </Descriptions>
      </Group>
    </Section>
  );
}
