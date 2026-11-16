import clsx from "clsx";
import { Activity as ActivityIcon, ChevronRight, CircleAlert, CircleCheck, LoaderCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { isNotable, lastStep, useActivities, useSeenActivities } from "../lib/activities";
import type { Operation } from "../lib/api";
import { useProjects } from "../lib/queries";
import { clockTime, elapsed, fullTime, useNow } from "../lib/time";
import { LogView, useDismiss } from "./ui";

/** The sidebar's activity indicator: the running operation's latest step, and the panel of recent ones. */
export function ActivityButton() {
  const { data = [] } = useActivities();
  const { seenAt, see } = useSeenActivities();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const shown = data.filter(isNotable);
  const running = shown.find((a) => a.status === "running");
  const isUnseen = (a: Operation) => a.status === "failed" && (a.finishedAt ?? 0) > seenAt;
  const unseenFailure = shown.some(isUnseen);
  const [toggled, setToggled] = useState<Record<number, boolean>>({});
  const isExpanded = (a: Operation) => toggled[a.id] ?? (a.status === "running" || isUnseen(a));
  const wide = shown.some((a) => isExpanded(a) && a.lines.length > 0);

  useDismiss(root, open, () => setOpen(false));
  useEffect(() => {
    if (!open) return;
    return () => {
      setToggled({});
      see();
    };
  }, [open, see]);

  return (
    <div ref={root} className="flex min-w-0 flex-1">
      <button
        onClick={() => setOpen((o) => !o)}
        title={running ? `${running.title}: ${lastStep(running) ?? "working…"}` : "Recent activity"}
        className={clsx(
          "flex h-7 min-w-0 items-center gap-1.5 rounded-md px-2 hover:bg-hover",
          running ? "text-muted" : unseenFailure ? "text-removed" : "text-faint hover:text-fg",
        )}
      >
        {running ? (
          <LoaderCircle size={13} className="shrink-0 animate-spin" />
        ) : unseenFailure ? (
          <CircleAlert size={14} className="shrink-0" />
        ) : (
          <ActivityIcon size={14} className="shrink-0" />
        )}
        {running && <span className="min-w-0 truncate text-[12px]">{lastStep(running) ?? running.title}</span>}
      </button>
      {open && (
        <div
          className={clsx(
            "absolute bottom-full left-2 z-40 mb-1 flex flex-col overflow-hidden rounded-lg border border-line bg-elevated shadow-lg transition-[width]",
            wide ? "max-h-[85vh] w-[min(760px,calc(100vw-1rem))]" : "max-h-[70vh] w-[400px]",
          )}
        >
          <div className="border-b border-line px-3 py-2 text-[12px] font-medium">Activity</div>
          {shown.length === 0 ? (
            <p className="px-3 py-4 text-[12px] text-faint">
              Nothing yet. Creating worktrees, starting sessions, snapshots and other operations that take a while show
              up here with their steps.
            </p>
          ) : (
            <div className="flex flex-col overflow-y-auto py-1">
              {shown.map((a) => (
                <ActivityRow
                  key={a.id}
                  activity={a}
                  expanded={isExpanded(a)}
                  onToggle={() => setToggled((t) => ({ ...t, [a.id]: !isExpanded(a) }))}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ActivityRow({
  activity,
  expanded,
  onToggle,
}: {
  activity: Operation;
  expanded: boolean;
  onToggle: () => void;
}) {
  const running = activity.status === "running";
  const step = lastStep(activity);
  return (
    <div className="flex flex-col px-1">
      <button
        onClick={onToggle}
        className="flex min-w-0 items-center gap-2 rounded-md px-2 py-1 text-left text-[12px] hover:bg-hover"
      >
        {running ? (
          <LoaderCircle size={13} className="shrink-0 animate-spin text-accent" />
        ) : activity.status === "failed" ? (
          <CircleAlert size={13} className="shrink-0 text-removed" />
        ) : (
          <CircleCheck size={13} className="shrink-0 text-added" />
        )}
        <span className="min-w-0 flex-1 truncate">
          {activity.title}
          <Subject activity={activity} />
        </span>
        <span className="shrink-0 text-[11px] text-faint tabular-nums" title={`Started ${fullTime(activity.startedAt)}`}>
          {clockTime(activity.startedAt)}
        </span>
        <Elapsed activity={activity} />
        <ChevronRight size={12} className={clsx("shrink-0 text-faint transition-transform", expanded && "rotate-90")} />
      </button>
      {!expanded && running && step && <p className="truncate pr-2 pl-8 text-[11px] text-muted">{step}</p>}
      {!expanded && activity.error && <p className="truncate pr-2 pl-8 text-[11px] text-removed">{activity.error}</p>}
      {expanded && (
        <div className="flex flex-col gap-1 pr-2 pb-2 pl-8">
          {activity.lines.length > 0 && <LogView lines={activity.lines} className="max-h-80" />}
          {activity.error && <p className="selectable text-[11px] break-words text-removed">{activity.error}</p>}
          {activity.lines.length === 0 && !activity.error && (
            <p className="text-[11px] text-faint">{running ? "Working…" : "Done."}</p>
          )}
        </div>
      )}
    </div>
  );
}

function Subject({ activity }: { activity: Operation }) {
  const { data: projects = [] } = useProjects();
  const worktree = activity.worktreePath?.split("/").pop();
  const project = projects.find((p) => p.id === activity.projectId)?.name;
  const subject = worktree ?? project;
  return subject ? <span className="text-faint"> · {subject}</span> : null;
}

function Elapsed({ activity }: { activity: Operation }) {
  const now = useNow(activity.status === "running");
  return (
    <span className="shrink-0 font-mono text-[11px] text-faint tabular-nums">
      {elapsed(activity.startedAt, activity.finishedAt ?? now)}
    </span>
  );
}
