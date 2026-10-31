import { useEffect, useRef, type ReactNode, type RefObject } from "react";

export function ConfirmDialog({ labelledBy, busy, onCancel, children, returnFocus }: { labelledBy: string; busy: boolean; onCancel: () => void; children: ReactNode; returnFocus?: RefObject<HTMLElement | null> }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => {
      element?.close();
      queueMicrotask(() => returnFocus?.current?.focus());
    };
  }, [returnFocus]);
  return <dialog ref={dialog} className="confirm-dialog ws-modal" aria-labelledby={labelledBy} onCancel={(event) => {
    event.preventDefault();
    if (!busy) onCancel();
  }}>{children}</dialog>;
}
