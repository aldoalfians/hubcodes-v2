import { Button, Typography } from "antd";
import { PlusOutlined } from "@ant-design/icons";
import { useStore } from "../store";
import { FixActions } from "./FixActions";

const { Text, Title, Paragraph } = Typography;

export function EmptyState() {
  const setDialog = useStore((s) => s.setDialog);
  const session = useStore((s) => s.session);
  const noTmux = session && !session.tmuxAvailable;
  const tmuxCheck = useStore((s) => s.doctor?.checks.find((c) => c.id === "tmux" && c.level === "fail"));
  return (
    <div className="empty">
      <div className="empty-card">
        {noTmux ? (
          <>
            <Title level={5}>tmux is not installed</Title>
            <Paragraph>hubcodes runs every agent inside tmux so they survive restarts and can be shared. {tmuxCheck?.fix ?? "Run brew install tmux in Terminal."}</Paragraph>
            <FixActions command={tmuxCheck?.command ?? "brew install tmux"} open={tmuxCheck?.open} />
            <div style={{ marginTop: 12 }}><Button onClick={() => window.location.reload()}>Check again</Button></div>
            <Text type="secondary">hubcodes finds tmux in Homebrew's folders on its own; no PATH changes needed.</Text>
          </>
        ) : (
          <>
            <Title level={5}>No agents yet</Title>
            <Paragraph>Start one here, or from any terminal, and it appears on this canvas.</Paragraph>
            <Button type="primary" icon={<PlusOutlined />} onClick={() => setDialog("new-agent")}>New agent <kbd>⌘N</kbd></Button>
            <pre>hubcodes spawn builder -p claude --cwd ~/code/app</pre>
          </>
        )}
      </div>
    </div>
  );
}
