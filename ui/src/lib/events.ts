import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

import type { LogLine, Operation, SessionInfo } from "./api";

/** Events the backend sends, with their payloads (see `sessions::events` in src-tauri). */
export interface AppEvents {
  "session-changed": SessionInfo;
  "app-quitting": null;
  /** An activity started or finished. */
  activity: Operation;
  /** A step or output line an activity logged. */
  "activity-log": { id: number; line: LogLine };
  /** An agent was installed into a worktree's container or sandbox. */
  "agent-installed": { projectId: string; worktreePath: string; agent: string };
  /** An agent waits for a device-code sign-in before it starts in a worktree. */
  "agent-sign-in": { worktreePath: string; agent: string; url: string; code: string };
  "agent-signed-in": { worktreePath: string };
}

/** Subscribes to `name` while mounted; `handler` may change between renders. */
export function useTauriEvent<K extends keyof AppEvents>(name: K, handler: (payload: AppEvents[K]) => void) {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    const unlisten = listen<AppEvents[K]>(name, ({ payload }) => latest.current(payload));
    return () => {
      unlisten.then((f) => f());
    };
  }, [name]);
}
