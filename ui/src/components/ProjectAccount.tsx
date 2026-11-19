import { useQuery, useQueryClient } from "@tanstack/react-query";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { KeyRound } from "lucide-react";
import { useState } from "react";

import {
  type AccountSignIn,
  api,
  errorMessage,
  type FilesScope,
  type Isolation,
  accountOf,
  isContainer,
  loginSharers,
  signInOf,
} from "../lib/api";
import {
  keys,
  useAgentSettings,
  useProjectEnvironment,
  useProjects,
  useUpdateProjectEnvironment,
} from "../lib/queries";
import { ClaudeSignInDialog } from "./ClaudeAccounts";
import { Button, ErrorText, Field, Group, Input, Select, SharedBadge } from "./ui";

/** How the project's Claude Code sessions sign in: with one of the accounts added under Accounts
 * in the settings, or by themselves; and where their Claude Code files live. */
export function ProjectAccount({ projectId, isolation }: { projectId: string; isolation: Isolation }) {
  const qc = useQueryClient();
  const { data: settings } = useAgentSettings();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const accounts = settings?.accounts ?? [];
  const current = settings?.projectAccounts[projectId] ?? "";
  const signIn: AccountSignIn = settings ? signInOf(settings, projectId) : "session";
  const label = (email: string | null | undefined) => email ?? "not signed in";
  const fallback = accounts.find((a) => a.id === settings?.defaultAccount);
  const accountId = settings ? accountOf(settings, projectId) : null;
  const { data: projects = [] } = useProjects();
  const others = settings ? loginSharers(settings, projects, accountId).filter((p) => p.id !== projectId) : [];

  async function save(change: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await change();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
      await qc.invalidateQueries({ queryKey: keys.agentSettings });
    }
  }

  if (!isContainer(isolation)) {
    return (
      <p className="text-[13px] leading-relaxed text-faint">
        Only Docker (sbx), Docker, Podman and Apple container sessions use these; this project's sign in the way its isolation does.
      </p>
    );
  }

  const hints: Record<AccountSignIn, string> = {
    folder:
      "Sessions use the account's login, which renews itself. Projects on this account share the login and nothing else: each keeps its own Claude Code folder. Since a sign-in would switch them all, /login and /logout are off in these sessions; sign in again under Settings › Accounts.",
    token:
      "Sessions sign in with the account's token, which the account's other token projects use too, and share no files with them.",
    session:
      "Each worktree's sessions sign in themselves with /login. The login stays with that worktree's sandbox or container and is shared with nothing, not even the project's other worktrees.",
  };

  return (
    <>
      <Group title="Sign-in">
        <Field
          label={
            <>
              Sign in with
              {signIn === "folder" && (
                <SharedBadge>
                  {others.length
                    ? `Login folder shared with ${others.length} other project${others.length === 1 ? "" : "s"}`
                    : "Login folder shared by the account's projects"}
                </SharedBadge>
              )}
            </>
          }
          hint={hints[signIn]}
          note={
            signIn === "token"
              ? "Downside: the token only runs the model, so Remote Control and Claude in Chrome don't work, and it has to be renewed after a year."
              : !accounts.length && "Add accounts under Accounts in the settings to sign in with one."
          }
        >
          <Select
            className="h-8 text-[13px]"
            value={signIn}
            disabled={busy}
            onChange={(e) => save(() => api.setProjectSignIn(projectId, e.target.value as AccountSignIn))}
          >
            <option value="folder" disabled={!accounts.length}>
              An account's login (login folder shared with its other projects)
            </option>
            <option value="token" disabled={!accounts.length}>
              An account's sign-in token (token shared with its other projects)
            </option>
            <option value="session">Sign in inside each worktree (nothing shared)</option>
          </Select>
        </Field>
        {signIn !== "session" && (
          <Field
            label="Account"
            hint="A worktree's container switches over at its next session start while no agent runs in it."
          >
            <Select
              className="h-8 text-[13px]"
              value={current}
              disabled={busy}
              onChange={(e) => save(() => api.setProjectAccount(projectId, e.target.value || null))}
            >
              <option value="">{`Default (${label(fallback?.email)})`}</option>
              {accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {label(a.email)}
                </option>
              ))}
            </Select>
          </Field>
        )}
        {signIn === "token" && accountId && (
          <AccountToken accountId={accountId} email={accounts.find((a) => a.id === accountId)?.email ?? null} />
        )}
      </Group>
      <Group title="Files">
        <ClaudeFilesScope projectId={projectId} signIn={signIn} />
      </Group>
      <ErrorText>{error}</ErrorText>
    </>
  );
}

function AccountToken({ accountId, email }: { accountId: string; email: string | null }) {
  const qc = useQueryClient();
  const { data: stored } = useQuery({
    queryKey: keys.accountToken(accountId),
    queryFn: () => api.accountTokenSet(accountId),
  });
  const [value, setValue] = useState("");
  const [getting, setGetting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => qc.invalidateQueries({ queryKey: keys.accountToken(accountId) });

  async function save(token: string | null) {
    setError(null);
    try {
      await api.setAccountToken(accountId, token);
      setValue("");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      await refresh();
    }
  }

  return (
    <Field
      label="Account's sign-in token"
      hint={`Every project on ${email ?? "this account"} that signs in with a token uses it. worki keeps it in the Keychain.`}
    >
      <div className="flex items-center gap-2">
        {stored && <span className="flex-1 text-[13px] text-muted">Stored.</span>}
        <Button variant={stored ? "secondary" : "primary"} onClick={() => setGetting(true)}>
          <KeyRound size={12} />
          {stored ? "Get a new token" : "Get a token"}
        </Button>
        {stored && (
          <Button variant="ghost" onClick={() => save(null)}>
            Remove
          </Button>
        )}
      </div>
      {!stored && (
        <div className="mt-2 flex gap-2">
          <Input
            type="password"
            className="h-8"
            value={value}
            placeholder="Or paste one from claude setup-token"
            onChange={(e) => setValue(e.target.value)}
          />
          <Button disabled={!value.trim()} onClick={() => save(value)}>
            Save
          </Button>
        </div>
      )}
      <ErrorText>{error}</ErrorText>
      {getting && (
        <ClaudeSignInDialog
          title={`Get a sign-in token${email ? ` for ${email}` : ""}`}
          flowId={`token:${accountId}`}
          start={(onEvent) => api.obtainAccountToken(accountId, onEvent)}
          onClose={() => {
            setGetting(false);
            refresh();
          }}
        />
      )}
    </Field>
  );
}

function ClaudeFilesScope({ projectId, signIn }: { projectId: string; signIn: AccountSignIn }) {
  const { data: environment } = useProjectEnvironment(projectId);
  const update = useUpdateProjectEnvironment(projectId);
  if (!environment) return null;
  if (signIn === "session") {
    return (
      <p className="text-[13px] leading-relaxed text-faint">
        Each worktree keeps its own settings, CLAUDE.md, skills, history and memory, together with its login. Its
        folder starts as a copy of the project's settings, without any login.
      </p>
    );
  }

  return (
    <Field
      label={
        <>
          Settings, CLAUDE.md, skills, history and memory
          {environment.claudeFiles === "project" && <SharedBadge>Shared by all worktrees in this project</SharedBadge>}
        </>
      }
      hint={
        <>
          {environment.claudeFiles === "project"
            ? "All the project's worktrees use the same, "
            : "Each worktree gets its own, starting as a copy of the project's, "}
          <button
            type="button"
            className="text-accent hover:underline"
            onClick={() => api.projectClaudeDir(projectId).then(revealItemInDir)}
          >
            kept by worki
          </button>
          . No other project sees them.
        </>
      }
    >
      <Select
        className="h-8 text-[13px]"
        value={environment.claudeFiles}
        onChange={(e) => update.mutate({ claudeFiles: e.target.value as FilesScope })}
      >
        <option value="project">Shared by all worktrees in this project</option>
        <option value="worktree">Separate for each worktree</option>
      </Select>
      <ErrorText>{update.error && errorMessage(update.error)}</ErrorText>
    </Field>
  );
}
