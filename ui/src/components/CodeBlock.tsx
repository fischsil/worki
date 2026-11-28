import { useQueryClient } from "@tanstack/react-query";
import clsx from "clsx";
import { Check, ChevronDown, Copy, LoaderCircle, Play, Square, SquareTerminal, X } from "lucide-react";
import { createContext, type ReactNode, useContext } from "react";

import { errorMessage } from "../lib/api";
import { isShellLanguage, runOnHost, type RunTarget, snippetCommand } from "../lib/runSnippet";
import { type RunStatus, type SnippetRun, useSnippetRuns } from "../lib/snippetRuns";
import { toast } from "../lib/toast";
import { useCopy } from "../lib/useCopy";
import { IconButton, LogView, Menu } from "./ui";

export const RunTargetContext = createContext<RunTarget | null>(null);
export const SnippetScope = createContext("");

export function CodeBlock({
  language,
  source,
  offset,
  children,
}: {
  language?: string;
  source: string;
  offset: number;
  children: ReactNode;
}) {
  const target = useContext(RunTargetContext);
  const scope = useContext(SnippetScope);
  const runnable = !!target && isShellLanguage(language) && source.trim() !== "";
  const key = target ? `${target.sessionId}:${scope}:${offset}` : "";
  const run = useSnippetRuns((s) => (runnable ? s.runs[key] : undefined));
  const running = run?.status.kind === "running";

  return (
    <div className="my-2">
      <div className="group/code relative">
        <pre className="overflow-x-auto rounded-md bg-hover px-3 py-2 font-mono fs-chat-xs [&_code]:bg-transparent [&_code]:p-0">
          {children}
        </pre>
        <div
          className={clsx(
            "absolute top-1 right-1 flex items-center gap-0.5 rounded-md bg-hover transition-opacity group-hover/code:opacity-100 has-[[data-overlay]]:opacity-100",
            running ? "opacity-100" : "opacity-0",
          )}
        >
          {runnable && (
            <RunButtons target={target} runKey={key} running={running} command={snippetCommand(language, source)} />
          )}
          <CopyButton text={source} />
        </div>
      </div>
      {run && <RunOutput run={run} runKey={key} />}
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
  const { copied, copy } = useCopy();
  return (
    <IconButton label={copied ? "Copied" : "Copy"} onClick={() => copy(text)}>
      {copied ? <Check size={13} /> : <Copy size={13} />}
    </IconButton>
  );
}

function RunButtons({
  target,
  runKey,
  running,
  command,
}: {
  target: RunTarget;
  runKey: string;
  running: boolean;
  command: string;
}) {
  const qc = useQueryClient();
  const { start, stop } = useSnippetRuns.getState();
  const inIsolation = { label: "Run in isolation", icon: <Play size={13} />, onSelect: () => start(runKey, target, command) };
  const onHost = () => runOnHost(qc, target, command).catch((e) => toast(errorMessage(e)));

  return (
    <>
      {running ? (
        <IconButton label="Stop" onClick={() => stop(runKey)}>
          <Square size={12} />
        </IconButton>
      ) : (
        <IconButton label={inIsolation.label} onClick={inIsolation.onSelect}>
          {inIsolation.icon}
        </IconButton>
      )}
      <Menu
        trigger={(open) => (
          <IconButton label="Choose where to run" onClick={open} className="w-4">
            <ChevronDown size={11} />
          </IconButton>
        )}
        items={[inIsolation, { label: "Run on host", icon: <SquareTerminal size={13} />, onSelect: onHost }]}
      />
    </>
  );
}

function RunOutput({ run, runKey }: { run: SnippetRun; runKey: string }) {
  return (
    <div className="mt-1 flex flex-col gap-1">
      <div className="flex items-center gap-1.5 px-1 fs-chat-xs text-faint">
        {run.status.kind === "running" && <LoaderCircle size={11} className="animate-spin" />}
        <span className={clsx("flex-1", isFailure(run.status) && "text-removed")}>{statusLabel(run.status)}</span>
        {run.status.kind !== "running" && (
          <IconButton label="Clear output" onClick={() => useSnippetRuns.getState().clear(runKey)} className="size-5">
            <X size={11} />
          </IconButton>
        )}
      </div>
      {run.lines.length > 0 && <LogView lines={run.lines} className="max-h-72" />}
      {run.status.kind === "failed" && (
        <div className="px-1 fs-chat-xs whitespace-pre-wrap text-removed">{run.status.error}</div>
      )}
    </div>
  );
}

function isFailure(status: RunStatus) {
  return status.kind === "failed" || (status.kind === "exited" && status.code !== 0);
}

function statusLabel(status: RunStatus) {
  switch (status.kind) {
    case "running":
      return "Running…";
    case "exited":
      return status.code === 0 ? "Done" : `Exited with code ${status.code}`;
    case "stopped":
      return "Stopped";
    case "failed":
      return "Couldn't run";
  }
}
