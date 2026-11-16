import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { api, errorMessage, isContainer, type Project, type Worktree, worktreeLabel } from "../lib/api";
import { keys, useSessions, useWorktrees } from "../lib/queries";
import { toast } from "../lib/toast";
import { Button, Checkbox, Dialog, Field, Input, WarnText } from "./ui";
import { useEffectiveIsolation } from "./WorktreeIsolation";

export function SaveEnvironmentDialog({
  project,
  path,
  onClose,
}: {
  project: Project;
  path: string;
  onClose: () => void;
}) {
  const { data: worktrees } = useWorktrees(project.id);
  const worktree = worktrees?.find((w) => w.path === path);
  return worktree ? <SaveEnvironmentForm project={project} worktree={worktree} onClose={onClose} /> : null;
}

function defaultName(project: Project): string {
  const slug = project.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "project";
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `worki-${slug}:${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}`;
}

function SaveEnvironmentForm({ project, worktree, onClose }: { project: Project; worktree: Worktree; onClose: () => void }) {
  const qc = useQueryClient();
  const isolation = useEffectiveIsolation(project, worktree);
  const { data: sessions = [] } = useSessions();
  const [name, setName] = useState(() => defaultName(project));
  const [useForProject, setUseForProject] = useState(true);

  const sandbox = isolation?.kind === "dockerSandbox";
  const noun = sandbox ? "sandbox" : "container";
  const tabs = sessions.filter((s) => s.worktreePath === worktree.path && s.state.kind === "running").length;
  const label = worktreeLabel(worktree);

  async function save() {
    const tag = name.trim();
    onClose();
    try {
      await api.saveEnvironmentTemplate(project.id, worktree.path, tag, useForProject);
      toast(
        useForProject
          ? `Saved snapshot ${tag}. New worktrees in ${project.name} start from it.`
          : `Saved snapshot ${tag}. Pick it under Isolation › Start from to use it.`,
        "info",
      );
    } catch (e) {
      toast(`Couldn't save snapshot ${tag}: ${errorMessage(e)}`);
    } finally {
      qc.invalidateQueries({ queryKey: keys.projects });
      qc.invalidateQueries({ queryKey: keys.sessions });
      qc.invalidateQueries({ queryKey: keys.worktreeIsolation(worktree.path) });
      qc.invalidateQueries({ queryKey: ["environmentTemplates"] });
    }
  }

  if (isolation && !isContainer(isolation)) {
    return (
      <Dialog title="Save environment snapshot" onClose={onClose} width={480} footer={<Button onClick={onClose}>Close</Button>}>
        <p className="text-[13px] leading-relaxed">
          Only worktrees running in a Docker Sandbox, Docker or Podman container have an environment that can be saved.
          Change the isolation of this worktree or project to one of those first.
        </p>
      </Dialog>
    );
  }

  return (
    <Dialog
      title="Save environment snapshot"
      onClose={onClose}
      width={520}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={save} disabled={!name.trim() || !isolation}>
            Save snapshot
          </Button>
        </>
      }
    >
      <p className="text-[13px] leading-relaxed">
        Saves <span className="font-semibold">{label}</span>'s {noun} as an image, with everything installed in it.
        New worktrees start from this image instead of the default one.
      </p>
      <p className="text-xs text-muted">
        Your code isn't included. Sign-ins are (Codex's login, for example), so keep it to yourself.
      </p>
      <Field label="Snapshot name">
        <Input className="font-mono text-xs" value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Checkbox checked={useForProject} onChange={setUseForProject}>
        <span className="text-[13px]">Start new worktrees in {project.name} from this snapshot</span>
      </Checkbox>
      {sandbox && tabs > 0 && (
        <WarnText>
          Saving stops the sandbox and ends its {tabs} running {tabs === 1 ? "tab" : "tabs"}.
        </WarnText>
      )}
    </Dialog>
  );
}
