import { useQueryClient } from "@tanstack/react-query";
import { message } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, LoaderCircle, Plus, RefreshCw, Trash2, UserRound } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { api, type ClaudeAccount, errorMessage, loginSharers, type LoginEvent } from "../lib/api";
import { keys, useAgentSettings, useProjects } from "../lib/queries";
import { Button, DefaultMark, ErrorText, Field, IconButton, Input, NestedDialog, SharedBadge } from "./ui";

/** Claude Code logins of worki's own: projects use the default one unless they pick another. */
export function ClaudeAccounts({ accounts, defaultId }: { accounts: ClaudeAccount[]; defaultId: string | null }) {
  const qc = useQueryClient();
  const [signingIn, setSigningIn] = useState<{ account: ClaudeAccount; isNew: boolean } | null>(null);
  const [checking, setChecking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => qc.invalidateQueries({ queryKey: keys.agentSettings });
  const { data: settings } = useAgentSettings();
  const { data: projects = [] } = useProjects();
  const sharers = (accountId: string) => (settings ? loginSharers(settings, projects, accountId) : []);

  async function act(work: () => Promise<unknown>) {
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(errorMessage(e));
    }
    await refresh();
  }

  const add = () => act(async () => setSigningIn({ account: await api.addClaudeAccount(), isNew: true }));

  const check = (account: ClaudeAccount) =>
    act(async () => {
      setChecking(account.id);
      await api.checkClaudeAccount(account.id).finally(() => setChecking(null));
    });

  const remove = (account: ClaudeAccount) =>
    act(async () => {
      const warning = await api.removeClaudeAccount(account.id);
      if (warning) await message(warning, { title: "Account removed", kind: "warning" });
    });

  return (
    <>
      {accounts.length > 0 ? (
        <div className="flex flex-col divide-y divide-line rounded-lg border border-line">
          {accounts.map((a) => {
            const sharing = sharers(a.id);
            return (
              <div key={a.id} className="flex items-center gap-3 px-3 py-2">
                <UserRound size={14} className={a.email ? "shrink-0 text-added" : "shrink-0 text-faint"} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px]">{a.email ?? "Not signed in"}</div>
                  {(a.plan || a.organization) && (
                    <div className="truncate text-[11px] text-faint">
                      {[a.plan, a.organization].filter(Boolean).join(" · ")}
                    </div>
                  )}
                  {sharing.length > 1 && (
                    <div className="mt-0.5" title={sharing.map((p) => p.name).join(", ")}>
                      <SharedBadge>Login folder shared by {sharing.length} projects</SharedBadge>
                    </div>
                  )}
                </div>
                <DefaultMark
                  isDefault={a.id === defaultId}
                  onMakeDefault={() => act(() => api.setDefaultAccount(a.id))}
                  label="Default"
                />
                <Button onClick={() => setSigningIn({ account: a, isNew: false })} className="shrink-0 whitespace-nowrap">
                  {a.email ? "Sign in again" : "Sign in"}
                </Button>
                {a.email && (
                  <IconButton label="Check who it signs in as" onClick={() => check(a)}>
                    {checking === a.id ? <LoaderCircle size={13} className="animate-spin" /> : <RefreshCw size={13} />}
                  </IconButton>
                )}
                <IconButton label="Remove account" onClick={() => remove(a)}>
                  <Trash2 size={13} />
                </IconButton>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="-mt-1 text-[13px] leading-relaxed text-faint">
          Sign in once per account. Projects use the default one unless they pick another, in Docker Sandboxes, Docker
          and Podman.
        </p>
      )}
      <Button className="self-start" onClick={add}>
        <Plus size={13} />
        Add account
      </Button>
      <ErrorText>{error}</ErrorText>
      {signingIn && (
        <SignInAccountDialog
          account={signingIn.account}
          onClose={async (signedIn) => {
            const { account, isNew } = signingIn;
            setSigningIn(null);
            if (isNew && !signedIn) await api.removeClaudeAccount(account.id).catch(() => {});
            refresh();
          }}
        />
      )}
    </>
  );
}

/** Runs `claude auth login` in a throwaway container: open the link, then paste the code it shows. */
function SignInAccountDialog({
  account,
  onClose,
}: {
  account: ClaudeAccount;
  onClose: (signedIn: boolean) => void;
}) {
  return (
    <ClaudeSignInDialog
      title={account.email ? `Sign in again as ${account.email}` : "Sign in to Claude Code"}
      flowId={account.id}
      start={(onEvent) => api.logInClaudeAccount(account.id, onEvent)}
      onClose={onClose}
    />
  );
}

/** A Claude Code sign-in in a throwaway container, as sign-in `flowId` in the backend: open the
 * link it sends, then paste the code the page shows. */
export function ClaudeSignInDialog({
  title,
  flowId,
  start,
  onClose,
}: {
  title: string;
  flowId: string;
  start: (onEvent: (event: LoginEvent) => void) => Promise<unknown>;
  onClose: (signedIn: boolean) => void;
}) {
  const [url, setUrl] = useState<string | null>(null);
  const [lastLine, setLastLine] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const started = useRef(false);
  const done = useRef(false);

  // Started once, not undone in a cleanup: React runs effects twice in development, and a
  // second sign-in replaces the first in the backend.
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    start((event) => {
      if (event.kind === "url") setUrl(event.url);
      else setLastLine(event.line);
    })
      .then(() => {
        done.current = true;
        onClose(true);
      })
      .catch((e) => {
        done.current = true;
        setError(errorMessage(e));
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flowId]);

  const close = () => {
    if (!done.current) api.cancelLogin(flowId);
    onClose(false);
  };

  const submit = async () => {
    if (!code.trim()) return;
    setError(null);
    try {
      await api.enterLoginCode(flowId, code);
      setSent(true);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <NestedDialog
      title={title}
      onClose={close}
      width={460}
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit} disabled={!url || sent || !code.trim()}>
            {sent && !error && <LoaderCircle size={12} className="animate-spin" />}
            Sign in
          </Button>
        </>
      }
    >
      {!url && !error ? (
        <div className="flex items-center gap-2 text-[13px] text-muted">
          <LoaderCircle size={13} className="animate-spin" />
          Starting Claude Code in a container…
        </div>
      ) : (
        url && (
          <>
            <div className="flex flex-col gap-1.5">
              <span className="text-xs font-medium text-muted">1. Approve with the account to use</span>
              <Button className="self-start" onClick={() => openUrl(url)}>
                <ExternalLink size={12} />
                Open sign-in page
              </Button>
            </div>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                submit();
              }}
            >
              <Field label="2. Paste the code the page shows">
                <Input
                  autoFocus
                  value={code}
                  disabled={sent && !error}
                  onChange={(e) => setCode(e.target.value)}
                  className="font-mono"
                />
              </Field>
            </form>
          </>
        )
      )}
      {lastLine && sent && <p className="truncate font-mono text-[11px] text-faint">{lastLine}</p>}
      <ErrorText>{error}</ErrorText>
    </NestedDialog>
  );
}
