import { useQueryClient } from "@tanstack/react-query";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Bot, FolderOpen, KeyRound, Settings, Shield, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import {
  api,
  errorMessage,
  type Isolation,
  isContainer,
  isolationLabel,
  type Project,
  type ProjectIsolation,
  projectIsolation,
  type ProjectPatch,
  projectScope,
} from "../lib/api";
import {
  keys,
  useIsolationSettings,
  useProjectEnvironment,
  useSetupScripts,
  useSshPasswordSet,
  useUpdateProjectEnvironment,
} from "../lib/queries";
import { useUi } from "../lib/store";
import { toast } from "../lib/toast";
import { AgentsPanel } from "./AgentsPanel";
import { DoctorPanel } from "./DoctorPanel";
import { IsolationPicker } from "./IsolationPicker";
import { ProjectAccount } from "./ProjectAccount";
import { parseRemote, RemoteFields, remoteForm } from "./RemoteFields";
import { SecretsEditor } from "./SecretsEditor";
import { type SettingsPage, SettingsWindow } from "./SettingsWindow";
import {
  Button,
  Checkbox,
  ErrorText,
  Field,
  Group,
  InfoText,
  Input,
  PageTitle,
  SharedBadge,
  Textarea,
} from "./ui";

type Page = "general" | "isolation" | "claude" | "secrets";

/** A project's settings, in pages, saved as they change: fields when they lose focus, and
 * whatever is left when the dialog closes. */
export function ProjectSettingsDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const qc = useQueryClient();
  const { selectedProjectId, select } = useUi();
  const { data: scripts } = useSetupScripts(project.id);
  const { data: isolationSettings } = useIsolationSettings();
  const globalIsolation: Isolation = isolationSettings?.default ?? { kind: "none" };

  const [name, setName] = useState(project.name);
  const [worktreeRoot, setWorktreeRoot] = useState(project.worktreeRoot);
  const [setupScript, setSetupScript] = useState(project.setupScript ?? "");
  const [isolation, setIsolation] = useState<ProjectIsolation>(project.isolation);
  const [form, setForm] = useState(() => remoteForm(project.remote));
  const parsed = project.remote ? parseRemote(form) : null;
  const remote = parsed && typeof parsed !== "string" ? parsed : project.remote;
  const { data: passwordStored = false, refetch: refetchPassword } = useSshPasswordSet(project.remote);
  const [error, setError] = useState<string | null>(null);
  const effective = projectIsolation({ ...project, isolation, remote }, globalIsolation);
  const [page, setPage] = useState<Page>("general");
  const { data: environment } = useProjectEnvironment(project.id);
  const updateEnvironment = useUpdateProjectEnvironment(project.id);
  const isolationChanged = JSON.stringify(isolation) !== JSON.stringify(project.isolation);

  /** Saves what differs from the stored project; true when nothing is left unsaved. */
  async function saveChanges(): Promise<boolean> {
    const patch: ProjectPatch = {};
    if (name.trim() && name.trim() !== project.name) patch.name = name;
    if (worktreeRoot !== project.worktreeRoot) patch.worktreeRoot = worktreeRoot;
    if (setupScript !== (project.setupScript ?? "")) patch.setupScript = setupScript;
    if (isolationChanged) {
      if (isolation.kind === "macUser" && !isolation.user) {
        setError("Enter a user name.");
        return false;
      }
      patch.isolation = isolation;
    }
    if (typeof parsed === "string") {
      setError(parsed);
      return false;
    }
    if (parsed && JSON.stringify(parsed) !== JSON.stringify(project.remote)) patch.remote = parsed;
    try {
      if (Object.keys(patch).length) {
        await api.updateProject(project.id, patch);
        await qc.invalidateQueries({ queryKey: keys.projects });
        if (patch.setupScript !== undefined) qc.invalidateQueries({ queryKey: keys.setupScripts(project.id) });
      }
      if (parsed && form.password) {
        await api.setSshPassword(parsed, form.password);
        setForm((f) => ({ ...f, password: "" }));
        refetchPassword();
      }
      setError(null);
      return true;
    } catch (e) {
      setError(errorMessage(e));
      return false;
    }
  }

  async function close() {
    if (await saveChanges()) onClose();
  }

  useEffect(() => {
    if (!isolationChanged) return;
    const timer = setTimeout(saveChanges, 500);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isolation]);


  async function removeProject() {
    const ok = await ask(
      `Remove “${project.name}” from worki? Its sessions are closed and its isolation is cleaned up (agent containers or sandboxes deleted, agent-user access removed). The repository and its worktrees stay on disk.`,
      { title: "Remove project", kind: "warning", okLabel: "Remove" },
    );
    if (!ok) return;
    if (selectedProjectId === project.id) select("", null);
    onClose();
    try {
      const warning = await api.removeProject(project.id);
      if (warning) await message(warning, { title: "Project removed", kind: "warning" });
    } catch (e) {
      toast(`Couldn't remove ${project.name}: ${errorMessage(e)}`);
    } finally {
      qc.invalidateQueries({ queryKey: keys.projects });
    }
  }

  const pages: (SettingsPage<Page> & { info?: string })[] = [
    { id: "general", label: "General", icon: <Settings size={14} /> },
    { id: "isolation", label: "Isolation", icon: <Shield size={14} /> },
    ...(remote ? [] : [{ id: "claude" as const, label: "Claude Code", icon: <Bot size={14} /> }]),
    {
      id: "secrets",
      label: "Secrets",
      icon: <KeyRound size={14} />,
      info: "This project's agents get these as environment variables, along with the global secrets from Settings. One with the same name as a global secret wins. Stored in your Keychain.",
    },
  ];

  const current = pages.find((p) => p.id === page);

  return (
    <SettingsWindow
      label="Project settings"
      heading={project.name}
      pages={pages}
      current={page}
      onSelect={setPage}
      onClose={close}
      navFooter={
        <button
          onClick={removeProject}
          className="flex h-8 items-center gap-2 rounded-md px-2 text-left text-[13px] text-removed hover:bg-hover"
        >
          <Trash2 size={14} />
          Remove project…
        </button>
      }
    >
      <section className="flex min-h-0 min-w-0 flex-1 flex-col">
        <PageTitle title={current?.label ?? ""} info={current?.info} />
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pb-6">
          {page === "general" && (
            <div className="contents" onBlur={() => saveChanges()}>
              <Field label="Name">
                <Input value={name} onChange={(e) => setName(e.target.value)} />
              </Field>
              <Field label={remote ? "Repository on the host" : "Repository"}>
                <Input value={project.repoPath} readOnly className="text-muted" />
              </Field>
              {project.remote && (
                <RemoteFields
                  value={form}
                  onChange={setForm}
                  passwordStored={passwordStored}
                  onRemovePassword={async () => {
                    await api.setSshPassword(project.remote!, null).catch((e) => setError(errorMessage(e)));
                    refetchPassword();
                  }}
                />
              )}
              <Field label="Worktree folder" hint="New worktrees are created in <folder>/<branch>.">
                <Input value={worktreeRoot} onChange={(e) => setWorktreeRoot(e.target.value)} />
              </Field>
              <Field
                label="Setup script"
                note={
                  scripts?.repoFile &&
                  (setupScript.trim()
                    ? "Overrides the script in .worki.toml."
                    : "Empty — the script from .worki.toml is used.")
                }
                hint="Runs with zsh, in the worktree's isolation. $WORKI_ROOT_PATH, $WORKI_WORKTREE_PATH and $WORKI_BRANCH are set, and $WORKI_PROJECT_DIR when the project folder is on."
              >
                <Textarea
                  rows={8}
                  className="font-mono text-[12px]"
                  value={setupScript}
                  onChange={(e) => setSetupScript(e.target.value)}
                  placeholder={scripts?.repoFile ?? 'cp "$WORKI_ROOT_PATH/.env" .env\npnpm install'}
                />
              </Field>
            </div>
          )}
          {page === "isolation" && (
            <>
              {!remote && (
                <IsolationPicker
                  label="New worktrees use"
                  value={isolation}
                  onChange={setIsolation}
                  allowInherit
                  inheritLabel={isolationLabel(globalIsolation)}
                />
              )}
              {effective.kind !== "none" && (
                <DoctorPanel key={JSON.stringify(effective)} isolation={effective} projectId={project.id} />
              )}
              {project.remote && (
                <AgentsPanel key={JSON.stringify(project.remote)} isolation={{ kind: "ssh", ...project.remote }} />
              )}
              {!remote && (
                <Group title="Project folder">
                  <div className="flex flex-col gap-1.5">
                    <div className="flex items-center gap-2">
                      <Checkbox
                        checked={environment?.sharedFolder ?? false}
                        onChange={(sharedFolder) => updateEnvironment.mutate({ sharedFolder })}
                        hint={
                          <>
                            Mounted at <code>/worki/project</code> (<code>$WORKI_PROJECT_DIR</code>) in every
                            worktree's container, for a build cache, say. Containers get it at their next start.
                          </>
                        }
                      >
                        Share a folder between the project's worktrees
                      </Checkbox>
                      {environment?.sharedFolder && <SharedBadge>Shared by all worktrees in this project</SharedBadge>}
                      {environment?.sharedFolder && (
                        <Button
                          variant="ghost"
                          className="ml-auto"
                          onClick={() => api.projectSharedDir(project.id).then(revealItemInDir)}
                        >
                          <FolderOpen size={12} />
                          Show in Finder
                        </Button>
                      )}
                    </div>
                    {!isContainer(effective) && (
                      <InfoText>Only Docker Sandboxes, Docker and Podman containers get it.</InfoText>
                    )}
                  </div>
                </Group>
              )}
            </>
          )}
          {page === "claude" && <ProjectAccount projectId={project.id} isolation={effective} />}
          {page === "secrets" && <SecretsEditor scope={projectScope(project.id)} />}
          <ErrorText>{error ?? (updateEnvironment.error && errorMessage(updateEnvironment.error))}</ErrorText>
        </div>
      </section>
    </SettingsWindow>
  );
}
