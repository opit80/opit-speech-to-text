import type { Theme } from "./types";

export function resolveTheme(choice: Theme, systemDark: boolean): "light" | "dark" {
  if (choice !== "system") return choice;
  return systemDark ? "dark" : "light";
}

/** Keeps native controls and every colour token in sync, including live system changes. */
export function watchTheme(choice: Theme): () => void {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => { document.documentElement.dataset.theme = resolveTheme(choice, media.matches); };
  apply();
  media.addEventListener("change", apply);
  return () => media.removeEventListener("change", apply);
}
