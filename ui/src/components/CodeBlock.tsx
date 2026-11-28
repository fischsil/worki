import { useQueryClient } from "@tanstack/react-query";
import { Check, ChevronDown, Copy, Play, SquareTerminal } from "lucide-react";
import { createContext, type ReactNode, useContext } from "react";

import { errorMessage } from "../lib/api";
import { isShellLanguage, runSnippet, type RunTarget, type RunWhere, snippetCommand } from "../lib/runSnippet";
import { toast } from "../lib/toast";
import { useCopy } from "../lib/useCopy";
import { IconButton, Menu } from "./ui";

export const RunTargetContext = createContext<RunTarget | null>(null);

export function CodeBlock({ language, source, children }: { language?: string; source: string; children: ReactNode }) {
  const target = useContext(RunTargetContext);
  const runnable = !!target && isShellLanguage(language) && source.trim() !== "";

  return (
    <div className="group/code relative my-2">
      <pre className="overflow-x-auto rounded-md bg-hover px-3 py-2 font-mono fs-chat-xs [&_code]:bg-transparent [&_code]:p-0">
        {children}
      </pre>
      <div className="absolute top-1 right-1 flex items-center gap-0.5 rounded-md bg-hover opacity-0 transition-opacity group-hover/code:opacity-100 has-[[data-overlay]]:opacity-100">
        {runnable && <RunButtons target={target} language={language} source={source} />}
        <CopyButton text={source} />
      </div>
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

function RunButtons({ target, language, source }: { target: RunTarget; language?: string; source: string }) {
  const qc = useQueryClient();
  const run = (where: RunWhere) =>
    runSnippet(qc, target, where, snippetCommand(language, source)).catch((e) => toast(errorMessage(e)));
  const inIsolation = { label: "Run in isolation", icon: <Play size={13} />, onSelect: () => run("isolation") };

  return (
    <>
      <IconButton label={inIsolation.label} onClick={inIsolation.onSelect}>
        {inIsolation.icon}
      </IconButton>
      <Menu
        trigger={(open) => (
          <IconButton label="Choose where to run" onClick={open} className="w-4">
            <ChevronDown size={11} />
          </IconButton>
        )}
        items={[
          inIsolation,
          { label: "Run on host", icon: <SquareTerminal size={13} />, onSelect: () => run("host") },
        ]}
      />
    </>
  );
}
