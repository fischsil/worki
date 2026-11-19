import { useEffect, useState } from "react";

import type { Resources } from "../lib/api";
import { Field, Input } from "./ui";

const HINT =
  "Empty uses the engine's default: 4 CPUs and 4 GB for Apple containers, all CPUs and half the memory for Docker (sbx), no limit for Docker and Podman. A worktree's container is recreated with new values at its next session start while no session runs in it; anything installed inside it is lost.";

/** CPUs and memory for containers and sandboxes; an empty field falls back to `fallback`. */
export function ResourceFields({
  value,
  fallback,
  onChange,
}: {
  value: Resources;
  fallback?: Resources;
  onChange: (next: Resources) => void;
}) {
  return (
    <div className="grid grid-cols-2 gap-3">
      <NumberField
        label="CPUs"
        hint={HINT}
        value={value.cpus}
        placeholder={placeholder(fallback?.cpus)}
        onCommit={(cpus) => onChange({ ...value, cpus })}
      />
      <NumberField
        label="Memory (GB)"
        hint={HINT}
        value={value.memoryGb}
        placeholder={placeholder(fallback?.memoryGb)}
        onCommit={(memoryGb) => onChange({ ...value, memoryGb })}
      />
    </div>
  );
}

function placeholder(inherited: number | null | undefined) {
  return inherited ? `Global: ${inherited}` : "Engine default";
}

function NumberField({
  label,
  hint,
  value,
  placeholder,
  onCommit,
}: {
  label: string;
  hint: string;
  value: number | null | undefined;
  placeholder: string;
  onCommit: (next: number | null) => void;
}) {
  const shown = value ? String(value) : "";
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);

  function commit() {
    const trimmed = text.trim();
    const next = trimmed === "" ? null : Number.parseInt(trimmed, 10);
    if (next !== null && (!Number.isFinite(next) || next < 1)) {
      setText(shown);
      return;
    }
    if (next !== (value ?? null)) onCommit(next);
  }

  return (
    <Field label={label} hint={hint}>
      <Input
        className="h-8 font-mono text-[13px]"
        inputMode="numeric"
        value={text}
        placeholder={placeholder}
        onChange={(e) => setText(e.target.value.replace(/[^0-9]/g, ""))}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && commit()}
      />
    </Field>
  );
}
