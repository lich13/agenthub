import { useCallback, useEffect, useRef, useState } from "react";

export interface CopyFeedbackController {
  state: "idle" | "copied" | "failed";
  feedback: string;
  setFeedback: (message: string) => void;
  copy: (
    value: string | null | undefined,
    successMessage?: string,
    emptyMessage?: string,
  ) => Promise<boolean>;
  clear: () => void;
}

export function useCopyFeedback(resetMs = 2000): CopyFeedbackController {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const [feedback, setFeedbackState] = useState("");
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const attemptRef = useRef(0);

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  useEffect(
    () => () => {
      clearTimer();
      attemptRef.current += 1;
    },
    [clearTimer],
  );

  const scheduleReset = useCallback(() => {
    clearTimer();
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      setState("idle");
      setFeedbackState("");
    }, resetMs);
  }, [clearTimer, resetMs]);

  const setFeedback = useCallback(
    (message: string) => {
      clearTimer();
      setFeedbackState(message);
      if (message) {
        scheduleReset();
      } else {
        setState("idle");
      }
    },
    [clearTimer, scheduleReset],
  );

  const copy = useCallback(
    async (
      value: string | null | undefined,
      successMessage = "已复制",
      emptyMessage = "",
    ) => {
      if (!value) {
        if (emptyMessage) setFeedback(emptyMessage);
        return false;
      }
      const current = ++attemptRef.current;
      clearTimer();
      try {
        await navigator.clipboard.writeText(value);
        if (current !== attemptRef.current) return false;
        setState("copied");
        setFeedbackState(successMessage);
        scheduleReset();
        return true;
      } catch (err) {
        if (current !== attemptRef.current) return false;
        setState("failed");
        const detail =
          err instanceof Error && err.message ? err.message : "剪贴板不可用";
        setFeedbackState(`复制失败：${detail}`);
        return false;
      }
    },
    [clearTimer, scheduleReset, setFeedback],
  );

  const clear = useCallback(() => {
    clearTimer();
    attemptRef.current += 1;
    setState("idle");
    setFeedbackState("");
  }, [clearTimer]);

  return {
    state,
    feedback,
    setFeedback,
    copy,
    clear,
  };
}
