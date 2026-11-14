import { isValidElement, type ComponentProps, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { RenameableSession } from "./RenameableSession";
import { ContextTaskMenu, ThreadMenuItems } from "./TaskMenu";

// The project uses Vitest's Node environment. Exercise component event/state
// boundaries here; native event dispatch, portals and layout belong to browser QA.
const hooks = vi.hoisted(() => ({
  index: 0, dirty: false,
  cells: new Map<number, unknown>(),
  deps: new Map<number, readonly unknown[] | undefined>(),
  cleanup: new Map<number, (() => void) | void>(),
  effects: [] as (() => void)[]
}));

vi.mock("react", async original => ({
  ...await original<typeof import("react")>(),
  useState: (initial: unknown) => {
    const index = hooks.index++;
    if (!hooks.cells.has(index)) hooks.cells.set(index, typeof initial === "function" ? initial() : initial);
    return [hooks.cells.get(index), (value: unknown) => {
      const next = typeof value === "function" ? value(hooks.cells.get(index)) : value;
      if (!Object.is(next, hooks.cells.get(index))) { hooks.cells.set(index, next); hooks.dirty = true; }
    }];
  },
  useRef: (initial: unknown) => {
    const index = hooks.index++;
    if (!hooks.cells.has(index)) hooks.cells.set(index, { current: initial });
    return hooks.cells.get(index);
  },
  useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) => {
    const index = hooks.index++;
    const previous = hooks.deps.get(index);
    if (!deps || !previous || deps.some((value, position) => !Object.is(value, previous[position]))) {
      hooks.deps.set(index, deps);
      hooks.effects.push(() => { hooks.cleanup.get(index)?.(); hooks.cleanup.set(index, effect()); });
    }
  }
}));

type Props = ComponentProps<typeof RenameableSession>;
type Element = ReactElement<Record<string, any>>;
type Ref = { current: unknown };
const host = () => ({ focus: vi.fn(), select: vi.fn(), getBoundingClientRect: () => ({ left: 20, top: 30, bottom: 90, height: 60 }) });

function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, any>>(node)) return [];
  return [node, ...elements(node.props.children)];
}

function mount(overrides: Partial<Props> = {}) {
  let props: Props = {
    title: "Fixture session", className: "fixture-row", selecting: false,
    onSelect: vi.fn(), onRename: vi.fn(async () => undefined), children: "Fixture session", ...overrides
  };
  let tree: ReactNode;
  const hosts = new Map<Ref, ReturnType<typeof host>>();
  const render = (next: Partial<Props> = {}) => {
    props = { ...props, ...next };
    let remaining = 10;
    do {
      hooks.index = 0;
      hooks.dirty = false;
      tree = RenameableSession(props);
      for (const ref of hosts.keys()) ref.current = null;
      for (const element of elements(tree)) {
        const ref = (element as unknown as { ref?: Ref }).ref;
        if (typeof element.type !== "string" || !ref) continue;
        if (!hosts.has(ref)) hosts.set(ref, host());
        ref.current = hosts.get(ref);
      }
      hooks.effects.splice(0).forEach(effect => effect());
      if (!remaining--) throw new Error("Fixture render did not settle");
    } while (hooks.dirty);
  };
  const find = (predicate: (element: Element) => boolean) => elements(tree).find(predicate);
  const required = (predicate: (element: Element) => boolean) => {
    const element = find(predicate);
    if (!element) throw new Error("Expected fixture element is missing");
    return element;
  };
  render();
  return {
    render, find, props,
    row: () => required(element => element.type === "button" && element.props.title === props.title),
    input: () => required(element => element.type === "input"),
    menu: () => required(element => element.type === ContextTaskMenu),
    actions: () => required(element => element.type === ThreadMenuItems).props.actions as ComponentProps<typeof ThreadMenuItems>["actions"],
    host: (element: Element) => hosts.get((element as unknown as { ref: Ref }).ref)!
  };
}

const key = (value: string, overrides: Record<string, unknown> = {}) => ({
  key: value, shiftKey: false, preventDefault: vi.fn(), nativeEvent: { isComposing: false },
  currentTarget: host(), ...overrides
});
const click = (detail: number, overrides: Record<string, unknown> = {}) => ({
  detail, currentTarget: host(), ...overrides
});
const doubleClick = () => ({ preventDefault: vi.fn() });

beforeEach(() => {
  hooks.index = 0; hooks.dirty = false; hooks.cells.clear(); hooks.deps.clear(); hooks.cleanup.clear(); hooks.effects.length = 0;
  vi.useFakeTimers();
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => setTimeout(() => callback(0), 0));
});
afterEach(() => {
  hooks.cleanup.forEach(cleanup => cleanup?.());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("RenameableSession event boundaries", () => {
  test("multi-selected rows expose batch context menu actions on right-click and Shift+F10 while suppressing rename", () => {
    const deleteBatch = vi.fn();
    const clearBatch = vi.fn();
    const view = mount({
      selecting: true,
      multiSelected: true,
      batchMenuActions: [
        { id: "batch-delete", label: "删除所选 2 项…", danger: true, run: deleteBatch },
        { id: "batch-clear", label: "取消选择", run: clearBatch },
      ],
    });
    expect(view.row().props.className).toContain("multi-selected");
    expect(view.row().props["aria-pressed"]).toBe(true);
    view.row().props.onDoubleClick(doubleClick());
    view.row().props.onKeyDown(key("F2"));
    view.render();
    expect(view.find(element => element.type === "input")).toBeUndefined();

    view.row().props.onContextMenu({ preventDefault: vi.fn(), clientX: 24, clientY: 36 });
    view.render();
    expect(view.actions().map(action => action.id)).toEqual(["batch-delete", "batch-clear"]);
    view.actions()[0]!.run();
    expect(deleteBatch).toHaveBeenCalledTimes(1);
  });

  test("selects immediately on the first click and opens rename on double-click without selecting twice", () => {
    const view = mount();
    const firstClick = click(1);
    const secondClick = click(2);
    view.row().props.onClick(firstClick);
    expect(view.props.onSelect).toHaveBeenCalledTimes(1);
    expect(firstClick.currentTarget.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    view.render();
    view.row().props.onClick(secondClick);
    view.row().props.onDoubleClick(doubleClick());
    view.render();
    vi.runAllTimers();
    expect(view.props.onSelect).toHaveBeenCalledTimes(1);
    expect(secondClick.currentTarget.focus).not.toHaveBeenCalled();
    expect(view.input().props.value).toBe("Fixture session");
    expect(view.host(view.input()).focus).toHaveBeenCalledTimes(1);
    expect(view.host(view.input()).select).toHaveBeenCalledTimes(1);
  });

  test("keyboard-generated clicks select immediately", () => {
    const view = mount();
    const event = click(0);
    view.row().props.onClick(event);
    expect(view.props.onSelect).toHaveBeenCalledTimes(1);
    expect(event.currentTarget.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
  });

  test("forwards selection modifiers and leaves Ctrl click to the context menu", () => {
    const view = mount();
    const event = click(1, { metaKey: true, shiftKey: true });
    view.row().props.onClick(event);
    expect(view.props.onSelect).toHaveBeenCalledExactlyOnceWith(event);
    expect(event.currentTarget.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    const ctrlClick = click(1, { ctrlKey: true });
    view.row().props.onClick(ctrlClick);
    expect(view.props.onSelect).toHaveBeenCalledOnce();
    expect(ctrlClick.currentTarget.focus).not.toHaveBeenCalled();
    view.row().props.onDoubleClick({ ...doubleClick(), metaKey: true });
    view.render();
    expect(view.find(element => element.type === "input")).toBeUndefined();
  });

  test("F2 and Enter rename the trimmed draft and restore row focus", async () => {
    const view = mount();
    const rowHost = view.host(view.row());
    const f2 = key("F2");
    view.row().props.onKeyDown(f2);
    view.render();
    view.input().props.onChange({ target: { value: "  Renamed fixture  " } });
    view.render();
    view.input().props.onKeyDown(key("Enter"));
    await Promise.resolve();
    view.render();
    vi.runAllTimers();
    expect(f2.preventDefault).toHaveBeenCalledOnce();
    expect(view.props.onRename).toHaveBeenCalledExactlyOnceWith("Renamed fixture");
    expect(view.props.onSelect).not.toHaveBeenCalled();
    expect(view.find(element => element.type === "input")).toBeUndefined();
    expect(rowHost.focus).toHaveBeenCalledOnce();
  });

  test("Escape cancels editing without a rename from the subsequent blur", () => {
    const view = mount();
    view.row().props.onKeyDown(key("F2"));
    view.render();
    const input = view.input();
    input.props.onKeyDown(key("Escape"));
    input.props.onBlur();
    view.render();
    vi.runAllTimers();
    expect(view.props.onRename).not.toHaveBeenCalled();
    expect(view.find(element => element.type === "input")).toBeUndefined();
    expect(view.host(view.row()).focus).toHaveBeenCalledOnce();
  });

  test("IME confirmation does not save, and empty names keep the editor open", () => {
    const view = mount();
    view.row().props.onKeyDown(key("F2"));
    view.render();
    view.input().props.onChange({ target: { value: "  " } });
    view.render();
    const composing = key("Enter", { nativeEvent: { isComposing: true } });
    view.input().props.onKeyDown(composing);
    expect(composing.preventDefault).not.toHaveBeenCalled();
    view.input().props.onKeyDown(key("Enter"));
    view.render();
    expect(view.props.onRename).not.toHaveBeenCalled();
    expect(view.input().props["aria-invalid"]).toBe(true);
    expect(view.find(element => element.props.role === "alert")?.props.children).toBe("名称不能为空");
  });

  test("right-click opens the row's actions without selecting it and menu rename uses the same editor", () => {
    const inspect = vi.fn();
    const view = mount({ menuActions: [{ id: "inspect", label: "Inspect fixture", icon: null, run: inspect }] });
    const event = { clientX: 60, clientY: 80, preventDefault: vi.fn() };
    view.row().props.onContextMenu(event);
    view.render();
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(view.props.onSelect).not.toHaveBeenCalled();
    expect(view.menu().props).toMatchObject({ x: 60, y: 80, returnFocus: view.host(view.row()) });
    view.actions().find(action => action.id === "inspect")!.run();
    expect(inspect).toHaveBeenCalledOnce();
    expect(view.host(view.row()).focus).toHaveBeenCalledWith({ preventScroll: true });
    view.actions().find(action => action.id === "rename")!.run();
    view.menu().props.onClose();
    view.render();
    expect(view.input().props.value).toBe("Fixture session");
    expect(view.props.onSelect).not.toHaveBeenCalled();
  });

  test.each([key("ContextMenu"), key("F10", { shiftKey: true })])("keyboard menu shortcut $key preserves selection", event => {
    const view = mount();
    view.row().props.onKeyDown(event);
    view.render();
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(view.menu().props).toMatchObject({ x: 44, y: 66 });
    expect(view.props.onSelect).not.toHaveBeenCalled();
  });

  test("the ellipsis opens the shared menu without selecting the row", () => {
    const view = mount();
    view.find(element => element.props["aria-label"] === "线程操作")!.props.onClick({ currentTarget: host() });
    view.render();
    expect(view.menu().props).toMatchObject({ x: 20, y: 94 });
    expect(view.props.onSelect).not.toHaveBeenCalled();
  });

  test("batch selection handles every click and suppresses rename and menus", () => {
    const view = mount({ selecting: true });
    const firstClick = click(1);
    const secondClick = click(2);
    view.row().props.onClick(firstClick);
    view.row().props.onClick(secondClick);
    view.row().props.onDoubleClick(doubleClick());
    view.row().props.onKeyDown(key("F2"));
    view.row().props.onKeyDown(key("ContextMenu"));
    view.row().props.onContextMenu({ preventDefault: vi.fn(), clientX: 20, clientY: 20 });
    view.render();
    expect(view.props.onSelect).toHaveBeenCalledTimes(2);
    expect(firstClick.currentTarget.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    expect(secondClick.currentTarget.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
    expect(view.find(element => element.type === "input" || element.type === ContextTaskMenu || element.props["aria-label"] === "线程操作")).toBeUndefined();
  });

  test.each([{ selecting: true }, { disabled: true }])("entering $selecting/$disabled state closes editing and cancels a stale blur", state => {
    const view = mount();
    view.row().props.onKeyDown(key("F2"));
    view.render();
    view.input().props.onChange({ target: { value: "Changed fixture" } });
    view.render();
    const input = view.input();
    view.render(state);
    input.props.onBlur();
    expect(view.props.onRename).not.toHaveBeenCalled();
    expect(view.find(element => element.type === "input" || element.type === ContextTaskMenu)).toBeUndefined();
  });

  test("a blocked rename remains readable and the menu action is disabled", () => {
    const view = mount({ renameBlockReason: "Fixture session is active" });
    view.row().props.onKeyDown(key("F2"));
    view.row().props.onContextMenu({ preventDefault: vi.fn(), clientX: 20, clientY: 20 });
    view.render();
    expect(view.find(element => element.type === "input")).toBeUndefined();
    expect(view.find(element => element.props.role === "alert")?.props.children).toBe("Fixture session is active");
    expect(view.actions().find(action => action.id === "rename")).toMatchObject({ disabled: true, reason: "Fixture session is active" });
    expect(view.props.onRename).not.toHaveBeenCalled();
  });

  test("Enter followed by blur submits once while saving", async () => {
    let resolve!: () => void;
    const pending = new Promise<void>(done => { resolve = done; });
    const onRename = vi.fn(() => pending);
    const view = mount({ onRename });
    view.row().props.onKeyDown(key("F2"));
    view.render();
    view.input().props.onChange({ target: { value: "Changed fixture" } });
    view.render();
    view.input().props.onKeyDown(key("Enter"));
    view.input().props.onBlur();
    view.render();
    expect(onRename).toHaveBeenCalledExactlyOnceWith("Changed fixture");
    expect(view.input().props.disabled).toBe(true);
    resolve();
    await pending;
    view.render();
    expect(view.find(element => element.type === "input")).toBeUndefined();
  });
});
