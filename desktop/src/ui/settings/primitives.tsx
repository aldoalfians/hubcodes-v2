import { useEffect, useState, type ReactNode } from "react";
import { AutoComplete, Button, Input, Modal, Popconfirm, Popover, Space, Switch, Tag, Typography } from "antd";
import { CheckCircleFilled, CloseCircleFilled, DeleteOutlined, InfoCircleOutlined, LoadingOutlined, RightOutlined } from "@ant-design/icons";

const { Text } = Typography;

/** Page: title, one-line tagline, optional long explanation behind an info popover, actions on the right. */
export function Section({ title, tagline, help, extra, children }: { title: string; tagline?: ReactNode; help?: ReactNode; extra?: ReactNode; children: ReactNode }) {
  return (
    <section className="st-section" aria-label={title}>
      <header className="st-head">
        <div className="st-head-text">
          <h3>
            {title}
            {help && (
              <Popover placement="bottomLeft" trigger="click" overlayClassName="st-help-pop" content={<div className="st-help-body">{help}</div>}>
                <button type="button" className="st-help" aria-label={`How ${title} works`}><InfoCircleOutlined /></button>
              </Popover>
            )}
          </h3>
          {tagline && <Text type="secondary" className="st-tagline">{tagline}</Text>}
        </div>
        {extra && <div className="st-extra">{extra}</div>}
      </header>
      {children}
    </section>
  );
}

/**
 * A bordered block of related settings; rows inside are divided by hairlines.
 * With `toggle` the block is a feature with an on/off switch in its header:
 * off, its settings fold away and `off` says what happens instead.
 */
export function Group({ title, desc, extra, children, className, toggle, off }: { title?: ReactNode; desc?: ReactNode; extra?: ReactNode; children?: ReactNode; className?: string; toggle?: { checked: boolean; onChange: (v: boolean) => void; label: string }; off?: ReactNode }) {
  const folded = !!toggle && !toggle.checked;
  return (
    <div className={`st-group${className ? ` ${className}` : ""}${toggle ? " is-feature" : ""}${folded ? " is-off" : ""}`}>
      {(title || extra || toggle) && (
        <div className="st-group-head">
          <div>
            {title && <div className="st-group-title">{title}</div>}
            {desc && <Text type="secondary" className="st-group-desc">{desc}</Text>}
            {folded && off && <Text type="secondary" className="st-group-off">{off}</Text>}
          </div>
          {(extra || toggle) && (
            <div className="st-group-extra">
              {!folded && extra}
              {toggle && <Switch checked={toggle.checked} onChange={toggle.onChange} aria-label={toggle.label} />}
            </div>
          )}
        </div>
      )}
      {!folded && children && <div className="st-group-body">{children}</div>}
    </div>
  );
}

/** One setting: label and help on the left, the control on the right. `stack` puts the control under the label (wide inputs). */
export function Row({ label, hint, children, stack, wide }: { label: ReactNode; hint?: ReactNode; children: ReactNode; stack?: boolean; wide?: boolean }) {
  return (
    <div className={`st-row${stack ? " is-stacked" : ""}${wide ? " is-wide" : ""}`}>
      <div className="st-row-info">
        <span className="st-row-label">{label}</span>
        {hint && <Text type="secondary" className="st-row-hint">{hint}</Text>}
      </div>
      <div className="st-row-control">{children}</div>
    </div>
  );
}

/**
 * An editable list of short values (roles, rules, paths, tool names): each
 * value is a removable chip, and the box underneath adds one, with the values
 * not yet in the list offered as suggestions. Pasting "a, b" or several lines
 * adds them all.
 */
export function ListInput({ value, onChange, suggestions = [], placeholder, mono, label, emptyText, disabled, separators = /[\n,]/ }: { value: string[]; onChange: (v: string[]) => void; suggestions?: string[]; placeholder?: string; mono?: boolean; label: string; emptyText?: ReactNode; disabled?: boolean; separators?: RegExp }) {
  const [text, setText] = useState("");
  const items = value ?? [];
  const add = (raw: string) => {
    const fresh = raw.split(separators).map((x) => x.trim()).filter((x) => x && !items.includes(x));
    if (fresh.length) onChange([...items, ...Array.from(new Set(fresh))]);
    setText("");
  };
  const q = text.trim().toLowerCase();
  const options = suggestions.filter((x) => !items.includes(x) && (!q || x.toLowerCase().includes(q))).slice(0, 50).map((x) => ({ value: x, label: <span className={mono ? "mono" : undefined}>{x}</span> }));
  return (
    <div className={`st-list-input${disabled ? " is-disabled" : ""}`} onClick={(e) => (e.currentTarget.querySelector("input") as HTMLInputElement | null)?.focus()}>
      <div className="st-chips" role="list" aria-label={label}>
        {items.map((x) => (
          <Tag key={x} role="listitem" className={`st-chip${mono ? " mono" : ""}`} closable={!disabled} onClose={(e) => { e.preventDefault(); onChange(items.filter((y) => y !== x)); }} closeIcon={<span aria-label={`Remove ${x}`}>×</span>}>{x}</Tag>
        ))}
        <AutoComplete
          value={text}
          options={options}
          disabled={disabled}
          variant="borderless"
          onChange={(v) => {
            const s = String(v ?? "");
            if (separators.test(s)) add(s); else setText(s);
          }}
          onSelect={(v) => add(String(v))}
          className={`st-chips-input${mono ? " mono" : ""}`}
          popupMatchSelectWidth={false}
          placeholder={items.length === 0 ? `${emptyText ?? "None yet."} Type to add, e.g. ${placeholder ?? ""}` : "Add…"}
          aria-label={`Add to ${label}`}
          onKeyDown={(e) => { if (e.key === "Enter" && text.trim() && !options.some((o) => o.value === text.trim())) { e.preventDefault(); add(text); } }}
          onBlur={() => text.trim() && add(text)}
        />
      </div>
    </div>
  );
}

/** Form field for drawers: label above, help below. `span` makes it take the full width of a two-column grid. */
export function Field({ label, hint, children, span, required }: { label: ReactNode; hint?: ReactNode; children: ReactNode; span?: boolean; required?: boolean }) {
  return (
    <div className={`st-field${span ? " is-span" : ""}`}>
      <span className="st-field-label">{label}{required && <span className="st-req" aria-hidden> *</span>}</span>
      <span className="st-field-control">{children}</span>
      {hint && <Text type="secondary" className="st-field-hint">{hint}</Text>}
    </div>
  );
}

/** Two-column grid of fields. */
export function Fields({ children, cols = 2 }: { children: ReactNode; cols?: 1 | 2 }) {
  return <div className={`st-fields cols-${cols}`}>{children}</div>;
}

/** Sub-heading inside a drawer form. */
export function FormGroup({ title, children, extra }: { title: ReactNode; children: ReactNode; extra?: ReactNode }) {
  return (
    <div className="st-form-group">
      <div className="st-form-group-head"><span>{title}</span>{extra}</div>
      {children}
    </div>
  );
}

/** A list of saved items (teams, sources, trackers…), each one row; editing happens in a drawer. */
/** A list of items. When empty it only says so: the section's own Add button is the one way to add (`onAdd` / `addLabel` are kept for callers, unused). */
export function ItemList({ children, empty }: { children: ReactNode; empty?: ReactNode; onAdd?: () => void; addLabel?: string }) {
  const hasItems = Array.isArray(children) ? children.some(Boolean) : !!children;
  return (
    <div className="st-list" role="list">
      {hasItems ? children : (
        <div className="st-empty">
          <Text type="secondary">{empty ?? "Nothing here yet."}</Text>
        </div>
      )}
    </div>
  );
}

export function ItemRow({
  icon,
  name,
  meta,
  summary,
  status,
  enabled,
  onEnable,
  actions,
  onOpen,
  onRemove,
  removeTitle,
  off,
}: {
  icon?: ReactNode;
  name: ReactNode;
  meta?: ReactNode;
  summary?: ReactNode;
  status?: ReactNode;
  enabled?: boolean;
  onEnable?: (v: boolean) => void;
  actions?: ReactNode;
  onOpen?: () => void;
  onRemove?: () => void;
  removeTitle?: string;
  off?: boolean;
}) {
  return (
    <div className={`st-item${off ? " is-off" : ""}`} role="listitem">
      {icon && <span className="st-item-icon">{icon}</span>}
      <button type="button" className="st-item-main" onClick={onOpen} disabled={!onOpen}>
        <span className="st-item-head">
          <span className="st-item-name">{name}</span>
          {meta}
        </span>
        {summary && <span className="st-item-sum">{summary}</span>}
      </button>
      {status && <span className="st-item-status">{status}</span>}
      <span className="st-item-actions" onClick={(e) => e.stopPropagation()}>
        {onEnable && <Switch size="small" checked={!!enabled} onChange={onEnable} aria-label="Enabled" />}
        {actions}
        {onRemove && (
          <Popconfirm title={removeTitle ?? "Remove this?"} okText="Remove" okButtonProps={{ danger: true }} onConfirm={onRemove}>
            <Button size="small" type="text" danger icon={<DeleteOutlined />} aria-label="Remove" />
          </Popconfirm>
        )}
        {onOpen && <Button size="small" type="text" icon={<RightOutlined />} onClick={onOpen} aria-label="Edit" />}
      </span>
    </div>
  );
}

/** Editor modal with a consistent footer: optional left-side extra (Test…), Cancel, primary. The body scrolls; header and footer stay. */
export function EditModal({ title, open, onClose, onApply, applyLabel = "Apply", extra, width = 640, children }: { title: ReactNode; open: boolean; onClose: () => void; onApply: () => void; applyLabel?: string; extra?: ReactNode; width?: number; children: ReactNode }) {
  return (
    <Modal
      title={title}
      open={open}
      width={Math.max(width, 560)}
      onCancel={onClose}
      destroyOnHidden
      centered
      mask={{ closable: false }}
      className="st-modal"
      footer={
        <div className="st-modal-foot">
          <span className="st-modal-extra">{extra}</span>
          <Space>
            <Button onClick={onClose}>Cancel</Button>
            <Button type="primary" onClick={onApply}>{applyLabel}</Button>
          </Space>
        </div>
      }
    >
      {children}
    </Modal>
  );
}

export type Check = { ok: boolean; text: string; busy?: boolean };

export function Result({ r }: { r: Check | null | undefined }) {
  if (!r) return null;
  return (
    <Text type={r.busy ? "secondary" : r.ok ? "success" : "danger"} className="st-result">
      {r.busy ? <LoadingOutlined /> : r.ok ? <CheckCircleFilled /> : <CloseCircleFilled />} {r.text}
    </Text>
  );
}

// The backend leaves empty MCP fields out of the JSON (they are omitted from
// hubcodes.toml too), so headers, url and command may be missing.
export const headersText = (h: Record<string, string> | undefined | null) => Object.entries(h ?? {}).map(([k, v]) => `${k}: ${v}`).join("\n");
export const parseHeaders = (text: string) => Object.fromEntries(text.split("\n").map((l) => l.split(/:\s*/, 2) as [string, string]).filter((kv) => kv[0] && kv[1]));
export const thousands = (v: string | number | undefined) => `${v ?? ""}`.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
export const unthousands = (v: string | undefined) => Number((v ?? "").replace(/,/g, ""));

export function JsonInput({ value, fallback, onChange, label }: { value: unknown; fallback: unknown; onChange: (v: unknown) => void; label: string }) {
  const [text, setText] = useState(JSON.stringify(value ?? fallback));
  const [bad, setBad] = useState(false);
  const canonical = JSON.stringify(value ?? fallback);
  useEffect(() => { setText(canonical); setBad(false); }, [canonical]);
  return (
    <Input
      className="mono"
      value={text}
      status={bad ? "error" : undefined}
      aria-label={label}
      onChange={(e) => {
        setText(e.target.value);
        try { onChange(JSON.parse(e.target.value)); setBad(false); } catch { setBad(true); }
      }}
    />
  );
}

/** Tool name + JSON arguments, side by side. */
export function ToolArgs({ tool, args, fallback, placeholder, onTool, onArgs, label }: { tool: string; args: unknown; fallback: unknown; placeholder: string; onTool: (v: string) => void; onArgs: (v: unknown) => void; label: string }) {
  return (
    <Space.Compact className="st-wide">
      <Input className="mono st-tool" value={tool} onChange={(e) => onTool(e.target.value)} placeholder={placeholder} spellCheck={false} aria-label={label} />
      <JsonInput value={args} fallback={fallback} onChange={onArgs} label={`${label} arguments`} />
    </Space.Compact>
  );
}

/**
 * Pick one of a few ways to do something, each with a line on what it means:
 * larger than a segmented control, so the choice explains itself.
 */
export function ChoiceCards<T extends string>({ value, onChange, options, label }: { value: T; onChange: (v: T) => void; options: { value: T; title: ReactNode; desc: ReactNode }[]; label: string }) {
  return (
    <div className="st-cards" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          type="button"
          key={o.value}
          role="radio"
          aria-checked={o.value === value}
          className={`st-card${o.value === value ? " is-on" : ""}`}
          onClick={() => onChange(o.value)}
        >
          <span className="st-card-dot" aria-hidden />
          <span className="st-card-title">{o.title}</span>
          <span className="st-card-desc">{o.desc}</span>
        </button>
      ))}
    </div>
  );
}

/** An on/off setting inside a form: label and help on the left, the switch on the right. */
/**
 * A quiet note above a list: one line of plain text and, when there is
 * something to do about it, one action. No tinted panel, no icon.
 */
export function Notice({ children, action, tone }: { children: ReactNode; action?: ReactNode; tone?: "warn" }) {
  return (
    <div className={`st-notice${tone ? ` is-${tone}` : ""}`} role="status">
      <span className="st-notice-text">{children}</span>
      {action && <span className="st-notice-action">{action}</span>}
    </div>
  );
}

export function ToggleField({ label, hint, checked, onChange }: { label: ReactNode; hint?: ReactNode; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="st-toggle-field">
      <span className="st-toggle-text">
        <span className="st-field-label">{label}</span>
        {hint && <Text type="secondary" className="st-field-hint">{hint}</Text>}
      </span>
      <Switch checked={checked} onChange={onChange} aria-label={typeof label === "string" ? label : undefined} />
    </div>
  );
}
