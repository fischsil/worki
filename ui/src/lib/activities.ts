import { useQuery, useQueryClient } from "@tanstack/react-query";
import { create } from "zustand";

import { api, type Operation, type Worktree } from "./api";
import { useTauriEvent } from "./events";
import { keys } from "./queries";

const QUICK_MS = 1000;
const LINES_KEPT = 500;

/** Recent activities, newest first, kept current by `useActivityEvents`. */
export const useActivities = () =>
  useQuery({ queryKey: keys.activities, queryFn: api.activities, staleTime: Infinity });

/** Whether an activity is worth listing: still running, failed, or took a noticeable while. */
export const isNotable = (a: Operation) =>
  a.status !== "done" || (a.finishedAt ?? a.startedAt) - a.startedAt >= QUICK_MS;

export function lastStep(a: Operation): string | null {
  for (let i = a.lines.length - 1; i >= 0; i--) {
    if (a.lines[i].stream === "info") return a.lines[i].line;
  }
  return null;
}

/** The activity running for a worktree, such as a session start or saving a snapshot. */
export function useWorktreeActivity(path: string): Operation | undefined {
  const { data } = useQuery({
    queryKey: keys.activities,
    queryFn: api.activities,
    staleTime: Infinity,
    select: (all) => all.find((a) => a.worktreePath === path && a.status === "running"),
  });
  return data;
}

/** When the panel was last opened: failures after it are pointed out. */
export const useSeenActivities = create<{ seenAt: number; see: () => void }>((set) => ({
  seenAt: Date.now(),
  see: () => set({ seenAt: Date.now() }),
}));

/** Applies the backend's activity events to the cache; mounted once, in the app. */
export function useActivityEvents() {
  const qc = useQueryClient();
  const change = (update: (old: Operation[]) => Operation[]) =>
    qc.setQueryData<Operation[]>(keys.activities, (old) => update(old ?? []));

  useTauriEvent("activity", (activity) => {
    change((old) =>
      old.some((a) => a.id === activity.id)
        ? old.map((a) => (a.id === activity.id ? activity : a))
        : [activity, ...old],
    );
  });

  useTauriEvent("activity-log", ({ id, line }) => {
    const activity = qc.getQueryData<Operation[]>(keys.activities)?.find((a) => a.id === id);
    if (!activity) return;
    change((old) =>
      old.map((a) => (a.id === id ? { ...a, lines: [...a.lines.slice(1 - LINES_KEPT), line] } : a)),
    );
    // A worktree being created shows in the sidebar as soon as git has added it, while its
    // setup goes on.
    const { projectId, worktreePath } = activity;
    if (projectId && worktreePath && line.stream === "info") {
      const listed = qc.getQueryData<Worktree[]>(keys.worktrees(projectId))?.some((w) => w.path === worktreePath);
      if (!listed) qc.invalidateQueries({ queryKey: keys.worktrees(projectId) });
    }
  });
}
