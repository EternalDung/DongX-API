export type ThemeMode = "system" | "light" | "dark";

const STORAGE_KEY = "dongx-theme";

/** Apply a theme mode to <html>: toggles the `.dark` class. */
export function applyTheme(mode: ThemeMode): void {
  const root = document.documentElement;
  const isDark =
    mode === "dark" ||
    (mode === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);

  root.classList.toggle("dark", isDark);
  root.style.colorScheme = isDark ? "dark" : "light";
  localStorage.setItem(STORAGE_KEY, mode);
}

/** Read the saved theme (defaults to system). Also listens for OS changes. */
export function initTheme(): void {
  const saved = (localStorage.getItem(STORAGE_KEY) as ThemeMode | null) ?? "system";
  applyTheme(saved);

  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => {
      const current =
        (localStorage.getItem(STORAGE_KEY) as ThemeMode | null) ?? "system";
      if (current === "system") applyTheme("system");
    });
}

export function getStoredTheme(): ThemeMode {
  return (localStorage.getItem(STORAGE_KEY) as ThemeMode | null) ?? "system";
}
