import { ChevronRight } from "lucide-react";

import { DEFAULT_WRITER_COMMAND } from "../lib/api";
import { ASK_PROMPTS, type AskPrompt } from "../lib/askPrompts";
import { useUi } from "../lib/store";
import { Field, Group, Input, Textarea } from "./ui";

export function GitSettings() {
  const writerCommand = useUi((s) => s.writerCommand);
  const setWriterCommand = useUi((s) => s.setWriterCommand);
  const customized = useUi((s) => Object.values(s.askPrompts).filter((p) => p != null).length);

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <h2 className="px-6 pt-5 pb-3 text-[15px] font-semibold">Git</h2>
      <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-6 pb-6">
        <Group title="Agent tasks">
          <Field
            label="Command for writing"
            hint="Writes commit messages and pull request descriptions in the background. For pull request descriptions, {fork} copies the conversation of the worktree's Claude agent when there is one; commit messages are written from the changes alone. {prompt} is the task. For Codex: codex exec {prompt}"
          >
            <Input
              value={writerCommand || DEFAULT_WRITER_COMMAND}
              onChange={(e) => setWriterCommand(e.target.value.trim() === DEFAULT_WRITER_COMMAND ? "" : e.target.value)}
              spellCheck={false}
              className="h-8 font-mono text-[12px]"
            />
            {writerCommand && (
              <button className="self-start text-[11px] text-muted hover:text-fg" onClick={() => setWriterCommand("")}>
                Reset to default
              </button>
            )}
          </Field>
          <details className="group">
            <summary className="flex cursor-default items-center gap-1 text-xs font-medium text-muted select-none hover:text-fg">
              <ChevronRight size={12} className="transition-transform group-open:rotate-90" />
              Prompts{customized > 0 && <span className="font-normal text-faint"> · {customized} changed</span>}
            </summary>
            <div className="mt-3 flex flex-col gap-4">
              {(Object.keys(ASK_PROMPTS) as AskPrompt[]).map((kind) => (
                <PromptField key={kind} kind={kind} />
              ))}
            </div>
          </details>
        </Group>
      </div>
    </section>
  );
}

function PromptField({ kind }: { kind: AskPrompt }) {
  const custom = useUi((s) => s.askPrompts[kind]);
  const setPrompt = useUi((s) => s.setAskPrompt);
  const { label, hint, text } = ASK_PROMPTS[kind];
  return (
    <Field label={`Prompt: ${label}`} hint={hint}>
      <Textarea
        rows={kind === "resolveConflicts" ? 3 : 7}
        value={custom ?? text}
        onChange={(e) => setPrompt(kind, e.target.value === text ? null : e.target.value)}
        className="text-[13px]"
      />
      {custom !== undefined && (
        <button className="self-start text-[11px] text-muted hover:text-fg" onClick={() => setPrompt(kind, null)}>
          Reset to default
        </button>
      )}
    </Field>
  );
}
