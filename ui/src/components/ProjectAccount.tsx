import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { api, errorMessage, type Isolation, isContainer } from "../lib/api";
import { keys, useAgentSettings } from "../lib/queries";
import { ErrorText, Field, Select } from "./ui";

/** The Claude Code account the project's agents use, from those added in the Agents settings. */
export function ProjectAccount({ projectId, isolation }: { projectId: string; isolation: Isolation }) {
  const qc = useQueryClient();
  const { data: settings } = useAgentSettings();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const accounts = settings?.accounts ?? [];
  const current = settings?.projectAccounts[projectId] ?? "";
  const label = (email: string | null | undefined) => email ?? "not signed in";
  const fallback = accounts.find((a) => a.id === settings?.defaultAccount);

  async function choose(accountId: string) {
    setBusy(true);
    setError(null);
    try {
      await api.setProjectAccount(projectId, accountId || null);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
      await qc.invalidateQueries({ queryKey: keys.agentSettings });
    }
  }

  const hint = !accounts.length
    ? "Add accounts under Agents in the settings; until then sessions use the isolation's own sign-in."
    : !isContainer(isolation)
      ? "Only Docker Sandboxes, Docker and Podman sessions use it; this project's keep their own sign-in."
      : "Sessions sign in as this account. A worktree's container switches over at its next session start while no agent runs in it.";

  return (
    <>
      <Field label="Claude Code account" hint={hint}>
        <Select
          className="h-8 text-[13px]"
          value={current}
          disabled={busy || !accounts.length}
          onChange={(e) => choose(e.target.value)}
        >
          <option value="">{accounts.length ? `Default (${label(fallback?.email)})` : "No accounts"}</option>
          {accounts.map((a) => (
            <option key={a.id} value={a.id}>
              {label(a.email)}
            </option>
          ))}
        </Select>
      </Field>
      <ErrorText>{error}</ErrorText>
    </>
  );
}
