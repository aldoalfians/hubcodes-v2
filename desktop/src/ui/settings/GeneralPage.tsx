import { useState } from "react";
import { Button, Input, Space, Switch } from "antd";
import { BellOutlined } from "@ant-design/icons";
import { bellEnabled, chime, setBell, useStore } from "../../store";
import { backend, isTauri } from "../../lib/backend";
import { THEMES, applyTheme, useTheme } from "../../lib/theme";
import { BackgroundGroup, UpdatesGroup } from "./ThisMac";
import { Group, ListInput, Row, Section } from "./primitives";
import { useSettings } from "./context";

const ROLE_SUGGESTIONS = ["orchestrator", "software engineer", "frontend", "backend", "reviewer", "pr-fixer", "tests", "qa", "docs", "research", "designer", "devops", "security", "product manager"];

export function GeneralPage() {
  const { cfg, edit } = useSettings();
  const session = useStore((s) => s.session);
  const notify = useStore((s) => s.notify);
  const [bell, setBellState] = useState(bellEnabled());
  return (
    <Section title="General" tagline="How this window behaves and where new agents begin.">
      <Group>
        <Row label="Notify me when an agent needs me" hint="Only while this window is in the background. The Dock badge counts them either way.">
          <Space>
            <Switch checked={cfg.notifications} onChange={(v) => edit({ notifications: v })} aria-label="Notify me when an agent needs me" />
            {isTauri && <Button size="small" icon={<BellOutlined />} onClick={() => backend.testNotification().catch((e) => notify(String(e), "error"))}>Send a test</Button>}
          </Space>
        </Row>
        <Row label="Play a sound when an agent needs me" hint="Two short tones, on this device only. Works with the window in front too.">
          <Space>
            <Switch checked={bell} onChange={(v) => { setBell(v); setBellState(v); }} aria-label="Play a sound when an agent needs me" />
            <Button size="small" onClick={chime}>Preview</Button>
          </Space>
        </Row>
        <Row label="New agents start in" hint="Used when the folder in New agent is left empty. ~ works.">
          <Input className="mono" value={cfg.defaultFolder ?? ""} onChange={(e) => edit({ defaultFolder: e.target.value || null })} placeholder={session?.home ? `${session.home} (home)` : "home folder"} spellCheck={false} />
        </Row>
        <Row label="Theme" hint="Backdrop, panels, accent, status colours and terminals, on this device. Most follow a Neovim colorscheme." stack>
          <ThemePicker />
        </Row>
        <Row label="Roles" hint="Suggested in New agent and on the role chip; any other role can still be typed. Providers with a role flag are told their role at start." stack>
          <ListInput label="Roles" value={cfg.roles} onChange={(v) => edit({ roles: v })} suggestions={ROLE_SUGGESTIONS} placeholder="Add a role, e.g. designer" emptyText="No roles yet. Agents can still be given any role." />
        </Row>
      </Group>
      {isTauri && <BackgroundGroup />}
      {isTauri && <UpdatesGroup />}
    </Section>
  );
}

/** Theme cards: a swatch of the backdrop, the name, one line on the mood. */
function ThemePicker() {
  const theme = useTheme();
  return (
    <div className="theme-grid" role="radiogroup" aria-label="Theme">
      {THEMES.map((t) => (
        <button
          key={t.id}
          type="button"
          role="radio"
          aria-checked={theme.id === t.id}
          className={`theme-card${theme.id === t.id ? " is-on" : ""}`}
          onClick={() => applyTheme(t.id)}
        >
          <span className="theme-swatch" style={{ background: `radial-gradient(70% 90% at 100% 110%, ${t.p.glowA}66, transparent 70%), radial-gradient(60% 70% at 0% 110%, ${t.p.glowB}44, transparent 70%), ${t.p.bg}` }}>
            <span className="theme-chips">
              <i style={{ background: t.p.primary }} />
              <i style={{ background: t.p.green }} />
              <i style={{ background: t.p.yellow }} />
              <i style={{ background: t.p.red }} />
            </span>
          </span>
          <span className="theme-name">{t.name}</span>
          <span className="theme-note">{t.source}</span>
        </button>
      ))}
    </div>
  );
}
