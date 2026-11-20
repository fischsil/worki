import { Fragment, useState } from "react";

import {
  DEFAULT_AGENT_USER,
  DEFAULT_CONTAINER_IMAGE,
  hasImage,
  type Isolation,
  type ProjectIsolation,
} from "../lib/api";
import { useEnvironmentTemplates } from "../lib/queries";
import { Field, Input, Select } from "./ui";

type Kind = ProjectIsolation["kind"];

const options: { kind: Kind; label: string; hint: string }[] = [
  { kind: "inherit", label: "Global default", hint: "" },
  { kind: "none", label: "None", hint: "Agents run as you, with full access to your account." },
  {
    kind: "macUser",
    label: "User Isolation",
    hint: "Agents run as a separate hidden macOS user that can only reach the worktrees assigned to it.",
  },
  {
    kind: "dockerSandbox",
    label: "Docker (sbx)",
    hint: "Each worktree gets its own sandbox, a separate Linux microVM. Needs Docker's sbx.",
  },
  {
    kind: "docker",
    label: "Docker",
    hint: "Each worktree gets its own Docker container. Needs Docker Desktop.",
  },
  {
    kind: "podman",
    label: "Podman",
    hint: "Each worktree gets its own container in the Podman machine VM. Needs Podman.",
  },
  {
    kind: "appleContainer",
    label: "Apple containers",
    hint: "Each worktree gets its own container in a lightweight VM of its own. Needs Apple's container tool, a Mac with Apple silicon and macOS 26.",
  },
];

export function IsolationPicker<T extends ProjectIsolation>({
  label,
  value,
  onChange,
  allowInherit,
  inheritLabel,
  inheritName = "Global default",
}: {
  label: string;
  value: T;
  onChange: (v: T) => void;
  allowInherit?: boolean;
  inheritLabel?: string;
  /** What "inherit" means here: the global default (projects) or the project default (worktrees). */
  inheritName?: string;
}) {
  const visible = options.filter((o) => allowInherit || o.kind !== "inherit");
  const current = visible.find((o) => o.kind === value.kind);

  function pick(kind: Kind) {
    if (kind === value.kind) return;
    const next: ProjectIsolation =
      kind === "macUser"
        ? { kind, user: DEFAULT_AGENT_USER }
        : kind === "dockerSandbox"
          ? { kind }
          : hasImage({ kind } as ProjectIsolation)
            ? ({ kind, image: DEFAULT_CONTAINER_IMAGE } as ProjectIsolation)
            : ({ kind } as ProjectIsolation);
    onChange(next as T);
  }

  return (
    <div className="flex flex-col gap-4">
      <Field label={label} hint={current?.hint}>
        <Select
          value={value.kind}
          onChange={(e) => pick(e.target.value as Kind)}
          className="h-8 w-full text-[13px]"
        >
          {visible.map((o) => (
            <Fragment key={o.kind}>
              <option value={o.kind}>
                {o.kind === "inherit" ? (inheritLabel ? `${inheritName} (${inheritLabel})` : inheritName) : o.label}
              </option>
              {o.kind === "inherit" && <hr />}
            </Fragment>
          ))}
        </Select>
      </Field>
      {value.kind === "macUser" && (
        <Field label="User name">
          <Input
            className="h-8 font-mono text-[13px]"
            value={value.user}
            onChange={(e) => onChange({ kind: "macUser", user: e.target.value.trim() } as T)}
          />
        </Field>
      )}
      {hasImage(value) && (
        <Field
          label="Image"
          hint={
            value.image === DEFAULT_CONTAINER_IMAGE
              ? "worki's agent image (Alpine, git, gh, ripgrep, Claude Code), built by Set up below."
              : "Any image with bash and git."
          }
        >
          <TemplateSelect
            isolation={value}
            value={value.image}
            fallback={DEFAULT_CONTAINER_IMAGE}
            fallbackLabel={DEFAULT_CONTAINER_IMAGE}
            onChange={(image) => onChange({ kind: value.kind, image: image ?? DEFAULT_CONTAINER_IMAGE } as T)}
          />
        </Field>
      )}
      {value.kind === "dockerSandbox" && (
        <SandboxTemplate
          template={value.template ?? null}
          onChange={(template) => onChange({ kind: "dockerSandbox", template } as T)}
        />
      )}
    </div>
  );
}

function SandboxTemplate({
  template,
  onChange,
}: {
  template: string | null;
  onChange: (template: string | null) => void;
}) {
  return (
    <Field
      label="Start from"
      hint="Agents are installed in a worktree's sandbox the first time they start there. Save a snapshot of one to give new worktrees everything installed in it."
    >
      <TemplateSelect
        isolation={{ kind: "dockerSandbox" }}
        value={template}
        fallback={null}
        fallbackLabel="Fresh sandbox"
        onChange={onChange}
      />
    </Field>
  );
}

const OTHER = "\u0000other";

/** Snapshots (images or sbx templates) on this machine, the default, and "Other…" to type a name. */
function TemplateSelect({
  isolation,
  value,
  fallback,
  fallbackLabel,
  onChange,
}: {
  isolation: Isolation;
  value: string | null;
  fallback: string | null;
  fallbackLabel: string;
  onChange: (value: string | null) => void;
}) {
  const { data: found = [], isFetching } = useEnvironmentTemplates(isolation);
  const listed = [...new Set([...found, ...(value && value !== fallback ? [value] : [])])].filter(
    (t) => t !== fallback,
  );
  const [typing, setTyping] = useState(false);
  const key = value ?? "";
  return (
    <span className="flex min-w-0 flex-col gap-2">
      <Select
        value={typing ? OTHER : key}
        onChange={(e) => {
          const picked = e.target.value;
          setTyping(picked === OTHER);
          if (picked !== OTHER) onChange(picked === "" ? fallback : picked);
        }}
        className="h-8 w-full font-mono text-[13px]"
      >
        <option value={fallback ?? ""}>{fallbackLabel}</option>
        {listed.map((t) => (
          <option key={t} value={t}>
            {t}
          </option>
        ))}
        <option value={OTHER}>{isFetching && found.length === 0 ? "Loading…" : "Other…"}</option>
      </Select>
      {typing && (
        <Input
          autoFocus
          className="h-8 font-mono text-[13px]"
          placeholder="name:tag"
          onBlur={(e) => {
            const v = e.target.value.trim();
            if (v) onChange(v);
            setTyping(false);
          }}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      )}
    </span>
  );
}
