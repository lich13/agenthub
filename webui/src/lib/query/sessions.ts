import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  executeSessionBatch,
  previewSessionBatch,
  type SessionBatchPreview,
  type SessionBatchResult,
  type SessionOperation,
  type SessionProvider,
} from "../api/sessions";
import { useConnection } from "./connection";
import {
  addSessionSelection,
  clickSessionSelection,
  emptySessionSelection,
  isMacLikePlatform,
  isToggleSelectionClick,
  syncSessionSelection,
  type SessionClick,
} from "../domain/sessionSelection";

function hasSubagentTree(provider: SessionProvider, operation: SessionOperation): boolean {
  return (
    (provider === "codex" || provider === "antigravity" || provider === "dsh") &&
    operation === "delete"
  );
}

export function useSessionSelection(
  provider: SessionProvider,
  filterKey: string,
  keys: string[],
  onSucceeded?: (keys: string[], operation: SessionOperation) => void,
  activeKey?: string | null,
  onSelectionStart?: () => void,
) {
  const client = useQueryClient();
  const connection = useConnection();
  const scope = `${connection.revision}\0${connection.target}\0${provider}\0${filterKey}`;
  const returnFocus = useRef<HTMLElement | null>(null);
  const [state, setState] = useState(() => emptySessionSelection(scope));
  const current = syncSessionSelection(state, scope, keys);
  const { selecting, keys: selection, notice } = current;
  const [preview, setPreview] = useState<SessionBatchPreview | null>(null);
  const [result, setResult] = useState<SessionBatchResult | null>(null);
  const epoch = useRef({ scope, value: 0 });
  if (epoch.current.scope !== scope) epoch.current = { scope, value: epoch.current.value + 1 };

  const prepare = useMutation({
    mutationFn: async (
      input: SessionOperation | { operation: SessionOperation; keys: string[] },
    ) => {
      const operation = typeof input === "string" ? input : input.operation;
      const sessionKeys = typeof input === "string" ? selection : input.keys;
      const generation = epoch.current.value;
      const includeDescendants = hasSubagentTree(provider, operation);
      if (generation !== epoch.current.value) throw new Error("选择范围已变化，请重新预览");
      const response: SessionBatchPreview = { provider, operation, items: [] };
      const size = operation === "delete" ? Math.max(1, sessionKeys.length) : 100;
      for (let start = 0; start < sessionKeys.length; start += size) {
        if (generation !== epoch.current.value) throw new Error("选择范围已变化");
        const page = await previewSessionBatch({
          provider,
          operation,
          sessionKeys: sessionKeys.slice(start, start + size),
          ...(includeDescendants ? { includeDescendants: true } : {}),
        });
        response.items.push(...page.items);
      }
      return { generation, response };
    },
    onSuccess: ({ generation, response }) => {
      if (generation === epoch.current.value) setPreview(response);
    },
  });

  const execute = useMutation({
    mutationFn: async () => {
      if (!preview) throw new Error("请先重新预览");
      const generation = epoch.current.value;
      const request = preview;
      // Every attempt consumes its preview, including an uncertain transport failure.
      setPreview(null);
      const items = request.items
        .filter(i => i.allowed)
        .map(i => ({
          sessionKey: i.sessionKey,
          ...(request.operation === "delete"
            ? { descendants: i.descendants }
            : { fingerprint: i.fingerprint! }),
        }));
      const response: SessionBatchResult = { items: [] };
      const size = request.operation === "delete" ? Math.max(1, items.length) : 100;
      const includeDescendants = hasSubagentTree(request.provider, request.operation);
      for (let start = 0; start < items.length; start += size) {
        if (generation !== epoch.current.value) throw new Error("选择范围已变化");
        const batchResult = await executeSessionBatch({
          provider: request.provider,
          operation: request.operation,
          items: items.slice(start, start + size),
          confirmed: true,
          ...(includeDescendants ? { includeDescendants: true } : {}),
        });
        response.items.push(...batchResult.items);
      }
      return { response, generation, operation: request.operation };
    },
    onSuccess: ({ response, generation, operation }) => {
      const succeeded = [
        ...new Set(
          response.items.flatMap(i => [
            ...(i.status === "succeeded" ? [i.sessionKey] : []),
            ...(i.deletedIds ?? []),
          ]),
        ),
      ];
      if (generation === epoch.current.value) {
        onSucceeded?.(succeeded, operation);
        setResult(response);
        setState(prev => {
          const remaining = prev.keys.filter(key => !succeeded.includes(key));
          return {
            ...prev,
            selecting: remaining.length > 0 && prev.selecting,
            keys: remaining,
            anchor: prev.anchor && succeeded.includes(prev.anchor) ? null : prev.anchor,
          };
        });
      }
      void client.invalidateQueries({
        predicate: query =>
          provider === "codex"
            ? /thread|system-capabilities|probe-status/.test(String(query.queryKey[0]))
            : query.queryKey[0] === provider,
      });
    },
  });

  useEffect(() => {
    setState(emptySessionSelection(scope));
    setPreview(null);
    setResult(null);
    prepare.reset();
    execute.reset();
  }, [scope]);

  const visibleKeys = keys.join("\0");
  useEffect(() => {
    setState(prev => syncSessionSelection(prev, scope, keys));
  }, [scope, visibleKeys]);

  const busy = prepare.isPending || execute.isPending;

  const select = (key: string, event: SessionClick, open: () => void) => {
    if (busy || (isMacLikePlatform() && event.ctrlKey && event.detail !== 0)) return;
    onSelectionStart?.();
    const modified = event.detail !== 0 && (event.shiftKey || isToggleSelectionClick(event));
    const next = clickSessionSelection(current, keys, key, event, activeKey);
    setState(next);
    if (modified) {
      epoch.current.value += 1;
      setPreview(null);
      setResult(null);
    } else {
      if (current.selecting || current.keys.length > 0) {
        epoch.current.value += 1;
        setPreview(null);
        setResult(null);
      }
      open();
    }
  };

  const prepareBatch = (operation: SessionOperation, origin?: HTMLElement | null) => {
    if (!selection.length || busy) return;
    returnFocus.current =
      origin ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    execute.reset();
    setResult(null);
    setPreview(null);
    prepare.mutate(operation);
  };

  const clear = () => {
    // Clearing a drawer must not interrupt a batch already being executed.
    if (!execute.isPending) epoch.current.value += 1;
    setPreview(null);
    setResult(null);
    if (!prepare.isPending) prepare.reset();
    if (!execute.isPending) execute.reset();
    setState({ ...current, selecting: false, keys: [], anchor: null, notice: null });
  };

  const selectAll = () => {
    if (!keys.length || busy) return;
    onSelectionStart?.();
    epoch.current.value += 1;
    setPreview(null);
    setResult(null);
    setState(addSessionSelection({ ...current, keys: [] }, keys));
  };

  const onListKeyDown = (event: ReactKeyboardEvent<HTMLElement>, allowSelection = true) => {
    const target = event.target as HTMLElement | null;
    const textInput = target?.tagName === "INPUT" &&
      !["checkbox", "radio", "button", "submit", "reset", "image", "file", "range", "color"].includes((target as HTMLInputElement).type);
    if (target && (textInput || target.tagName === "TEXTAREA" || target.isContentEditable || target.closest('dialog,[role="dialog"],[role="alertdialog"],[role="menu"]'))) {
      return;
    }
    if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "a") {
      event.preventDefault();
      event.stopPropagation();
      if (allowSelection) selectAll();
      return;
    }
    if (!allowSelection) return;
    if (event.key === "Escape" && (selecting || selection.length > 0)) {
      event.preventDefault();
      event.stopPropagation();
      clear();
      return;
    }
    if (
      selection.length > 0 &&
      !busy &&
      ((event.metaKey && event.key === "Backspace") || event.key === "Delete")
    ) {
      event.preventDefault();
      event.stopPropagation();
      prepareBatch("delete", target);
    }
  };

  return {
    selecting,
    selection,
    notice,
    preview,
    result,
    prepare,
    execute,
    returnFocus,
    select,
    busy,
    prepareBatch,
    onListKeyDown,
    toggle: (key: string) =>
      select(key, { detail: 1, metaKey: true, shiftKey: false, ctrlKey: false }, () => {}),
    prepareSingle: (key: string, operation: SessionOperation, origin?: HTMLElement | null) => {
      onSelectionStart?.();
      returnFocus.current =
        origin ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
      epoch.current.value += 1;
      setState({ ...current, selecting: false, keys: [key], anchor: key, notice: null });
      setResult(null);
      setPreview(null);
      prepare.mutate({ operation, keys: [key] });
    },
    cancel: clear,
    selectAll,
    clear,
    dismissPreview: () => {
      epoch.current.value += 1;
      setPreview(null);
    },
  };
}

export type SessionSelection = ReturnType<typeof useSessionSelection>;

export type { SessionOperation } from "../api/sessions";
