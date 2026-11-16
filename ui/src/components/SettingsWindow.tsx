import clsx from "clsx";
import { X } from "lucide-react";
import { type ReactNode, useEffect, useRef } from "react";

import { IconButton } from "./ui";

export interface SettingsPage<T extends string> {
  id: T;
  label: string;
  icon: ReactNode;
}

/** A window of settings pages: the pages listed on the left, the current one on the right. Escape,
 * a click outside and the close button call `onClose`. */
export function SettingsWindow<T extends string>({
  label,
  heading,
  pages,
  current,
  onSelect,
  onClose,
  navFooter,
  children,
}: {
  label: string;
  heading: string;
  pages: SettingsPage<T>[];
  current: T;
  onSelect: (page: T) => void;
  onClose: () => void;
  navFooter?: ReactNode;
  children: ReactNode;
}) {
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close.current();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div
      data-overlay
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/25 p-8 backdrop-blur-[1px]"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-label={label}
        className="flex h-[min(760px,88vh)] w-[min(980px,94vw)] overflow-hidden rounded-xl border border-line bg-elevated shadow-2xl"
      >
        <nav className="flex w-52 shrink-0 flex-col gap-0.5 border-r border-line bg-panel/60 p-2">
          <div className="truncate px-2 pt-2 pb-3 text-[11px] font-semibold tracking-wide text-faint uppercase">
            {heading}
          </div>
          {pages.map((p) => (
            <button
              key={p.id}
              onClick={() => onSelect(p.id)}
              className={clsx(
                "flex h-8 items-center gap-2 rounded-md px-2 text-left text-[13px]",
                p.id === current ? "bg-selected font-medium text-fg" : "text-muted hover:bg-hover hover:text-fg",
              )}
            >
              {p.icon}
              {p.label}
            </button>
          ))}
          {navFooter && (
            <>
              <div className="flex-1" />
              {navFooter}
            </>
          )}
        </nav>

        <div className="relative flex min-w-0 flex-1 flex-col">
          <IconButton label={`Close ${label.toLowerCase()}`} onClick={onClose} className="absolute top-4 right-4 z-10">
            <X size={15} />
          </IconButton>
          {children}
        </div>
      </div>
    </div>
  );
}
