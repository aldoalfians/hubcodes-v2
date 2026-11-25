import { Table } from "antd";
import { SHORTCUTS } from "../shortcuts";
import { Group, Section } from "./primitives";

export function ShortcutsPage() {
  return (
    <Section title="Shortcuts" tagline="Keys and gestures on the canvas.">
      <Group>
        <Table size="small" pagination={false} rowKey="keys" dataSource={SHORTCUTS} showHeader={false} columns={[{ dataIndex: "label", key: "label" }, { dataIndex: "keys", key: "keys", width: 200, align: "right", render: (k: string) => <kbd>{k}</kbd> }]} />
      </Group>
    </Section>
  );
}
