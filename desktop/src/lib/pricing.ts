/**
 * Rough list prices per million tokens, for the cost page. Estimates: the
 * providers' own dashboards are the source of truth. Unknown models fall
 * back to the family default so the page still shows a number.
 */
export interface Price {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}

const USD = (input: number, output: number, cacheRead = input / 10, cacheWrite = input * 1.25): Price => ({ input, output, cacheRead, cacheWrite });

const TABLE: [RegExp, Price][] = [
  [/opus/i, USD(15, 75)],
  [/sonnet/i, USD(3, 15)],
  [/haiku/i, USD(0.8, 4)],
  [/gpt-5.*(codex|pro)/i, USD(2.5, 10)],
  [/gpt-5.*mini/i, USD(0.25, 2)],
  [/gpt-5/i, USD(1.25, 10)],
  [/o3|o4/i, USD(2, 8)],
  [/gemini.*pro/i, USD(1.25, 10)],
  [/gemini.*flash/i, USD(0.1, 0.4)],
  [/grok/i, USD(3, 15)],
  [/deepseek/i, USD(0.27, 1.1)],
  [/qwen|llama|mistral|kimi|glm/i, USD(0.5, 1.5)],
];

export function priceFor(model: string): Price | null {
  const m = model.trim();
  if (!m) return null;
  for (const [re, p] of TABLE) if (re.test(m)) return p;
  return null;
}

/** Dollars for one usage row; null when the model is unknown. */
export function costOf(model: string, input: number, output: number, cacheRead: number, cacheWrite: number): number | null {
  const p = priceFor(model);
  if (!p) return null;
  return (input * p.input + output * p.output + cacheRead * p.cacheRead + cacheWrite * p.cacheWrite) / 1_000_000;
}

export const fmtUsd = (n: number) => (n < 0.01 && n > 0 ? "<$0.01" : `$${n.toFixed(n >= 100 ? 0 : 2)}`);
export const fmtTok = (n: number) => (n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${Math.round(n / 1000)}k` : String(n));
