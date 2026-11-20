import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import clsx from "clsx";
import {
  ArrowDownToLine,
  BellRing,
  Check,
  CircleCheck,
  CircleX,
  Coffee,
  ChevronRight,
  X,
  MessageSquare,
  FolderGit2,
  GitBranch,
  GitPullRequest,
  House,
  ListFilter,
  LoaderCircle,
  Pencil,
  Plus,
  RefreshCw,
  Server,
  Settings,
  Settings2,
  UserRound,
} from "lucide-react";
import { useRef, useState } from "react";

import {
  api,
  errorMessage,
  type OpenPullRequest,
  type Project,
  projectAccountId,
  projectOfWorktree,
  type PullRequest,
  type Worktree,
  worktreeLabel,
} from "../lib/api";
import { useWorktreeComments } from "../lib/review";
import {
  keys,
  useAddProject,
  useAgentSettings,
  useIsolationSettings,
  useOpenPullRequests,
  useProjects,
  usePullRequest,
  useReorderProjects,
  useSessions,
  useWorktrees,
  useWorktreeStatus,
} from "../lib/queries";
import { lastStep, useWorktreeActivity } from "../lib/activities";
import { useKeepAwake } from "../lib/awake";
import { useUi } from "../lib/store";
import { plural } from "../lib/text";
import { toast } from "../lib/toast";
import { installUpdate, useUpdates } from "../lib/updates";
import { ActivityDot, summarize } from "./ActivityDot";
import { ActivityButton } from "./ActivityPanel";
import { PrStateIcon } from "./BranchPanel";
import { ResizeHandle } from "./ResizeHandle";
import { ContextMenu, ErrorText, Hint, IconButton, Menu } from "./ui";
import { worktreeActions } from "./worktreeActions";

export function Sidebar() {
  const { data: allProjects = [] } = useProjects();
  const filter = useAccountFilter(allProjects);
  const projects = filter.visible;
  const { navRef, dragging, drop, dragProps } = useProjectReorder(projects, allProjects);
  const addProject = useAddProject();
  const openDialog = useUi((s) => s.openDialog);
  const width = useUi((s) => s.panelWidths.sidebar);
  const rounded = useUi((s) => s.rounded);

  return (
    <aside className="fs-sidebar relative flex h-full shrink-0 flex-col bg-sidebar" style={{ width }}>
      <ResizeHandle panel="sidebar" edge="right" />
      <div data-tauri-drag-region className="h-13 shrink-0" />
      <div className="flex items-center justify-between px-3 pb-1">
        <span className="fs-sidebar-xs font-semibold tracking-wide text-faint uppercase">Projects</span>
        <div className="flex items-center">
          {filter.hasAccounts && (
            <Menu
              trigger={(toggle) => (
                <IconButton label={filter.active ? "Showing one account's projects" : "Filter by account"} onClick={toggle}>
                  <ListFilter size={14} className={clsx(filter.active && "text-accent")} />
                </IconButton>
              )}
              items={filter.items}
            />
          )}
          <Menu
            trigger={(toggle) => (
              <IconButton label="Add project" onClick={toggle}>
                <Plus size={15} />
              </IconButton>
            )}
            items={[
              { label: "Add local repository…", icon: <FolderGit2 size={13} />, onSelect: () => addProject.mutate() },
              {
                label: "Add remote repository (SSH)…",
                icon: <Server size={13} />,
                onSelect: () => openDialog({ kind: "addRemoteProject" }),
              },
            ]}
          />
        </div>
      </div>
      {addProject.error && (
        <div className="px-3 pb-2">
          <ErrorText>{String(addProject.error)}</ErrorText>
        </div>
      )}
      <nav ref={navRef} className="flex-1 overflow-y-auto px-2 pb-3">
        <NeedsYou projects={allProjects} shown={projects} />
        {projects.map((p, i) => (
          <ProjectNode
            key={p.id}
            project={p}
            dragging={dragging === p.id}
            dropBefore={drop === i}
            dropAfter={drop === projects.length && i === projects.length - 1}
            dragProps={dragProps(p.id)}
          />
        ))}
        {projects.length === 0 && (
          <p className="px-2 py-4 fs-sidebar-xs leading-relaxed text-faint">
            {allProjects.length === 0
              ? "No projects yet. Add a git repository to start managing its worktrees."
              : "No projects use this account."}
          </p>
        )}
      </nav>
      {!rounded && <StatusBar />}
    </aside>
  );
}

export function StatusBar({ fullWidth = false }: { fullWidth?: boolean }) {
  const openDialog = useUi((s) => s.openDialog);
  return (
    <div
      className={clsx(
        "fs-sidebar relative flex shrink-0 items-center border-t border-line bg-sidebar px-2",
        fullWidth ? "py-0.5" : "py-1.5",
      )}
    >
      <button
        onClick={() => openDialog({ kind: "settings", tab: "general" })}
        title="Settings (⌘,)"
        className="flex h-7 items-center gap-2 rounded-md px-2 text-muted hover:bg-hover hover:text-fg"
      >
        <Settings size={14} />
        Settings
      </button>
      <KeepAwakeButton />
      {fullWidth ? (
        <div className="ml-auto flex min-w-0 items-center gap-1">
          <UpdateButton />
          <ActivityButton fromRight />
        </div>
      ) : (
        <>
          <ActivityButton />
          <UpdateButton />
        </>
      )}
    </div>
  );
}

/** The email of the Claude Code account the project's agents use, when its isolation uses one. */
function useProjectAccount(project: Project): string | null {
  const { data: agents } = useAgentSettings();
  const { data: isolation } = useIsolationSettings();
  if (!agents || !isolation) return null;
  const id = projectAccountId(project, agents, isolation);
  const account = agents.accounts.find((a) => a.id === id);
  return account ? (account.email ?? "not signed in") : null;
}

/** The filter's value for projects whose agents use no account. */
const NO_ACCOUNT = "";

/** The projects the sidebar's account filter lets through, with the filter's menu items. */
function useAccountFilter(projects: Project[]) {
  const { data: agents } = useAgentSettings();
  const { data: isolation } = useIsolationSettings();
  const filter = useUi((s) => s.accountFilter);
  const setFilter = useUi((s) => s.setAccountFilter);
  if (!agents || !isolation || agents.accounts.length === 0) {
    return { visible: projects, active: false, items: [], hasAccounts: false };
  }
  const accountOfProject = new Map(
    projects.map((p) => [p, projectAccountId(p, agents, isolation) ?? NO_ACCOUNT] as const),
  );
  const known = filter === NO_ACCOUNT || agents.accounts.some((a) => a.id === filter);
  const selected = known ? filter : null;
  const visible = selected === null ? projects : projects.filter((p) => accountOfProject.get(p) === selected);
  const choice = (value: string | null, label: string) => ({
    label,
    icon: selected === value ? <Check size={13} /> : undefined,
    onSelect: () => setFilter(value),
  });
  const items = [
    choice(null, "All accounts"),
    ...agents.accounts.map((a) => choice(a.id, a.email ?? "Not signed in")),
    ...([...accountOfProject.values()].includes(NO_ACCOUNT) ? [choice(NO_ACCOUNT, "Without account")] : []),
  ];
  return { visible, active: selected !== null, items, hasAccounts: true };
}

/** Reorders the shown `projects` by dragging; projects the filter hides keep their places in `all`. */
function useProjectReorder(projects: Project[], all: Project[]) {
  const reorder = useReorderProjects();
  const [dragging, setDragging] = useState<string | null>(null);
  const [drop, setDrop] = useState<number | null>(null);
  const endedDrag = useRef(false);
  const navRef = useRef<HTMLElement>(null);

  function startDrag(e: React.PointerEvent<HTMLElement>, id: string) {
    const nav = navRef.current;
    if (e.button !== 0 || !nav) return;
    const header = e.currentTarget;
    const { pointerId, clientY: startY } = e;
    const from = projects.findIndex((p) => p.id === id);
    let middles: number[] | null = null;
    let at: number | null = null;
    endedDrag.current = false;

    const move = (ev: PointerEvent) => {
      if (!middles) {
        if (Math.abs(ev.clientY - startY) < 4) return;
        header.setPointerCapture(pointerId);
        middles = [...nav.querySelectorAll<HTMLElement>("[data-project]")].map((n) => {
          const rect = n.getBoundingClientRect();
          return rect.top + rect.height / 2 + nav.scrollTop;
        });
        setDragging(id);
        document.body.style.cursor = "grabbing";
      }
      const y = ev.clientY + nav.scrollTop;
      const below = middles.findIndex((m) => y < m);
      const index = below === -1 ? middles.length : below;
      at = index === from || index === from + 1 ? null : index;
      setDrop(at);
    };

    const end = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      if (!middles) return;
      endedDrag.current = true;
      document.body.style.cursor = "";
      setDragging(null);
      setDrop(null);
      if (ev.type !== "pointerup" || at === null) return;
      const reordered = projects.map((p) => p.id);
      reordered.splice(from, 1);
      reordered.splice(at > from ? at - 1 : at, 0, id);
      let i = 0;
      reorder.mutate(all.map((p) => (projects.includes(p) ? reordered[i++] : p.id)));
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  const dragProps = (id: string) => ({
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => startDrag(e, id),
    onClickCapture: (e: React.MouseEvent) => {
      if (endedDrag.current) e.stopPropagation();
      endedDrag.current = false;
    },
  });

  return { navRef, dragging, drop, dragProps };
}

function DropLine({ className }: { className: string }) {
  return <div className={clsx("pointer-events-none absolute inset-x-1 z-10 h-0.5 rounded-full bg-removed", className)} />;
}

function ProjectNode({
  project,
  dragging,
  dropBefore,
  dropAfter,
  dragProps,
}: {
  project: Project;
  dragging: boolean;
  dropBefore: boolean;
  dropAfter: boolean;
  dragProps: React.HTMLAttributes<HTMLDivElement>;
}) {
  const collapsed = useUi((s) => !!s.collapsed[project.id]);
  const toggle = useUi((s) => s.toggleCollapsed);
  const openDialog = useUi((s) => s.openDialog);
  const { data: worktrees = [], error } = useWorktrees(project.id);
  const account = useProjectAccount(project);
  const showPullRequests = useUi((s) => s.showPullRequests);
  const { data: prs = [] } = useOpenPullRequests(project.id, showPullRequests);
  const openPrs = showPullRequests ? prs : [];
  const prBranches = new Set(openPrs.map((pr) => pr.branch));
  const unassigned = worktrees.filter((w) => w.isMain || !w.branch || !prBranches.has(w.branch));
  const qc = useQueryClient();
  const [fetching, setFetching] = useState(false);

  async function fetchAll() {
    setFetching(true);
    try {
      await api.fetchRemote(project.repoPath);
      await qc.invalidateQueries({
        predicate: ({ queryKey: [kind] }) =>
          [
            "branches",
            "compareBase",
            "branchLog",
            "branchChanges",
            "status",
            "pullRequest",
            "openPullRequests",
          ].includes(kind as string),
      });
    } catch (e) {
      toast(`Couldn't fetch ${project.name}: ${errorMessage(e)}`);
    } finally {
      setFetching(false);
    }
  }

  return (
    <div data-project={project.id} className={clsx("relative mb-4", dragging && "opacity-50")}>
      {dropBefore && <DropLine className="-top-2" />}
      {dropAfter && <DropLine className="-bottom-2" />}
      <div
        {...dragProps}
        className="group flex h-7 items-center gap-1 rounded-md pr-1 pl-1 select-none hover:bg-hover"
      >
        <button className="flex min-w-0 flex-1 items-center gap-1" onClick={() => toggle(project.id)}>
          <ChevronRight size={13} className={clsx("shrink-0 text-faint transition-transform", !collapsed && "rotate-90")} />
          {project.remote ? (
            <Hint title={`Remote repository on ${project.remote.host}`} className="text-muted">
              <Server size={14} />
            </Hint>
          ) : (
            <Hint title="Repository on this Mac" className="text-muted">
              <FolderGit2 size={14} />
            </Hint>
          )}
          <span className="max-w-[75%] shrink-0 truncate font-semibold" title={project.remote ? `${project.remote.host}:${project.repoPath}` : project.repoPath}>
            {project.name}
          </span>
          {account && (
            <span
              className="flex min-w-0 flex-1 items-center justify-end gap-1 pr-1 fs-sidebar-xs text-faint group-hover:hidden group-has-disabled:hidden"
              title={`Claude Code account: ${account}`}
            >
              <UserRound size={11} className="shrink-0" />
              <span className="truncate">{account}</span>
            </span>
          )}
        </button>
        <div className={clsx("flex", account ? "hidden group-hover:flex has-disabled:flex" : "opacity-0 group-hover:opacity-100 has-disabled:opacity-100")}>
          <IconButton label="Fetch (git fetch --all)" onClick={fetchAll} disabled={fetching}>
            <RefreshCw size={13} className={clsx(fetching && "animate-spin")} />
          </IconButton>
          <IconButton label="New worktree" onClick={() => openDialog({ kind: "newWorktree", projectId: project.id })}>
            <Plus size={14} />
          </IconButton>
          <IconButton label="Project settings" onClick={() => openDialog({ kind: "projectSettings", projectId: project.id })}>
            <Settings2 size={13} />
          </IconButton>
        </div>
      </div>
      {!collapsed && (
        <div className="mt-1 flex flex-col gap-0.5">
          {error && <p className="px-7 py-1 fs-sidebar-xs text-removed">{String(error)}</p>}
          {unassigned.map((wt) => (
            <WorktreeRow key={wt.path} project={project} worktree={wt} />
          ))}
          <button
            onClick={() => openDialog({ kind: "newWorktree", projectId: project.id })}
            className="flex h-7 items-center gap-2 rounded-md pl-7 text-left text-muted hover:bg-hover hover:text-fg"
          >
            <Plus size={13} />
            New worktree
          </button>
          <PullRequestSection
            id="mine"
            title="My pull requests"
            project={project}
            worktrees={worktrees}
            prs={openPrs.filter((pr) => pr.authored)}
          />
          <PullRequestSection
            id="reviewing"
            title="Reviewing"
            project={project}
            worktrees={worktrees}
            prs={openPrs.filter((pr) => !pr.authored)}
          />
        </div>
      )}
    </div>
  );
}

/** Pull requests with a worktree show as one; the others are greyed out until clicked. */
function PullRequestSection({
  id,
  title,
  project,
  worktrees,
  prs,
}: {
  id: string;
  title: string;
  project: Project;
  worktrees: Worktree[];
  prs: OpenPullRequest[];
}) {
  const key = `${project.id}:${id}`;
  const collapsed = useUi((s) => !!s.collapsed[key]);
  const toggle = useUi((s) => s.toggleCollapsed);
  if (prs.length === 0) return null;
  return (
    <>
      <button
        onClick={() => toggle(key)}
        className="mt-3 flex items-center gap-1 pb-1 pl-3.5 text-left fs-sidebar-xs font-semibold tracking-wide text-faint uppercase hover:text-muted"
      >
        <ChevronRight size={11} className={clsx("shrink-0 transition-transform", !collapsed && "rotate-90")} />
        {title} ({prs.length})
      </button>
      {!collapsed &&
        prs.map((pr) => {
          const worktree = worktrees.find((w) => !w.isMain && w.branch === pr.branch);
          return worktree ? (
            <WorktreeRow key={pr.number} project={project} worktree={worktree} openPr={pr} />
          ) : (
            <OpenPullRequestRow key={pr.number} project={project} pr={pr} />
          );
        })}
    </>
  );
}

function OpenPullRequestRow({ project, pr }: { project: Project; pr: OpenPullRequest }) {
  const qc = useQueryClient();
  const select = useUi((s) => s.select);
  const [creating, setCreating] = useState(false);

  async function create() {
    setCreating(true);
    try {
      const res = await api.createPrWorktree(project.id, { number: pr.number, branch: pr.branch, base: pr.base });
      await qc.invalidateQueries({ queryKey: keys.worktrees(project.id) });
      qc.invalidateQueries({ queryKey: keys.branches(project.id) });
      select(project.id, res.worktree.path);
      if (res.setupError) toast(`Worktree ${pr.branch} was created, but its setup script failed. See Activity for its output.`);
    } catch (e) {
      toast(`Couldn't create a worktree for #${pr.number}: ${errorMessage(e)}`);
    } finally {
      setCreating(false);
    }
  }

  return (
    <div
      role="button"
      onClick={() => !creating && create()}
      title={`${pr.title}${pr.authored ? "" : ` by ${pr.author}`}\nClick to create a worktree`}
      className={clsx(
        "flex h-7 cursor-default items-center gap-2 rounded-md pr-1 pl-7 text-faint hover:bg-hover hover:text-muted",
        creating && "opacity-70",
      )}
    >
      {creating ? (
        <LoaderCircle size={13} className="shrink-0 animate-spin" />
      ) : (
        <GitPullRequest size={13} className="shrink-0" />
      )}
      <span className="min-w-0 flex-1 truncate">{pr.title}</span>
      {pr.approved && <ApprovedMark />}
      <PrNumberLink pr={pr} />
    </div>
  );
}

function ApprovedMark() {
  return (
    <Hint title="Approved" className="text-added">
      <CircleCheck size={12} />
    </Hint>
  );
}

function PrNumberLink({ pr }: { pr: OpenPullRequest }) {
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        openUrl(pr.url).catch((err) => toast(`Couldn't open ${pr.url}: ${errorMessage(err)}`));
      }}
      title={`Open #${pr.number} on GitHub`}
      className="shrink-0 rounded px-0.5 fs-sidebar-xs text-faint hover:text-accent hover:underline"
    >
      #{pr.number}
    </button>
  );
}

function WorktreeRow({ project, worktree, openPr }: { project: Project; worktree: Worktree; openPr?: OpenPullRequest }) {
  const selected = useUi((s) => s.selectedWorktreePath === worktree.path);
  const select = useUi((s) => s.select);
  const openDialog = useUi((s) => s.openDialog);
  const qc = useQueryClient();
  const { data: status } = useWorktreeStatus(worktree.path, 30_000);
  const { data: sessions = [] } = useSessions();
  const { data: pr } = usePullRequest(worktree.path, !worktree.isMain && !!worktree.branch);
  const comments = useWorktreeComments(worktree.path).length;
  const busy = useWorktreeActivity(worktree.path);
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);

  const dirty = status?.changedFiles.length ?? 0;
  const mine = sessions.filter((s) => s.worktreePath === worktree.path);
  const activity = summarize(mine);
  const waitingOn = mine.find((s) => s.state.kind === "running" && s.activity === "waiting");

  return (
    <div
      onClick={() => {
        select(project.id, worktree.path);
        qc.invalidateQueries({ queryKey: keys.status(worktree.path) });
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenuAt({ x: e.clientX, y: e.clientY });
      }}
      title="Right-click for worktree actions"
      className={clsx("flex h-7 items-center gap-2 rounded-md pr-1 pl-7", selected ? "bg-selected" : "hover:bg-hover")}
    >
      <WorktreeIcon worktree={worktree} pr={pr} />
      <span className={clsx("min-w-0 flex-1 truncate", worktree.prunable && "text-faint line-through")}>
        {openPr?.title ?? worktreeLabel(worktree)}
      </span>
      {dirty > 0 && (
        <Hint title={plural(dirty, "uncommitted file")} className="text-warn">
          <Pencil size={11} />
        </Hint>
      )}
      {pr && checksFailed(pr) && (
        <Hint title={`${plural(pr.checksFailed, "check")} failed on #${pr.number}`} className="text-removed">
          <CircleX size={12} />
        </Hint>
      )}
      {busy && (
        <Hint title={`${busy.title}: ${lastStep(busy) ?? "working…"}`} className="text-muted">
          <LoaderCircle size={12} className="animate-spin" />
        </Hint>
      )}
      {activity && activity !== "exited" && (
        <ActivityDot
          state={activity}
          title={waitingOn ? `${waitingOn.title} is waiting${waitingOn.attention ? `: ${waitingOn.attention}` : ""}` : undefined}
        />
      )}
      {comments > 0 && (
        <span
          className="flex items-center gap-0.5 fs-sidebar-xs text-accent"
          title={`${plural(comments, "review comment")} not sent yet`}
        >
          <MessageSquare size={11} />
          {comments}
        </span>
      )}
      {openPr?.approved && <ApprovedMark />}
      {openPr && <PrNumberLink pr={openPr} />}
      {menuAt && (
        <ContextMenu at={menuAt} items={worktreeActions(project, worktree, openDialog)} onClose={() => setMenuAt(null)} />
      )}
    </div>
  );
}

/** The main checkout's house, the pull request's state, or a plain branch. */
function WorktreeIcon({ worktree, pr }: { worktree: Worktree; pr: PullRequest | null | undefined }) {
  if (worktree.isMain) {
    return (
      <Hint title="Main checkout: the repository folder itself" className="text-faint">
        <House size={13} />
      </Hint>
    );
  }
  if (!pr) {
    return (
      <Hint title={`Worktree on branch ${worktree.branch ?? "(detached)"}, no pull request yet`} className="text-faint">
        <GitBranch size={13} />
      </Hint>
    );
  }
  return (
    <Hint title={`Pull request #${pr.number} ${prStateLabel(pr)}: ${pr.title}`}>
      <PrStateIcon pr={pr} size={13} />
    </Hint>
  );
}

/** Agents waiting for an answer, across all projects, so none of them sits unnoticed. */
function NeedsYou({ projects, shown }: { projects: Project[]; shown: Project[] }) {
  const { data: sessions = [] } = useSessions();
  const select = useUi((s) => s.select);
  const setActive = useUi((s) => s.setActiveSession);

  const waiting = sessions
    .filter((s) => s.state.kind === "running" && s.activity === "waiting")
    .map((s) => ({ s, project: projectOfWorktree(projects, s.worktreePath) }))
    .filter(({ project }) => !project || shown.includes(project));
  if (waiting.length === 0) return null;
  return (
    <div className="mb-2 rounded-lg bg-warn/8 p-1">
      <div
        className="flex items-center gap-1.5 px-2 pt-1 pb-1 fs-sidebar-xs font-semibold tracking-wide text-warn uppercase"
        title="Agents waiting for an answer, across all projects"
      >
        <BellRing size={11} />
        Needs you
      </div>
      {waiting.map(({ s, project }) => (
        <div key={s.id} className="group/need flex items-start rounded-md hover:bg-hover">
          <button
            onClick={() => {
              if (project) select(project.id, s.worktreePath);
              setActive(s.worktreePath, s.id);
            }}
            title={s.attention ?? `${s.title} is waiting for you`}
            className="flex min-w-0 flex-1 flex-col px-2 py-1 text-left"
          >
            <span className="flex min-w-0 items-center gap-1.5 fs-sidebar">
              <span className="truncate">
                {s.title} <span className="text-faint">· {s.worktreePath.split("/").pop()}</span>
              </span>
            </span>
            {s.attention && <span className="truncate fs-sidebar-xs text-muted">{s.attention}</span>}
          </button>
          <IconButton
            label="Skip for now: hides this without answering"
            onClick={() => api.dismissAttention(s.id).catch(() => {})}
            className="mt-1 mr-1 size-5 opacity-0 group-hover/need:opacity-100"
          >
            <X size={12} />
          </IconButton>
        </div>
      ))}
    </div>
  );
}

const checksFailed = (pr: PullRequest) => pr.state === "OPEN" && pr.checksFailed > 0;

function prStateLabel(pr: PullRequest) {
  if (pr.state === "MERGED") return "merged";
  if (pr.state === "CLOSED") return "closed";
  return pr.isDraft ? "draft" : "open";
}

/** Offers the newer release once one was found, and installs it on click. */
function UpdateButton() {
  const { available, status, progress, error } = useUpdates();
  if (!available) return null;
  const installing = status === "installing";
  return (
    <button
      onClick={installUpdate}
      disabled={installing}
      title={
        error
          ? `The update failed: ${error}`
          : `worki ${available.version} is available. Installs it and restarts worki; agents in sandboxes and containers keep running.`
      }
      className="ml-auto flex h-7 items-center gap-1.5 rounded-md bg-accent px-2 text-[12px] font-medium text-accent-fg hover:brightness-110 disabled:opacity-80"
    >
      {installing ? <LoaderCircle size={12} className="animate-spin" /> : <ArrowDownToLine size={12} />}
      {installing
        ? progress !== null
          ? `Updating ${Math.round(progress * 100)}%`
          : "Updating…"
        : error
          ? "Retry update"
          : `Update to ${available.version}`}
    </button>
  );
}

function KeepAwakeButton() {
  const [on, setOn] = useKeepAwake();
  return (
    <button
      onClick={() => setOn(!on)}
      aria-pressed={on}
      title={on ? "Keeping the Mac awake while agents run. Click to allow sleep." : "Keep the Mac awake while agents run"}
      className={clsx(
        "flex size-7 items-center justify-center rounded-md hover:bg-hover",
        on ? "text-accent" : "text-faint hover:text-fg",
      )}
    >
      <Coffee size={14} />
    </button>
  );
}
