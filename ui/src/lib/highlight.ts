import type { BundledLanguage, Highlighter, ThemedToken, ThemeRegistration } from "shiki";

import type { DiffLine } from "./diff";

export type { ThemedToken };

const FILE_NAMES: Record<string, BundledLanguage> = { dockerfile: "docker", containerfile: "docker", makefile: "make" };

/** Too large to highlight without a noticeable pause. */
const MAX_CHARS = 400_000;

/** Shiki, loaded with the first diff, so it stays out of the app's startup. */
const shiki = () => import("shiki");

let highlighter: Promise<{ h: Highlighter; theme: ThemeRegistration }> | null = null;

/**
 * Shiki with its JavaScript regex engine (the WebAssembly one doesn't load in every webview), and
 * a theme whose colors come from `--shiki-*` variables, so they follow worki's light and dark
 * themes.
 */
async function load(lang: BundledLanguage) {
  highlighter ??= shiki().then(async ({ createHighlighter, createCssVariablesTheme, createJavaScriptRegexEngine }) => {
    const theme = createCssVariablesTheme({ name: "worki", fontStyle: true });
    const h = await createHighlighter({ themes: [theme], langs: [], engine: createJavaScriptRegexEngine() });
    return { h, theme };
  });
  const loaded = await highlighter;
  await loaded.h.loadLanguage(lang);
  return loaded;
}

/** Shiki's language for `file`, by name or extension, or `null` when it has none. */
async function languageOf(file: string): Promise<BundledLanguage | null> {
  const name = file.split("/").pop()!.toLowerCase();
  if (FILE_NAMES[name]) return FILE_NAMES[name];
  const ext = name.includes(".") ? name.split(".").pop()! : name;
  const { bundledLanguagesInfo } = await shiki();
  const lang = bundledLanguagesInfo.find((l) => l.id === ext || l.aliases?.includes(ext));
  return (lang?.id as BundledLanguage | undefined) ?? null;
}

/**
 * Tokens for each code line of a diff, by line index; empty when the file can't be highlighted.
 * The old and new sides are highlighted as whole texts, so strings and comments spanning lines
 * come out right.
 */
export async function highlightDiff(file: string, lines: DiffLine[]): Promise<Map<number, ThemedToken[]>> {
  const tokens = new Map<number, ThemedToken[]>();
  const size = lines.reduce((n, l) => n + l.text.length, 0);
  if (size > MAX_CHARS) return tokens;

  let lang: BundledLanguage | null;
  let loaded: Awaited<ReturnType<typeof load>>;
  try {
    lang = await languageOf(file);
    if (!lang) return tokens;
    loaded = await load(lang);
  } catch (e) {
    console.warn(`highlighting ${file}:`, e);
    return tokens;
  }
  const { h, theme } = loaded;
  const side = (kinds: DiffLine["kind"][], keep: DiffLine["kind"][]) => {
    const indices = lines.flatMap((l, i) => (kinds.includes(l.kind) ? [i] : []));
    const text = indices.map((i) => lines[i].text).join("\n");
    const highlighted = h.codeToTokensBase(text, { lang: lang!, theme });
    indices.forEach((i, n) => keep.includes(lines[i].kind) && tokens.set(i, highlighted[n] ?? []));
  };
  side(["context", "del"], ["del"]);
  side(["context", "add"], ["context", "add"]);
  return tokens;
}
