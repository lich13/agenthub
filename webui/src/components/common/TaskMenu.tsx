import { MoreHorizontal } from "lucide-react";
import { createPortal } from "react-dom";
import { useLayoutEffect, useEffect, useImperativeHandle, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";

export function TaskMenu({ label = "任务操作", children, triggerRef }: { label?: string; children: ReactNode; triggerRef?: Ref<HTMLElement> }) {
  const menu = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  useImperativeHandle(triggerRef, () => trigger.current as HTMLButtonElement, []);
  const close = (restoreFocus: boolean) => {
    if (!open) return;
    setOpen(false);
    if (restoreFocus) requestAnimationFrame(() => trigger.current?.focus());
  };
  useEffect(() => {
    const outside = (event: Event) => {
      if (!menu.current?.contains(event.target as Node)) close(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  const focusMenuItem = (direction: "next" | "previous") => {
    const buttons = Array.from(menu.current?.querySelectorAll<HTMLButtonElement>(".task-menu-items button:not(:disabled)") ?? []);
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = index < 0
      ? (direction === "next" ? 0 : buttons.length - 1)
      : (index + (direction === "next" ? 1 : buttons.length - 1)) % buttons.length;
    buttons[next]?.focus();
  };
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") { event.preventDefault(); close(true); return; }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) setOpen(true);
      requestAnimationFrame(() => focusMenuItem(event.key === "ArrowDown" ? "next" : "previous"));
    }
  };
  return <div ref={menu} className="task-menu" onKeyDown={handleKeyDown} onBlur={(event) => {
    if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) close(false);
  }}>
    <button ref={trigger} type="button" className="icon-button" title={label} aria-label={label} aria-expanded={open} onClick={() => setOpen(value => !value)}><MoreHorizontal size={18} /></button>
    {open && <div className="task-menu-items" onClick={(event) => {
      if ((event.target as Element).closest("button")) close(true);
    }}>{children}</div>}
  </div>;
}

export type ThreadMenuAction = {
  id: string;
  label: string;
  icon: ReactNode;
  disabled?: boolean;
  reason?: string | null;
  danger?: boolean;
  run: () => void;
};

export function ThreadMenuItems({ actions, itemRole = "menuitem" }: { actions: ThreadMenuAction[]; itemRole?: "menuitem" | "button" }) {
  return <>{actions.map(action => <button key={action.id} type="button" role={itemRole === "menuitem" ? "menuitem" : undefined}
    disabled={action.disabled} title={action.reason ?? undefined}
    className={action.danger ? "danger-menu-item" : undefined} onClick={action.run}>
    {action.icon}{action.label}
  </button>)}</>;
}

export function ContextTaskMenu({ x, y, onClose, returnFocus, children }: {
  x: number; y: number;
  onClose: () => void;
  returnFocus: HTMLElement | null;
  children: ReactNode;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const [position, setPosition] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const rect = menu.current?.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(x, window.innerWidth - (rect?.width ?? 240) - 8)),
      top: Math.max(8, Math.min(y, window.innerHeight - (rect?.height ?? 260) - 8)),
    });
    menu.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true });
  }, [x, y]);
  useEffect(() => {
    const close = () => { closeRef.current(); returnFocus?.focus({ preventScroll: true }); };
    const outside = (event: Event) => {
      if (!menu.current?.contains(event.target as Node)) {
        closeRef.current();
        if (!(event.target instanceof Element) || !event.target.closest("button,input,select,textarea,a,[tabindex]")) returnFocus?.focus({ preventScroll: true });
      }
    };
    const scrolled = (event: Event) => {
      if (!menu.current?.contains(event.target as Node)) close();
    };
    const key = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" || event.key === "Tab") {
        if (event.key === "Escape") event.preventDefault();
        close();
      }
    };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("scroll", scrolled, true);
    document.addEventListener("keydown", key, true);
    window.addEventListener("resize", close);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("scroll", scrolled, true);
      document.removeEventListener("keydown", key, true);
      window.removeEventListener("resize", close);
    };
  }, [returnFocus]);
  return createPortal(<div ref={menu} className="task-menu-items context-task-menu" role="menu"
    aria-label="线程操作" style={{ position: "fixed", ...position, zIndex: 100 }}
    onContextMenu={event => event.preventDefault()}
    onKeyDown={event => {
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      const buttons = Array.from(menu.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
        : index < 0 ? 0 : (index + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length;
      buttons[next]?.focus();
    }}
    onClick={event => {
      if ((event.target as Element).closest("button:not(:disabled)")) closeRef.current();
    }}>{children}</div>, document.body);
}
