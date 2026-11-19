import { Plus, Trash2 } from "lucide-react";
import { useState } from "react";

import { api, errorMessage } from "../lib/api";
import { useAgentSettings, useRefreshSecrets, useSecretStatus } from "../lib/queries";
import { Button, ErrorText, IconButton, Input } from "./ui";

/** The secrets in `scope` ("global", or "project:<id>"): names here, values in the Keychain.
 * Saved immediately. */
export function SecretsEditor({ scope }: { scope: string }) {
  const { data: settings } = useAgentSettings();
  const names = settings?.secrets[scope] ?? [];
  const { data: status = [] } = useSecretStatus(scope, names);
  const stored = new Set(status.filter(([, ok]) => ok).map(([n]) => n));
  const accounts = (settings?.accounts.length ?? 0) > 0;

  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);

  const refresh = useRefreshSecrets();

  async function save() {
    setError(null);
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) return setError("Use an environment variable name, e.g. GH_TOKEN.");
    if (!value) return setError("Enter a value.");
    try {
      await api.setSecret(scope, name, value);
      setName("");
      setValue("");
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function remove(n: string) {
    try {
      await api.deleteSecret(scope, n);
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <div className="flex flex-col gap-2">
      {names.map((n) => (
        <div key={n} className="flex flex-col gap-0.5 px-1">
          <div className="flex items-center gap-2">
            <span className="flex-1 font-mono text-xs">{n}</span>
            <span className="text-[11px] text-faint">{stored.has(n) ? "stored" : "no value"}</span>
            <IconButton label={`Remove ${n}`} onClick={() => remove(n)}>
              <Trash2 size={13} />
            </IconButton>
          </div>
          {n === SIGN_IN_TOKEN && accounts && <AccountTokenNote />}
        </div>
      ))}
      <div className="grid grid-cols-[200px_1fr_auto] items-center gap-2 px-1">
        <Input
          value={name}
          onChange={(e) => setName(e.target.value.trim())}
          placeholder="GH_TOKEN"
          className="font-mono text-xs"
        />
        <Input type="password" value={value} onChange={(e) => setValue(e.target.value)} placeholder="Value" />
        <Button onClick={save} disabled={!name || !value}>
          <Plus size={13} />
          Add
        </Button>
      </div>
      {name === SIGN_IN_TOKEN && accounts && !names.includes(name) && <AccountTokenNote />}
      <ErrorText>{error}</ErrorText>
    </div>
  );
}

const SIGN_IN_TOKEN = "CLAUDE_CODE_OAUTH_TOKEN";

function AccountTokenNote() {
  return (
    <p className="text-xs text-faint">
      Claude Code sessions of projects with an account in Docker (sbx), Docker, Podman or Apple containers sign in with the account
      instead.
    </p>
  );
}
