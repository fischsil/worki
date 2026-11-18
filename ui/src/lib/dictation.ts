import { useEffect, useRef, useState } from "react";

import { api, errorMessage } from "./api";

/** Dictation into a text box: what's said goes after the text that was there when it started. */
export function useDictation(text: string, setText: (text: string) => void, onError: (error: string | null) => void) {
  const [listening, setListening] = useState(false);
  // Events of a dictation that was cancelled or replaced still arrive; they're dropped.
  const run = useRef(0);
  useEffect(
    () => () => {
      run.current++;
      api.cancelDictation();
    },
    [],
  );

  const cancel = () => {
    if (!listening) return;
    run.current++;
    setListening(false);
    api.cancelDictation();
  };

  async function toggle() {
    if (listening) {
      api.stopDictation();
      return;
    }
    onError(null);
    setListening(true);
    const current = ++run.current;
    const before = text.trimEnd();
    try {
      if (!(await api.dictationAccess()))
        throw new Error("Allow worki to use speech recognition in System Settings › Privacy & Security › Speech Recognition.");
      await api.startDictation((event) => {
        if (current !== run.current) return;
        if (event.type === "failed") {
          setListening(false);
          onError(event.message);
          return;
        }
        setText(before && event.text ? `${before} ${event.text}` : before || event.text);
        if (event.done) setListening(false);
      });
    } catch (e) {
      if (current !== run.current) return;
      setListening(false);
      onError(errorMessage(e));
    }
  }

  return { listening, toggle, cancel };
}
