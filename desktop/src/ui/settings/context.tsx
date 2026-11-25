import { createContext, useContext, type Dispatch, type SetStateAction } from "react";
import type { Config, MemoryConfig, Share } from "../../lib/types";
import type { Opt } from "./shared";

export type ListKey = "connections" | "kbs" | "trackers" | "templates" | "playbooks" | "prompts" | "triggers" | "peers" | "providers";

/** What the settings pages share: the config being edited and the ways to change it. */
export type Settings = {
  cfg: Config | null;
  setCfg: Dispatch<SetStateAction<Config | null>>;
  dirty: boolean;
  edit: (patch: Partial<Config>) => void;
  editMemory: (patch: Partial<MemoryConfig>) => void;
  editServe: (patch: Partial<Config["serve"]>) => void;
  editPolicy: (patch: Partial<Config["policy"]>) => void;
  editShare: (i: number, patch: Partial<Share>) => void;
  setListItem: <K extends ListKey>(key: K, i: number | null, item: Config[K][number]) => void;
  removeListItem: (key: ListKey, i: number) => void;
  nameTaken: (list: { name: string }[], name: string, index: number | null) => boolean;
  providerOptions: Opt[];
  roleOptions: Opt[];
  configDir: string;
  tsIp: string | null;
};

const SettingsContext = createContext<Settings | null>(null);
export const SettingsProvider = SettingsContext.Provider;

/** For pages that may render before the config has loaded (Session). */
export function useMaybeSettings(): Settings {
  const s = useContext(SettingsContext);
  if (!s) throw new Error("useSettings outside SettingsDialog");
  return s;
}

/** For pages that render only once the config has loaded. */
export function useSettings(): Settings & { cfg: Config } {
  const s = useMaybeSettings();
  if (!s.cfg) throw new Error("useSettings before the config loaded");
  return s as Settings & { cfg: Config };
}
