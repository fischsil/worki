import { create } from "zustand";

import { api, errorMessage, type LogLine } from "./api";
import type { RunTarget } from "./runSnippet";

export type RunStatus =
  | { kind: "running" }
  | { kind: "exited"; code: number }
  | { kind: "stopped" }
  | { kind: "failed"; error: string };

export interface SnippetRun {
  id: string;
  lines: LogLine[];
  status: RunStatus;
}

const MAX_LINES = 2000;
const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07/g;

interface SnippetRuns {
  runs: Record<string, SnippetRun>;
  start: (key: string, target: RunTarget, command: string) => void;
  stop: (key: string) => void;
  clear: (key: string) => void;
}

export const useSnippetRuns = create<SnippetRuns>()((set, get) => {
  const update = (key: string, id: string, change: (run: SnippetRun) => Partial<SnippetRun>) =>
    set((s) => {
      const run = s.runs[key];
      return run?.id === id ? { runs: { ...s.runs, [key]: { ...run, ...change(run) } } } : s;
    });

  return {
    runs: {},
    start: (key, target, command) => {
      get().stop(key);
      const id = crypto.randomUUID();
      set((s) => ({ runs: { ...s.runs, [key]: { id, lines: [], status: { kind: "running" } } } }));
      let pending: LogLine[] = [];
      let flushing: ReturnType<typeof setTimeout> | undefined;
      const flush = () => {
        clearTimeout(flushing);
        flushing = undefined;
        const lines = pending;
        pending = [];
        if (lines.length) update(key, id, (run) => ({ lines: [...run.lines, ...lines].slice(-MAX_LINES) }));
      };
      const onOutput = (line: LogLine) => {
        pending.push({ ...line, line: line.line.replace(ANSI, "") });
        if (pending.length > MAX_LINES) pending = pending.slice(-MAX_LINES);
        flushing ??= setTimeout(flush, 50);
      };
      const finish = (status: RunStatus) => {
        flush();
        update(key, id, () => ({ status }));
      };
      api.runSnippet(target.projectId, target.worktreePath, id, command, onOutput).then(
        (code) => finish(code === null ? { kind: "stopped" } : { kind: "exited", code }),
        (e) => finish({ kind: "failed", error: errorMessage(e) }),
      );
    },
    stop: (key) => {
      const run = get().runs[key];
      if (run?.status.kind === "running") api.stopSnippet(run.id).catch(() => {});
    },
    clear: (key) => {
      get().stop(key);
      set((s) => {
        const { [key]: _, ...runs } = s.runs;
        return { runs };
      });
    },
  };
});
