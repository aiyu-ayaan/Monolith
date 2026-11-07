import { createSignal } from "solid-js";

const get = (k: string) => {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
};
const set = (k: string, v: string) => {
  try {
    localStorage.setItem(k, v);
  } catch {
    /* private mode */
  }
};

/** Preferred code tab (`typescript`, `kotlin`, ...), remembered across pages. */
export const [lang, setLangSignal] = createSignal<string | null>(get("docs.lang"));
export const setLang = (l: string) => {
  setLangSignal(l);
  set("docs.lang", l);
};

export type Theme = "light" | "dark" | "system";
export const [theme, setThemeSignal] = createSignal<Theme>((get("docs.theme") as Theme) || "system");
export const setTheme = (t: Theme) => {
  setThemeSignal(t);
  set("docs.theme", t);
  applyTheme();
};
export function applyTheme() {
  const t = theme();
  if (t === "system") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", t);
}
