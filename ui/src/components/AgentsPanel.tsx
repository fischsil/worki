import { useQueryClient } from "@tanstack/react-query";
import clsx from "clsx";
import { CircleCheck, CircleX, KeyRound, LoaderCircle, RefreshCw } from "lucide-react";
import { useState } from "react";

import {
  type AgentStatus,
  api,
  errorMessage,
  type Isolation,
  type LogLine,
  scopeKey,
} from "../lib/api";
import { keys, useAgentStatuses, useRefreshSecrets } from "../lib/queries";
import { SignInDialog } from "./SignInDialog";
import { Button, ErrorText, IconButton, LogView } from "./ui";

type Action = "share" | "token" | "removeToken";

type AgentsIsolation = Extract<Isolation, { kind: "macUser" } | { kind: "ssh" }>;

/**
 * Per-agent setup for User Isolation and SSH: reuse your installation, sign in. Containers and
 * sandboxes need none: agents are installed on first use and Claude Code signs in with an account.
 */
export function AgentsPanel({ isolation }: { isolation: AgentsIsolation }) {
  const qc = useQueryClient();
  const { data: agents, isFetching, error, refetch } = useAgentStatuses(isolation);
  const [running, setRunning] = useState<{ exe: string; action: Action } | null>(null);
  const [result, setResult] = useState<{ exe: string; lines: LogLine[]; error?: string } | null>(null);
  const [signingIn, setSigningIn] = useState<AgentStatus | null>(null);
  const refreshSecrets = useRefreshSecrets();
  const user = isolation.kind === "macUser" ? isolation.user : isolation.host;
  const where = isolation.kind === "ssh" ? `on ${isolation.host}` : `for ${user}`;

  async function run(agent: AgentStatus, action: Action, tokenName = "") {
    const exe = agent.executable;
    setRunning({ exe, action });
    const lines: LogLine[] = [];
    const push = (line: LogLine) => {
      lines.push(line);
      setResult({ exe, lines: [...lines] });
    };
    setResult({ exe, lines: [] });
    try {
      if (action === "share") push({ stream: "info", line: `Linked ${await api.shareAgent(isolation, exe)}` });
      if (action === "token" && agent.auth) {
        await api.signInAgent(isolation, exe, tokenName, push);
        push({ stream: "info", line: `Saved ${agent.auth.secret} in your Keychain for agents running as ${user}.` });
        await refreshSecrets();
      }
      if (action === "removeToken" && agent.auth) {
        await api.deleteSecret(scopeKey(isolation), agent.auth.secret);
        push({ stream: "info", line: `Removed ${agent.auth.secret}.` });
        await refreshSecrets();
      }
    } catch (e) {
      setResult({ exe, lines: [...lines], error: errorMessage(e) });
    } finally {
      setRunning(null);
      qc.invalidateQueries({ queryKey: keys.agentStatuses(isolation) });
    }
  }

  return (
    <div className="flex flex-col gap-1 rounded-lg border border-line bg-panel p-2">
      <div className="flex items-center justify-between px-1 pb-1">
        <span className="text-[11px] font-semibold tracking-wide text-faint uppercase">
          Agents {where}
        </span>
        <IconButton label="Refresh" onClick={() => refetch()} disabled={isFetching}>
          <RefreshCw size={12} className={clsx(isFetching && "animate-spin")} />
        </IconButton>
      </div>
      {!agents && isFetching && <p className="px-1 py-1 text-xs text-faint">Looking for agents…</p>}
      {error && <ErrorText>{String(error)}</ErrorText>}
      {agents?.map((a) => {
        const busy = running?.exe === a.executable ? running.action : null;
        const btn = (action: Action, label: string, show: boolean, primary = false) =>
          show && (
            <Button
              key={action}
              variant={primary ? "primary" : "secondary"}
              onClick={() => (action === "token" ? setSigningIn(a) : run(a, action))}
              disabled={running !== null}
            >
              {busy === action && <LoaderCircle size={12} className="animate-spin" />}
              {label}
            </Button>
          );
        return (
          <div key={a.profileId} className="flex flex-col gap-1.5 rounded-md px-1 py-1.5">
            <div className="flex items-start gap-2">
              {a.envPath ? (
                <CircleCheck size={14} className="mt-0.5 shrink-0 text-added" />
              ) : (
                <CircleX size={14} className="mt-0.5 shrink-0 text-removed" />
              )}
              <div className="min-w-0 flex-1">
                <div className="text-[13px]">
                  {a.name}{" "}
                  <span className="text-faint">
                    {a.envPath ? `is installed ${where}` : `is not installed ${where}`}
                  </span>
                </div>
                <div className="selectable truncate font-mono text-[11px] text-faint">
                  {a.envPath ?? (a.hostPath ? `Yours: ${a.hostPath}` : `Not installed for you either`)}
                </div>
                {a.auth && (
                  <div className="mt-0.5 flex items-center gap-1 text-[11px] text-faint">
                    <KeyRound size={11} className={a.auth.set ? "text-added" : ""} />
                    {a.auth.set ? `Signed in with a token (${a.auth.secret})` : "Not signed in"}
                  </div>
                )}
              </div>
            </div>
            <div className="ml-6 flex flex-wrap gap-1.5">
              {btn("share", "Use my installation", a.canShare && !a.envPath && !!a.hostPath, true)}
              {btn("token", a.auth?.set ? "Sign in again" : "Sign in", !!a.auth)}
              {btn("removeToken", "Remove token", !!a.auth?.set)}
            </div>
            {result?.exe === a.executable && (result.lines.length > 0 || result.error) && (
              <ResultLog lines={result.lines} error={result.error} />
            )}
          </div>
        );
      })}
      <p className="px-1 pt-1 text-xs leading-relaxed text-faint">
        Sign in runs <span className="font-mono">claude setup-token</span> as you and approves in your browser. The
        token uses your subscription and is passed to {user}'s sessions; your own login stays untouched. Codex
        signs in per environment with a code you approve in the browser the first time it starts there.
      </p>
      {signingIn && (
        <SignInDialog onSignIn={(name) => run(signingIn, "token", name)} onClose={() => setSigningIn(null)} />
      )}
    </div>
  );
}

function ResultLog({ lines, error }: { lines: LogLine[]; error?: string }) {
  return (
    <div className="ml-6 flex flex-col gap-1">
      {lines.length > 0 && <LogView lines={lines} className="max-h-40 bg-elevated" />}
      <ErrorText>{error}</ErrorText>
    </div>
  );
}
