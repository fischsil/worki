import { useQueryClient } from "@tanstack/react-query";
import { LoaderCircle, RefreshCw } from "lucide-react";
import { useState } from "react";

import { api, errorMessage } from "../lib/api";
import { keys, useIsolationSettings } from "../lib/queries";
import { useKeepAwake } from "../lib/awake";
import { useUi } from "../lib/store";
import { checkForUpdates, useUpdates } from "../lib/updates";
import { Button, Checkbox, ErrorText, Group } from "./ui";

export function GeneralSettings() {
  const qc = useQueryClient();
  const autoUpdate = useUi((s) => s.autoUpdate);
  const setAutoUpdate = useUi((s) => s.setAutoUpdate);
  const updates = useUpdates();
  const reopen = useUi((s) => s.reopenSessions);
  const setReopen = useUi((s) => s.setReopenSessions);
  const [keepAwake, setKeepAwake] = useKeepAwake();
  const { data: isolation } = useIsolationSettings();
  const [error, setError] = useState<string | null>(null);

  async function setStopOnQuit(stopOnQuit: boolean) {
    if (!isolation) return;
    try {
      await api.saveIsolationSettings({ ...isolation, stopOnQuit });
      setError(null);
      await qc.invalidateQueries({ queryKey: keys.isolationSettings });
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <h2 className="px-6 pt-5 pb-3 text-[15px] font-semibold">General</h2>
      <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-6 pb-6">
        <Group title="Power">
          <Checkbox
            checked={keepAwake}
            onChange={setKeepAwake}
            hint="While an agent session is open. The display can still turn off; closing the lid still sleeps. The cup icon at the bottom of the sidebar switches it too."
          >
            <span className="text-[13px]">Keep the Mac awake while agents run</span>
          </Checkbox>
        </Group>

        <Group title="Updates">
          <Checkbox
            checked={autoUpdate}
            onChange={setAutoUpdate}
            hint="Looks for a new worki release on GitHub when worki starts and every few hours. A new release shows an Update button at the bottom of the sidebar."
          >
            <span className="text-[13px]">Check for updates automatically</span>
          </Checkbox>
          <div className="flex items-center gap-3">
            <Button onClick={() => checkForUpdates()} disabled={updates.status !== "idle"}>
              {updates.status === "checking" ? <LoaderCircle size={12} className="animate-spin" /> : <RefreshCw size={12} />}
              Check now
            </Button>
            <span className="text-[13px] text-faint">
              {updates.error
                ? <span className="text-removed">Couldn't check: {updates.error}</span>
                : updates.available
                  ? `worki ${updates.available.version} is available. Use Update at the bottom of the sidebar.`
                  : updates.checkedAt
                    ? "worki is up to date."
                    : null}
            </span>
          </div>
        </Group>

        <Group title="Startup and quit">
          <Checkbox
            checked={reopen}
            onChange={setReopen}
            hint="Claude sessions that were open when worki quit start again and continue their conversations."
          >
            <span className="text-[13px]">Reopen Claude sessions on start</span>
          </Checkbox>
          <Checkbox
            checked={isolation?.stopOnQuit ?? false}
            onChange={setStopOnQuit}
            hint="Off: Docker, Podman and sandbox environments keep running, so sessions start faster next time. On: they're stopped to free memory and started again when a session needs them."
          >
            <span className="text-[13px]">Stop containers when worki quits</span>
          </Checkbox>
          <ErrorText>{error}</ErrorText>
        </Group>
      </div>
    </section>
  );
}
