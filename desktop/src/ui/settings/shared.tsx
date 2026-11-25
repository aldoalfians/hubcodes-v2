import { InputNumber } from "antd";
import { thousands, unthousands } from "./primitives";

/** One item being edited in a drawer: `index` null means a new one. */
export type Editing<T> = { index: number | null; draft: T } | null;
export type Opt = { value: string; label: string };

export const kindLabel = (kinds: { value: string; label: string }[], v: string) => kinds.find((k) => k.value === v)?.label ?? v;
export const hostOf = (url: string) => { try { return new URL(url).host; } catch { return url; } };
export const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
export const code = (s: string) => <code>{s}</code>;

/** A number with its unit next to it, not inside the box: `[ 8 ] agents`. */
export function Amount({ value, onChange, unit, min = 0, max, step = 1, label, placeholder, thousandsSep }: { value: number | null; onChange: (v: number) => void; unit: string; min?: number; max?: number; step?: number; label?: string; placeholder?: string; thousandsSep?: boolean }) {
  return (
    <span className="st-amount">
      <InputNumber
        min={min}
        max={max}
        step={step}
        value={value}
        placeholder={placeholder}
        onChange={(n) => onChange(Math.max(min, Number(n) || 0))}
        className="st-amount-n"
        aria-label={label}
        formatter={thousandsSep ? thousands : undefined}
        parser={thousandsSep ? unthousands : undefined}
      />
      <span className="st-amount-unit">{unit}</span>
    </span>
  );
}

/** A number where empty (0) means "no limit" or "off". */
export function OptionalNumber({ value, onChange, unit, empty, step = 1, thousandsSep, label }: { value: number; onChange: (v: number) => void; unit: string; empty: string; step?: number; thousandsSep?: boolean; label?: string }) {
  return <Amount value={value > 0 ? value : null} onChange={onChange} unit={unit} placeholder={empty} step={step} thousandsSep={thousandsSep} label={label} />;
}
