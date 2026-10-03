/** Colour themes. `system` follows the OS light/dark setting. */
const THEMES = [
  ["system", "System"],
  ["light", "Light"],
  ["dark", "Dark"],
  ["solarized-light", "Solarized Light"],
  ["solarized-dark", "Solarized Dark"],
  ["nord", "Nord"],
  ["dracula", "Dracula"],
  ["paper", "Paper"],
] as const;

const STORAGE_KEY = "theme";
const dark = window.matchMedia("(prefers-color-scheme: dark)");

/** The theme to put on <html>: `system` resolved to light or dark. */
function resolve(choice: string): string {
  if (choice === "system") return dark.matches ? "dark" : "light";
  return choice;
}

/**
 * Fill the theme menu, apply the saved choice, and call `onChange` whenever
 * the applied theme changes (menu or OS setting). index.html applies the saved
 * theme before first paint with the same rules.
 */
export function initTheme(select: HTMLSelectElement, onChange: () => void): void {
  for (const [id, label] of THEMES) select.add(new Option(label, id));
  let choice = "system";
  try {
    choice = localStorage.getItem(STORAGE_KEY) ?? "system";
  } catch {}
  if (!THEMES.some(([id]) => id === choice)) choice = "system";
  select.value = choice;

  const apply = () => {
    document.documentElement.dataset.theme = resolve(select.value);
    onChange();
  };
  select.addEventListener("change", () => {
    try {
      localStorage.setItem(STORAGE_KEY, select.value);
    } catch {}
    apply();
  });
  dark.addEventListener("change", () => {
    if (select.value === "system") apply();
  });
  apply();
}
