import { useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorMessage, isolationLabel, type Project, type ProjectIsolation } from "../lib/api";
import { keys, useBranches, useProjectDefaultIsolation, useSetupScripts } from "../lib/queries";
import { useUi } from "../lib/store";
import { toast } from "../lib/toast";
import { BranchPicker } from "./BranchPicker";
import { IsolationPicker } from "./IsolationPicker";
import { Button, Checkbox, Dialog, Field, Input } from "./ui";

export function NewWorktreeDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const qc = useQueryClient();
  const select = useUi((s) => s.select);
  const { data: branches = [] } = useBranches(project.id);
  const { data: scripts } = useSetupScripts(project.id);
  const setupScript = scripts?.project?.trim() || scripts?.repoFile?.trim() || null;

  const [name, setName] = useState("");
  const [base, setBase] = useState(project.defaultBase);
  const [fetching, setFetching] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);

  async function fetchLatest() {
    setFetching(true);
    setFetchError(null);
    try {
      await api.fetchRemote(project.repoPath);
      await qc.invalidateQueries({ queryKey: keys.branches(project.id) });
    } catch (e) {
      setFetchError(errorMessage(e));
    } finally {
      setFetching(false);
    }
  }
  const [runSetup, setRunSetup] = useState(true);
  const [isolation, setIsolation] = useState<ProjectIsolation>({ kind: "inherit" });
  const projectDefault = useProjectDefaultIsolation(project);

  const branch = branchName(name);
  const nameTaken = branches.includes(branch);
  const localBranches = branches.filter((b) => !b.startsWith("origin/"));
  const canSubmit = !!branch && !nameTaken && !!base;

  useEffect(() => {
    if (branches.length > 0 && !branches.includes(base)) setBase(localBranches[0] ?? branches[0]);
  }, [branches]);

  /** Closes at once: the Activity panel and the worktree's row show how it goes. */
  async function submit() {
    if (!canSubmit) return;
    onClose();
    try {
      const res = await api.createWorktree(project.id, { name, base, runSetup: runSetup && !!setupScript, isolation });
      await qc.invalidateQueries({ queryKey: keys.worktrees(project.id) });
      qc.invalidateQueries({ queryKey: keys.branches(project.id) });
      select(project.id, res.worktree.path);
      if (res.setupError) toast(`Worktree ${branch} was created, but its setup script failed. See Activity for its output.`);
    } catch (e) {
      toast(`Couldn't create worktree ${branch}: ${errorMessage(e)}`);
    }
  }

  return (
    <Dialog
      title={`New worktree · ${project.name}`}
      onClose={onClose}
      width={520}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit} disabled={!canSubmit}>
            Create worktree
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <Field label="Start from" note={fetchError && <span className="text-removed">Couldn't fetch: {fetchError}</span>}>
          <div className="flex gap-2">
            <div className="min-w-0 flex-1">
              <BranchPicker
                value={base}
                branches={branches.length > 0 ? branches : [base]}
                onChange={setBase}
              />
            </div>
            <Button
              type="button"
              onClick={fetchLatest}
              disabled={fetching}
              title="Fetch the latest branches from the remote"
              className="h-8"
            >
              <RefreshCw size={12} className={fetching ? "animate-spin" : undefined} />
              {fetching ? "Fetching…" : "Fetch"}
            </Button>
          </div>
        </Field>
        <Field
          label="Worktree name"
          note={
            nameTaken ? (
              <span className="text-removed">A branch named “{branch}” already exists.</span>
            ) : branch ? (
              <span className="flex flex-col gap-0.5">
                <span>
                  New branch <span className="font-mono text-muted">{branch}</span> from{" "}
                  <span className="font-mono text-muted">{base}</span>
                </span>
                <span className="truncate">
                  Folder <span className="font-mono text-muted">{tildify(`${project.worktreeRoot}/${slug(branch)}`)}</span>
                </span>
              </span>
            ) : (
              "Also used as the branch name."
            )
          }
        >
          <Input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="feature/my-change"
          />
        </Field>
        {!project.remote && (
          <IsolationPicker
            label="Isolation"
            value={isolation}
            onChange={setIsolation}
            allowInherit
            inheritName="Project default"
            inheritLabel={projectDefault ? isolationLabel(projectDefault) : undefined}
          />
        )}
        <Checkbox checked={runSetup && !!setupScript} onChange={setRunSetup} disabled={!setupScript}>
          <span className="flex flex-col gap-0.5">
            <span>Run setup script</span>
            <span className="text-xs text-faint">
              {setupScript
                ? scripts?.project?.trim()
                  ? "From project settings"
                  : "From .worki.toml"
                : "No setup script configured"}
            </span>
          </span>
        </Checkbox>
        <button type="submit" hidden />
      </form>

    </Dialog>
  );
}

/** Mirrors `paths::branch_name` in worki-core. */
function branchName(name: string): string {
  return name.trim().split(/\s+/).filter(Boolean).join("-");
}

/** Mirrors `paths::slug` in worki-core. */
function slug(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[^a-z0-9._]+/g, "-")
    .replace(/^[-.]+|[-.]+$/g, "");
  return s || "worktree";
}

function tildify(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}
