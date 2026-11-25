import { Button, Space } from "antd";
import { CopyOutlined, ExportOutlined } from "@ant-design/icons";
import { backend } from "../lib/backend";
import { useStore } from "../store";

/** Copy the command that fixes a setup problem, or open the page / app that does. */
export function FixActions({ command, open, openLabel }: { command?: string; open?: string; openLabel?: string }) {
  const notify = useStore((s) => s.notify);
  if (!command && !open) return null;
  const label = openLabel ?? (open?.startsWith("http") ? `Open ${new URL(open).host}` : `Open ${open?.split("/").pop()?.replace(/\.app$/, "") ?? ""}`);
  return (
    <Space size={6} wrap className="fix-actions">
      {command && (
        <Button size="small" icon={<CopyOutlined />} onClick={() => void navigator.clipboard.writeText(command).then(() => notify(`Copied: ${command}. Paste it in Terminal.`), () => notify("Could not copy", "error"))}>
          Copy <code>{command}</code>
        </Button>
      )}
      {open && <Button size="small" icon={<ExportOutlined />} onClick={() => void backend.openPath(open).catch((e) => notify(String(e), "error"))}>{label}</Button>}
    </Space>
  );
}
