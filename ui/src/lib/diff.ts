export type DiffLine =
  | { kind: "hunk"; text: string }
  | { kind: "context" | "add" | "del"; text: string; oldNo: number | null; newNo: number | null }
  | { kind: "note"; text: string };

/** Parses `git diff` output for one file into display lines, skipping the file header. */
export function parseDiff(text: string): { lines: DiffLine[]; binary: boolean } {
  const lines: DiffLine[] = [];
  let oldNo = 0;
  let newNo = 0;
  let inHunk = false;
  let binary = false;

  for (const raw of text.split("\n")) {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(raw);
    if (hunk) {
      oldNo = Number(hunk[1]);
      newNo = Number(hunk[2]);
      inHunk = true;
      lines.push({ kind: "hunk", text: raw });
      continue;
    }
    if (!inHunk) {
      if (raw.startsWith("Binary files")) binary = true;
      continue;
    }
    if (raw.startsWith("+")) lines.push({ kind: "add", text: raw.slice(1), oldNo: null, newNo: newNo++ });
    else if (raw.startsWith("-")) lines.push({ kind: "del", text: raw.slice(1), oldNo: oldNo++, newNo: null });
    else if (raw.startsWith(" ")) lines.push({ kind: "context", text: raw.slice(1), oldNo: oldNo++, newNo: newNo++ });
    else if (raw.startsWith("\\")) lines.push({ kind: "note", text: raw.slice(1).trim() });
  }
  return { lines, binary };
}

export type DiffRow = { kind: "line"; index: number } | { kind: "gap"; id: number; start: number; end: number };

/** Revealed lines of a collapsed run, counted from its top and from its bottom. */
export interface GapReveal {
  head: number;
  tail: number;
}

/**
 * Collapses unchanged lines further than `context` lines from a change, except those in `keep`.
 * Gap ids are the index of the run's first line, so they stay stable while lines are revealed.
 */
export function diffRows(
  lines: DiffLine[],
  revealed: Record<number, GapReveal>,
  keep: Set<number> = new Set(),
  context = 3,
): DiffRow[] {
  const shown = lines.map((l, i) => l.kind === "add" || l.kind === "del" || l.kind === "note" || keep.has(i));
  lines.forEach((l, i) => {
    if (l.kind !== "add" && l.kind !== "del") return;
    for (let j = Math.max(0, i - context); j <= Math.min(lines.length - 1, i + context); j++) shown[j] = true;
  });
  const rows: DiffRow[] = [];
  let i = 0;
  while (i < lines.length) {
    if (lines[i].kind === "hunk") {
      i++;
      continue;
    }
    if (shown[i]) {
      rows.push({ kind: "line", index: i });
      i++;
      continue;
    }
    const start = i;
    while (i < lines.length && !shown[i] && lines[i].kind === "context") i++;
    const end = i;
    const { head = 0, tail = 0 } = revealed[start] ?? {};
    if (head + tail >= end - start) {
      for (let j = start; j < end; j++) rows.push({ kind: "line", index: j });
      continue;
    }
    for (let j = start; j < start + head; j++) rows.push({ kind: "line", index: j });
    rows.push({ kind: "gap", id: start, start: start + head, end: end - tail });
    for (let j = end - tail; j < end; j++) rows.push({ kind: "line", index: j });
  }
  return rows;
}

/** Character ranges `[start, end)` within a line. */
export type Range = [number, number];

/** Above this many token pairs, comparing two lines word by word isn't worth the time. */
const MAX_WORD_PAIRS = 40_000;

/**
 * The words that changed within edited lines, by line index. Each run of removed lines is paired
 * line by line with the added lines right after it; lines that changed almost entirely are left
 * out, since their whole row already says so.
 */
export function changedWords(lines: DiffLine[]): Map<number, Range[]> {
  const changed = new Map<number, Range[]>();
  let i = 0;
  while (i < lines.length) {
    if (lines[i].kind !== "del") {
      i++;
      continue;
    }
    const removed: number[] = [];
    while (i < lines.length && lines[i].kind === "del") removed.push(i++);
    const added: number[] = [];
    while (i < lines.length && lines[i].kind === "add") added.push(i++);
    for (let k = 0; k < Math.min(removed.length, added.length); k++) {
      const ranges = wordDiff(lines[removed[k]].text, lines[added[k]].text);
      if (!ranges) continue;
      changed.set(removed[k], ranges[0]);
      changed.set(added[k], ranges[1]);
    }
  }
  return changed;
}

function wordDiff(a: string, b: string): [Range[], Range[]] | null {
  const ta = words(a);
  const tb = words(b);
  if (ta.length * tb.length > MAX_WORD_PAIRS) return null;

  const common: number[][] = Array.from({ length: ta.length + 1 }, () => new Array(tb.length + 1).fill(0));
  for (let x = ta.length - 1; x >= 0; x--)
    for (let y = tb.length - 1; y >= 0; y--)
      common[x][y] = ta[x].text === tb[y].text ? common[x + 1][y + 1] + 1 : Math.max(common[x + 1][y], common[x][y + 1]);

  const ra: Range[] = [];
  const rb: Range[] = [];
  let x = 0;
  let y = 0;
  while (x < ta.length || y < tb.length) {
    if (x < ta.length && y < tb.length && ta[x].text === tb[y].text) {
      x++;
      y++;
    } else if (y >= tb.length || (x < ta.length && common[x + 1][y] >= common[x][y + 1])) {
      add(ra, ta[x++]);
    } else {
      add(rb, tb[y++]);
    }
  }
  const mostly = (ranges: Range[], text: string) =>
    ranges.reduce((n, [s, e]) => n + e - s, 0) > 0.6 * text.trim().length;
  if (ra.length + rb.length === 0 || mostly(ra, a) || mostly(rb, b)) return null;
  return [ra, rb];
}

interface Word {
  text: string;
  start: number;
}

function words(line: string): Word[] {
  return [...line.matchAll(/\w+|\s+|[^\w\s]/g)].map((m) => ({ text: m[0], start: m.index }));
}

/** Adds a word's range, joining it to the previous one when they touch. */
function add(ranges: Range[], word: Word) {
  const end = word.start + word.text.length;
  const last = ranges.at(-1);
  if (last && last[1] === word.start) last[1] = end;
  else ranges.push([word.start, end]);
}
