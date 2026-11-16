import clsx from "clsx";
import { Monitor, Moon, Sun } from "lucide-react";
import type { ReactNode } from "react";

import { FONT_SIZES, type Theme, useUi } from "../lib/store";
import { Group, Select } from "./ui";

const themes: Choice<Theme>[] = [
  { value: "system", label: "System", icon: <Monitor size={13} /> },
  { value: "light", label: "Light", icon: <Sun size={13} /> },
  { value: "dark", label: "Dark", icon: <Moon size={13} /> },
];

export function AppearanceSettings() {
  const ui = useUi();
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <h2 className="px-6 pt-5 pb-3 text-[15px] font-semibold">Appearance</h2>
      <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-6 pb-6">
        <Group title="Theme">
          <Segmented choices={themes} value={ui.theme} onChange={ui.setTheme} />
        </Group>

        <Group title="Font sizes">
          <div className="grid w-72 grid-cols-[1fr_auto] items-center gap-x-4 gap-y-2">
            <FontSizeField label="Sidebar" value={ui.sidebarFontSize} onChange={ui.setSidebarFontSize} />
            <FontSizeField label="Terminal" value={ui.terminalFontSize} onChange={ui.setTerminalFontSize} />
            <FontSizeField label="Chat" value={ui.chatFontSize} onChange={ui.setChatFontSize} />
            <FontSizeField label="Changes panel" value={ui.panelFontSize} onChange={ui.setPanelFontSize} />
            <FontSizeField label="Diff" value={ui.diffFontSize} onChange={ui.setDiffFontSize} />
          </div>
        </Group>
      </div>
    </section>
  );
}

function FontSizeField({ label, value, onChange }: { label: string; value: number; onChange: (size: number) => void }) {
  return (
    <label className="contents">
      <span className="text-[13px]">{label}</span>
      <Select value={value} onChange={(e) => onChange(Number(e.target.value))} className="h-7 w-24 text-[13px]">
        {FONT_SIZES.map((size) => (
          <option key={size} value={size}>
            {size} pt
          </option>
        ))}
      </Select>
    </label>
  );
}

interface Choice<T> {
  value: T;
  label: string;
  icon: ReactNode;
}

function Segmented<T>({ choices, value, onChange }: { choices: Choice<T>[]; value: T; onChange: (value: T) => void }) {
  return (
    <div className="flex w-fit rounded-md border border-line p-0.5">
      {choices.map((c) => (
        <button
          key={c.label}
          onClick={() => onChange(c.value)}
          className={clsx(
            "flex h-7 items-center gap-1.5 rounded px-3 text-[13px]",
            value === c.value ? "bg-selected font-medium text-fg" : "text-muted hover:text-fg",
          )}
        >
          {c.icon}
          {c.label}
        </button>
      ))}
    </div>
  );
}
