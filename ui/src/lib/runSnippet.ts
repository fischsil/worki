import type { QueryClient } from "@tanstack/react-query";

import { api, type SessionInfo } from "./api";
import { keys, rememberSession, worktreeShells } from "./queries";
import { useUi } from "./store";

export interface RunTarget {
  projectId: string;
  worktreePath: string;
  sessionId: string;
}

const TRANSCRIPT_LANGUAGES = ["console", "shell-session", "shellsession"];
const SHELL_LANGUAGES = ["bash", "sh", "shell", "zsh", "fish", ...TRANSCRIPT_LANGUAGES];
const QUIET_MS = 300;
const READY_TIMEOUT_MS = 15000;

export function isShellLanguage(language: string | undefined): boolean {
  return !!language && SHELL_LANGUAGES.includes(language.toLowerCase());
}

/** The command to run from a shell snippet; a `console` transcript keeps only its `$ ` lines. */
export function snippetCommand(language: string | undefined, source: string): string {
  if (!TRANSCRIPT_LANGUAGES.includes(language?.toLowerCase() ?? "")) return source.trim();
  const commands = source.split("\n").flatMap((line) => line.match(/^\s*\$ (.*)/)?.[1] ?? []);
  return commands.length ? commands.join("\n") : source.trim();
}

export async function runOnHost(qc: QueryClient, target: RunTarget, command: string) {
  const shell = await hostShell(qc, target);
  await api.writeSession(shell.id, `\x15${command.replace(/\n/g, "\r")}\r`);
}

const cached = (qc: QueryClient) => qc.getQueryData<SessionInfo[]>(keys.sessions) ?? [];

async function hostShell(qc: QueryClient, { projectId, worktreePath }: RunTarget): Promise<SessionInfo> {
  const { activeTerminal, setActiveTerminal, showTerminal, toggleTerminal } = useUi.getState();
  const shells = worktreeShells(cached(qc), worktreePath).filter((s) => s.state.kind === "running");
  let shell = shells.find((s) => s.id === activeTerminal[worktreePath]) ?? shells.at(-1);
  if (!shell) {
    shell = await api.spawnWorktreeShell(projectId, worktreePath, 120, 12);
    rememberSession(qc, shell);
    await untilReady(shell.id);
  }
  setActiveTerminal(worktreePath, shell.id);
  if (!showTerminal) toggleTerminal();
  return shell;
}

/** Resolves once a new shell printed something and then went quiet, so typed input isn't lost. */
async function untilReady(id: string) {
  let done!: () => void;
  const ready = new Promise<void>((resolve) => (done = resolve));
  let quiet: ReturnType<typeof setTimeout> | undefined;
  const limit = setTimeout(done, READY_TIMEOUT_MS);
  const sink = await api
    .attachSession(id, () => {
      clearTimeout(quiet);
      quiet = setTimeout(done, QUIET_MS);
    })
    .catch(() => null);
  if (sink !== null) {
    await ready;
    await api.detachSession(id, sink).catch(() => {});
  }
  clearTimeout(quiet);
  clearTimeout(limit);
}
