import { useQueryClient } from "@tanstack/react-query";
import { ask } from "@tauri-apps/plugin-dialog";
import { Camera, Eraser, LoaderCircle, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";

import { api, errorMessage, type Snapshot, type SnapshotEngine } from "../lib/api";
import { useSnapshotCleanup, useSnapshots } from "../lib/queries";
import { SnapshotCleanupDialog } from "./SnapshotCleanupDialog";
import { Button, ErrorText, IconButton, PageTitle } from "./ui";

const ENGINES: Record<SnapshotEngine, string> = {
  dockerSandbox: "Docker Sandboxes",
  docker: "Docker",
  podman: "Podman",
  appleContainer: "Apple containers",
};

const created = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

/** A size the way the engines print it, in powers of 1000. */
function formatSize(bytes: number): string {
  const units = ["B", "kB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

const totalSize = (snapshots: Snapshot[]) => {
  const sizes = snapshots.map((s) => s.size).filter((s): s is number => s !== null);
  return sizes.length ? sizes.reduce((a, b) => a + b, 0) : null;
};

/** Saved environment snapshots, what starts from them, and deleting the ones nothing uses. */
export function SnapshotSettings() {
  const { data: snapshots, isFetching, refetch, error: loadError } = useSnapshots();
  const [error, setError] = useState<string | null>(null);
  const { data: cleanup } = useSnapshotCleanup();
  const [editingCleanup, setEditingCleanup] = useState(false);
  const engines = (Object.keys(ENGINES) as SnapshotEngine[]).filter((e) => snapshots?.some((s) => s.engine === e));

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageTitle
        title="Snapshots"
        info="Sandboxes and containers saved with Save snapshot in a worktree's isolation, newest first. Delete the ones no project or worktree starts from anymore to free disk space. Engines that aren't running aren't listed."
      />
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pb-6">
        <div className="flex items-center justify-end gap-3">
          <Button
            onClick={() => setEditingCleanup(true)}
            disabled={cleanup === undefined}
            title="A script that runs inside the sandbox or container before each snapshot is saved"
          >
            <Eraser size={13} />
            Cleanup script…
            <span className="text-faint">{cleanup?.trim() ? "On" : "Off"}</span>
          </Button>
          <IconButton label="Refresh" onClick={() => refetch()}>
            <RefreshCw size={13} className={isFetching ? "animate-spin" : undefined} />
          </IconButton>
        </div>
        {snapshots === undefined ? (
          !loadError && (
            <p className="flex items-center gap-2 text-xs text-muted">
              <LoaderCircle size={12} className="animate-spin" />
              Looking for snapshots…
            </p>
          )
        ) : snapshots.length === 0 ? (
          <p className="text-xs text-muted">No snapshots yet.</p>
        ) : (
          engines.map((engine) => {
            const listed = snapshots.filter((s) => s.engine === engine);
            const total = totalSize(listed);
            return (
              <div key={engine} className="flex flex-col gap-1">
                <h3 className="flex items-baseline gap-2 text-[11px] font-semibold tracking-wide text-faint uppercase">
                  {ENGINES[engine]}
                  {total !== null && <span className="font-normal normal-case tabular-nums">{formatSize(total)}</span>}
                </h3>
                {listed.map((s) => (
                  <SnapshotRow key={s.name} snapshot={s} onError={setError} />
                ))}
              </div>
            );
          })
        )}
        <ErrorText>{error ?? (loadError ? errorMessage(loadError) : null)}</ErrorText>
      </div>
      {editingCleanup && cleanup !== undefined && (
        <SnapshotCleanupDialog script={cleanup} onClose={() => setEditingCleanup(false)} />
      )}
    </section>
  );
}

function SnapshotRow({ snapshot, onError }: { snapshot: Snapshot; onError: (error: string | null) => void }) {
  const qc = useQueryClient();
  const [deleting, setDeleting] = useState(false);
  const used = snapshot.usedBy.length > 0;

  async function remove() {
    const ok = await ask(
      `Delete the snapshot “${snapshot.name}”?\n\nNew environments can't start from it anymore. Existing sandboxes and containers keep what they have. This can't be undone.`,
      { title: "Delete snapshot", kind: "warning", okLabel: "Delete" },
    );
    if (!ok) return;
    setDeleting(true);
    onError(null);
    try {
      await api.deleteSnapshot(snapshot.engine, snapshot.name);
      await qc.invalidateQueries({ queryKey: ["environmentTemplates"] });
    } catch (e) {
      onError(errorMessage(e));
    } finally {
      setDeleting(false);
    }
  }

  return (
    <div className="group flex items-center gap-3 rounded-md px-2 py-1.5 hover:bg-hover">
      <Camera size={14} className="shrink-0 text-muted" />
      <div className="min-w-0 flex-1">
        <div className="truncate font-mono text-[12px]">{snapshot.name}</div>
        <div className="truncate text-[11px] text-faint" title={snapshot.usedBy.join(", ") || undefined}>
          {snapshot.created !== null && `${created.format(new Date(snapshot.created * 1000))} · `}
          {used ? `Used by ${snapshot.usedBy.join(", ")}` : "Not used"}
        </div>
      </div>
      {snapshot.size !== null && (
        <span className="shrink-0 text-[12px] text-muted tabular-nums">{formatSize(snapshot.size)}</span>
      )}
      <IconButton
        label={used ? "Pick another isolation where it's used to delete it" : "Delete snapshot"}
        onClick={remove}
        disabled={used || deleting}
        className="disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted"
      >
        {deleting ? <LoaderCircle size={13} className="animate-spin" /> : <Trash2 size={13} />}
      </IconButton>
    </div>
  );
}
