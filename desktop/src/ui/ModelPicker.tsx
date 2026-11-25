import { useMemo, useState } from "react";
import { Select, Tag, Typography } from "antd";
import type { ModelInfo } from "../lib/types";

const { Text } = Typography;

/**
 * The provider's real model list as a searchable select: label and id on one
 * line, the provider's current default marked, the note on hover. Any id can
 * still be typed (catalogs lag behind what a CLI accepts): it is offered as
 * Use “…”. Empty means the provider's own default.
 */
export function ModelPicker({ value, onChange, models, disabled, placeholder, loading, size, compact = false }: {
  value: string;
  onChange: (v: string) => void;
  models: ModelInfo[];
  disabled?: boolean;
  placeholder?: string;
  loading?: boolean;
  size?: "small" | "middle";
  /** No caption line under the field (table cells). */
  compact?: boolean;
}) {
  const [typed, setTyped] = useState("");
  const options = useMemo(() => {
    const list = models.map((m) => ({
      value: m.id,
      search: `${m.id} ${m.label}`.toLowerCase(),
      label: (
        <span className="mp-opt" title={m.note || undefined}>
          <span className="mp-label">{m.label}</span>
          {m.current && <Tag variant="filled" color="warning" className="mp-default">default</Tag>}
          {m.label !== m.id && <span className="mp-id">{m.id}</span>}
        </span>
      ),
    }));
    const t = typed.trim();
    const v = value.trim();
    if (v && !models.some((m) => m.id === v)) list.unshift({ value: v, search: v.toLowerCase(), label: <span className="mp-opt"><span className="mp-label mono">{v}</span><span className="mp-id">not in the list</span></span> });
    if (t && !list.some((o) => o.value === t)) list.unshift({ value: t, search: t.toLowerCase(), label: <span className="mp-opt"><span className="mp-label">Use “{t}”</span></span> });
    return list;
  }, [models, typed, value]);
  const chosen = models.find((m) => m.id === value.trim());
  return (
    <div className={`model-picker${compact ? " is-compact" : ""}`}>
      <Select
        showSearch
        allowClear
        size={size}
        value={value.trim() || undefined}
        onChange={(v) => { setTyped(""); onChange(String(v ?? "")); }}
        onSearch={setTyped}
        filterOption={(input, o) => !input || (o?.search ?? "").includes(input.toLowerCase()) || o?.value === input.trim()}
        options={options}
        labelRender={(o) => { const id = String(o.value ?? ""); const m = models.find((x) => x.id === id); return <span className={m ? undefined : "mono"}>{m ? m.label : id}</span>; }}
        disabled={disabled}
        loading={loading}
        placeholder={placeholder}
        notFoundContent={loading && models.length === 0 ? "Reading the provider's model list…" : "Type a model id to use it"}
        popupMatchSelectWidth={false}
        className="mp-select"
        aria-label="Model"
      />
      {chosen && !compact && chosen.note && <Text type="secondary" className="model-chosen">{chosen.note}</Text>}
    </div>
  );
}
