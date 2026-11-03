export type ThemeMode = "system" | "dark" | "light";

export const THEME_STORAGE_KEY = "agenthub.theme";

export const APPEARANCE_OPTIONS: ReadonlyArray<{ id: ThemeMode; label: string }> = [
  { id: "system", label: "跟随系统" },
  { id: "dark", label: "深色" },
  { id: "light", label: "浅色" }
];

export function normalizeThemeMode(value: unknown): ThemeMode {
  return value === "dark" || value === "light" ? value : "system";
}

export function readStoredThemeMode(
  storage: Pick<Storage, "getItem"> | null = typeof localStorage !== "undefined" ? localStorage : null
): ThemeMode {
  try {
    return normalizeThemeMode(storage?.getItem(THEME_STORAGE_KEY));
  } catch {
    return "system";
  }
}

export function writeStoredThemeMode(
  mode: ThemeMode,
  storage: Pick<Storage, "setItem"> | null = typeof localStorage !== "undefined" ? localStorage : null
): ThemeMode {
  const normalized = normalizeThemeMode(mode);
  try {
    storage?.setItem(THEME_STORAGE_KEY, normalized);
  } catch {
    // Ignore storage write errors in restricted environments.
  }
  return normalized;
}

export function applyThemeMode(
  mode: ThemeMode,
  root: { setAttribute: (name: string, value: string) => void; removeAttribute: (name: string) => void } | null =
    typeof document !== "undefined" ? document.documentElement : null
): ThemeMode {
  const normalized = normalizeThemeMode(mode);
  if (root) {
    if (normalized === "system") {
      root.removeAttribute("data-theme");
    } else {
      root.setAttribute("data-theme", normalized);
    }
  }
  return normalized;
}
