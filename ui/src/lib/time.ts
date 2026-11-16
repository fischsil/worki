import { useEffect, useState } from "react";

/** The current time, ticking each second while `active`. */
export function useNow(active: boolean) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}

/** "42s", "3m 05s". */
export function elapsed(since: number, now: number): string {
  const s = Math.max(0, Math.floor((now - since) / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}

const timeOfDay = new Intl.DateTimeFormat(undefined, { timeStyle: "short" });
const dayAndTime = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
const full = new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "medium" });

/** "14:32" today, "Nov 14, 14:32" on another day. */
export function clockTime(ms: number): string {
  const date = new Date(ms);
  return date.toDateString() === new Date().toDateString() ? timeOfDay.format(date) : dayAndTime.format(date);
}

export const fullTime = (ms: number) => full.format(new Date(ms));
