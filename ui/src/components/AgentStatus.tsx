import clsx from "clsx";
import { Gauge, UserRound } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";

import type { Account, SessionInfo } from "../lib/api";
import { formatTokens, totalUsage } from "../lib/usage";
import { CopyContinue, RemoteControl } from "./SessionActions";

export const PERMISSION_MODES: Record<string, string> = {
  default: "Ask before edits",
  acceptEdits: "Accept edits",
  plan: "Plan",
  auto: "Auto",
  dontAsk: "Don't ask",
  bypassPermissions: "Bypass permissions",
  untrusted: "Ask before commands",
  "on-request": "Ask when needed",
  never: "Never ask",
};

/** Status bar below the session tabs: model, effort, permissions and context use, with
 * everything else the agent reports on click. */
export function AgentStatus({ session }: { session: SessionInfo }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const a = session.agent;

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  if (!a?.model) return null;
  const usage = totalUsage(session.usage);
  const context = a.contextUsed !== null ? Math.round(a.contextUsed) : null;
  const account = accountOf(session);
  const accountLabel = account && [accountName(account), account.plan].filter(Boolean).join(" · ");

  return (
    <div ref={ref} className="@container relative flex shrink-0 border-b border-line">
      <button
        onClick={() => setOpen((o) => !o)}
        title="Agent status"
        className={clsx(
          "flex h-6 min-w-0 flex-1 items-center gap-3 overflow-hidden px-3 text-[11px] whitespace-nowrap text-muted hover:bg-hover hover:text-fg",
          open && "bg-hover text-fg",
        )}
      >
        <span className="flex shrink-0 items-center gap-1.5">
          <Gauge size={12} />
          {a.model}
        </span>
        {a.effort && <span className="shrink-0">effort {a.effort}</span>}
        {a.permissionMode && (
          <span className="min-w-0 truncate">{PERMISSION_MODES[a.permissionMode] ?? a.permissionMode}</span>
        )}
        {accountLabel && (
          <span className="flex min-w-0 items-center gap-1 truncate" title={accountLabel}>
            <UserRound size={11} className="shrink-0" />
            <span className="truncate">{accountLabel}</span>
          </span>
        )}
        <span className="flex-1" />
        {usage && <span className="hidden @lg:inline">{formatTokens(usage.tokens)} tokens</span>}
        {a.costUsd !== null && <span className="hidden @md:inline">${a.costUsd.toFixed(2)}</span>}
        {context !== null && <ContextMeter percent={context} />}
      </button>
      <RemoteControl session={session} />
      <CopyContinue session={session} />
      {open && (
        <div data-overlay className="absolute top-full right-2 z-40 mt-1 w-64 rounded-lg border border-line bg-elevated p-3 text-xs shadow-xl">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
            <Row label="Model">{a.model}</Row>
            {a.effort && <Row label="Effort">{a.effort}</Row>}
            {a.fastMode !== null && <Row label="Fast mode">{a.fastMode ? "On" : "Off"}</Row>}
            {a.thinking !== null && <Row label="Thinking">{a.thinking ? "On" : "Off"}</Row>}
            {a.permissionMode && (
              <Row label="Permissions">{PERMISSION_MODES[a.permissionMode] ?? a.permissionMode}</Row>
            )}
            {context !== null && (
              <Row label="Context">
                {context}%{a.contextSize ? ` of ${formatTokens(a.contextSize)}` : ""}
              </Row>
            )}
            {usage && <Row label="Tokens">{formatTokens(usage.tokens)}</Row>}
            {a.costUsd !== null && <Row label="Cost">${a.costUsd.toFixed(2)}</Row>}
            {a.outputStyle && a.outputStyle !== "default" && <Row label="Output style">{a.outputStyle}</Row>}
            {account && accountName(account) && <Row label="Account">{accountName(account)}</Row>}
            {account?.plan && <Row label="Plan">{account.plan}</Row>}
            {account?.organization && <Row label="Organization">{account.organization}</Row>}
            {a.version && <Row label="Version">{a.version}</Row>}
          </dl>
          {context !== null && (
            <div className="mt-2.5 h-1 overflow-hidden rounded-full bg-hover">
              <div className={clsx("h-full rounded-full", meterColor(context))} style={{ width: `${context}%` }} />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** The agent's own report when it names the account, else what the session's environment said. */
function accountOf(session: SessionInfo): Account | null {
  const reported = session.agent.account;
  return reported?.email ? reported : (session.account ?? reported);
}

function accountName(account: Account): string | null {
  return account.email ?? account.organization ?? (account.token ? (account.tokenName ?? "Sign-in token") : null);
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-faint">{label}</dt>
      <dd className="truncate text-right text-fg">{children}</dd>
    </>
  );
}

function ContextMeter({ percent }: { percent: number }) {
  return (
    <span className="flex shrink-0 items-center gap-1.5" title={`${percent}% of the context window in use`}>
      context
      <span className="h-1 w-12 overflow-hidden rounded-full bg-hover">
        <span className={clsx("block h-full rounded-full", meterColor(percent))} style={{ width: `${percent}%` }} />
      </span>
      {percent}%
    </span>
  );
}

const meterColor = (percent: number) => (percent >= 80 ? "bg-removed" : percent >= 60 ? "bg-warn" : "bg-accent");
