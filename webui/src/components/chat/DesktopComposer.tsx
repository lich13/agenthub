import { ArrowUp, AtSign, Check, ChevronDown, Copy, FolderPlus, Paperclip, Plus, Square, Target, WandSparkles, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { chooseDesktopItem, createDesktopThread, desktopCatalog, desktopDiagnostics, type DesktopCatalogItem, type DesktopChosenItem, type DesktopMode } from "../../lib/query/desktop";
import { emptyReferenceDraft, editReferenceDraft, inlineReferenceQuery, insertDesktopReference, removeDesktopReference, toDesktopReferenceSpans } from "../../lib/desktopReferences";
import { useDesktopReferenceCatalog } from "../../hooks/useDesktopReferenceCatalog";
import { useCodexDesktop } from "../../hooks/useCodexDesktop";
import { useDesktopAttachments } from "../../hooks/useDesktopAttachments";
import { DesktopRequests } from "./DesktopRequests";
import { requestDesktopOpenIntent } from "../../lib/desktopIntent";
import { DesktopStatus } from "./DesktopStatus";
import { DesktopGoal } from "./DesktopGoal";
import { ConfirmDialog } from "../common/ConfirmDialog";

export function NewDesktopThread({ onCreated }: { onCreated: (id: string) => void | Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const created = useRef<string>();
  const [uncertain, setUncertain] = useState(false);
  const create = async () => {
    setBusy(true); setError("");
    try {
      if (!created.current) {
        const { item } = await chooseDesktopItem("project");
        if (!item) return;
        const result = await createDesktopThread(item.handle);
        created.current = result.threadId;
      }
      requestDesktopOpenIntent(created.current);
      await onCreated(created.current);
      created.current = undefined;
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : "无法新建线程";
      if (message.includes("结果未确认")) setUncertain(true);
      setError(message);
    }
    finally { setBusy(false); }
  };
  return <><button className="icon-button compact" title="新建 Codex 线程" disabled={busy || uncertain} onClick={() => { void create(); }}><FolderPlus size={16} /></button>
    {error && <span role="alert" className="desktop-create-error">{error}</span>}</>;
}

export function DesktopComposer({ threadId }: { threadId: string }) {
  const desktop = useCodexDesktop(threadId);
  const { view, busy } = desktop;
  const [draft, setDraft] = useState(emptyReferenceDraft);
  const { text, references } = draft;
  const textarea = useRef<HTMLTextAreaElement>(null);
  const attachmentRegion = useRef<HTMLFormElement>(null);
  const addMenu = useRef<HTMLDetailsElement>(null);
  const [copyState, setCopyState] = useState<"idle" | "copying" | "copied" | "failed">("idle");
  const currentSession = useRef<string>();
  currentSession.current = view?.sessionId;
  const currentComposer = useRef("");
  currentComposer.current = JSON.stringify([threadId, view?.sessionId]);
  const [selection, setSelection] = useState({ start: 0, end: 0, focused: false });
  const [dismissed, setDismissed] = useState("");
  const [selectedOption, setSelectedOption] = useState(0);
  const [catalogQuery, setCatalogQuery] = useState("");
  const [localError, setLocalError] = useState("");
  const [choosing, setChoosing] = useState(false);
  const [catalogKind, setCatalogKind] = useState("");
  const [discard, setDiscard] = useState<string | null>(null);
  const identity = useRef<{ payload: string; id: string }>();
  const discardTrigger = useRef<HTMLButtonElement>(null);
  const writable = Boolean(view?.connected && view.writable);
  useEffect(() => { setLocalError(""); setCatalogKind(""); setDiscard(null); setChoosing(false); setCopyState("idle"); if (addMenu.current) addMenu.current.open = false; }, [threadId, view?.sessionId]);
  const attachments = useDesktopAttachments(writable && !busy && !choosing ? view?.sessionId : undefined, attachmentRegion, references.map(item => item.handle), items => {
    setLocalError(""); desktop.clearActionError();
    setDraft(value => {
      const additions = items.filter(item => !value.references.some(reference => reference.handle === item.handle));
      if (value.references.length + additions.length > 32) {
        setLocalError("引用数量超出限制，本次附件未添加");
        attachments.release(additions.map(item => item.handle));
        return value;
      }
      return { ...value, references: [...value.references, ...additions] };
    });
  }, setLocalError);
  const active = Boolean(view?.activeTurnId);
  const query = inlineReferenceQuery(text, selection.start, selection.end);
  const queryKey = JSON.stringify(query);
  const inline = writable && selection.focused && !catalogKind && query && dismissed !== queryKey
    && !draft.spans.some(span => span.start <= selection.start && selection.start <= span.end) ? query : null;
  const catalog = useDesktopReferenceCatalog(view?.sessionId, writable ? catalogKind || inline?.kind || "" : "", catalogKind ? catalogQuery : inline?.query || "");
  const option = Math.min(selectedOption, Math.max(0, catalog.items.length - 1));
  const selectReference = (item: DesktopChosenItem) => {
    try {
      const { draft: next, caret } = insertDesktopReference(draft, item, inline?.start ?? selection.start, inline?.end ?? selection.end);
      setDraft(next); setSelection({ start: caret, end: caret, focused: true });
      setCatalogKind(""); setLocalError(""); desktop.clearActionError(); setDismissed("");
      queueMicrotask(() => { textarea.current?.focus(); textarea.current?.setSelectionRange(caret, caret); });
    } catch (reason) { setLocalError(reason instanceof Error ? reason.message : "无法插入所选引用"); }
  };
  const send = async (steer = false) => {
    if (!view) return;
    const handles = references.map(item => item.handle);
    const spans = toDesktopReferenceSpans(draft);
    const rangePayload = spans.length ? { referenceSpans: spans } : {};
    const payload = JSON.stringify({ text, handles, ...rangePayload, steer, turn: steer ? view.activeTurnId : null });
    if (identity.current?.payload !== payload) identity.current = { payload, id: crypto.randomUUID() };
    setLocalError("");
    try {
      if (steer && view.activeTurnId) await desktop.act({ type: "steer", messageId: identity.current.id, text, references: handles, ...rangePayload, turnId: view.activeTurnId });
      else await desktop.act({ type: "send", messageId: identity.current.id, text, references: handles, ...rangePayload });
      setDraft(emptyReferenceDraft()); setSelection({ start: 0, end: 0, focused: true }); identity.current = undefined;
    } catch { /* The hook owns the fixed error; keep the same ID for a retry. */ }
  };
  const attachFile = async () => {
    if (!view) return;
    const session = view.sessionId;
    setChoosing(true); setLocalError("");
    try {
      const { item } = await chooseDesktopItem("file", view.sessionId);
      if (item && currentSession.current === session) { desktop.clearActionError(); setDraft(value => {
        if (value.references.some(reference => reference.handle === item.handle)) return value;
        if (value.references.length >= 32) { setLocalError("引用数量超出限制，本次附件未添加"); return value; }
        return { ...value, references: [...value.references, item] };
      }); }
    } catch (reason) { if (currentSession.current === session) setLocalError(reason instanceof Error ? reason.message : "无法添加附件"); }
    finally { if (currentSession.current === session) setChoosing(false); }
  };
  const copyDiagnostics = async () => {
    if (copyState === "copying") return;
    const scope = currentComposer.current;
    setCopyState("copying");
    try {
      await desktopDiagnostics(true);
      if (currentComposer.current === scope) setCopyState("copied");
    } catch { if (currentComposer.current === scope) setCopyState("failed"); }
  };
  const selectCatalog = (kind: string) => {
    if (addMenu.current) addMenu.current.open = false;
    setCatalogKind(kind); setCatalogQuery(""); setSelectedOption(0);
  };
  const locked = busy || choosing;
  return <section className="desktop-composer" aria-label="与 Codex 协作">
    <div className="desktop-composer-heading">
      <DesktopStatus view={view} />
      <span className="desktop-composer-spacer" />
      {copyState === "copied" && <span className="desktop-copy-feedback" role="status">诊断已复制</span>}
      {copyState === "failed" && <span className="desktop-copy-feedback" role="alert">无法复制诊断，请重试</span>}
      <button type="button" className="icon-button compact desktop-diagnostics" title="复制诊断" aria-label="复制诊断" disabled={copyState === "copying"} onClick={() => { void copyDiagnostics(); }}>{copyState === "copied" ? <Check size={14} /> : <Copy size={14} />}</button>
    </div>
    {view?.connected && view.goal && <DesktopGoal key={view.sessionId} goal={view.goal} open={desktop.open} busy={busy} />}
      {view && view.queue.length > 0 && <details className="desktop-outbox" open={view.queue.some(item => item.status !== "queued")}>
        <summary>待发送 · {view.queue.length}</summary>
        <ol>{view.queue.map(item => <li key={item.id}>
          <span><small>{item.status === "outcome_unknown" ? "结果待核对" : item.status === "held" ? item.kind === "steer" ? "补充未发送，请取消后重试" : "已暂停" : item.status === "dispatching" ? "正在提交到 Codex…" : "等待线程空闲"}</small>{item.preview || "引用内容"}</span>
          {item.status === "outcome_unknown" ? <button ref={discardTrigger} disabled={locked || !writable} onClick={() => setDiscard(item.id)}>核对后移除</button>
            : ["queued", "held"].includes(item.status) && <button disabled={locked || !writable} onClick={() => { void desktop.act({ type: "cancelQueued", messageId: item.id }).catch(() => {}); }}>取消</button>}
        </li>)}</ol>
        {view.queue.some(item => item.status === "held" && item.kind === "queue") && <button disabled={locked || !writable} onClick={() => { void desktop.act({ type: "resumeQueue" }).catch(() => {}); }}>继续发送队列</button>}
      </details>}
    {!writable ? <div className="desktop-connect-row"><span role={desktop.error ? "alert" : undefined}>{view?.connectionPhase === "owner_confirmation_required" ? "Codex owner 已变化，已恢复阅读；点击连接确认后继续协作" : view?.connectionPhase === "resyncing" ? "正在重新同步 Codex…" : view?.connectionPhase === "reconnecting" ? "正在恢复 Codex 连接…" : desktop.error || (busy ? "正在连接 Codex…" : "连接本机 Codex 后继续此线程")}</span><button disabled={locked} onClick={() => { void desktop.open(); }}>在 Codex 中打开</button><button disabled={locked} onClick={desktop.connect}>连接</button></div> : <>
      <DesktopRequests requests={view!.requests} busy={locked} act={desktop.act} open={desktop.open} />
      <form ref={attachmentRegion} onSubmit={event => { event.preventDefault(); void send(); }}>
        {references.length > 0 && <div className="desktop-reference-chips">{references.map(item => <span key={item.handle}>{item.kind === "skill" ? <WandSparkles size={13} /> : item.kind === "plugin" || item.kind === "app" ? <AtSign size={13} /> : <Paperclip size={13} />}<span>{item.name}</span><button type="button" aria-label={`移除 ${item.name}`} disabled={locked} onClick={() => { attachments.release([item.handle]); setDraft(value => removeDesktopReference(value, item.handle)); setLocalError(""); desktop.clearActionError(); }}><X size={12} /></button></span>)}</div>}
        <textarea ref={textarea} aria-label="发送给 Codex" aria-autocomplete="list" aria-controls={inline ? "desktop-reference-options" : undefined} aria-expanded={Boolean(inline)} aria-activedescendant={inline && catalog.items[option] ? `desktop-reference-option-${option}` : undefined}
          placeholder={active ? "添加下一条消息，或补充当前回合…" : "与 Codex 协作…"} rows={2} maxLength={131072} value={text} disabled={locked}
          onPaste={attachments.paste}
          onFocus={() => setSelection(value => ({ ...value, focused: true }))} onBlur={() => setSelection(value => ({ ...value, focused: false }))}
          onSelect={event => setSelection({ start: event.currentTarget.selectionStart, end: event.currentTarget.selectionEnd, focused: true })}
          onChange={event => { setDraft(value => editReferenceDraft(value, event.target.value)); setSelection({ start: event.target.selectionStart ?? event.target.value.length, end: event.target.selectionEnd ?? event.target.value.length, focused: true }); setSelectedOption(0); }}
          onKeyDown={event => {
            if (event.nativeEvent.isComposing) return;
            if (inline && !event.metaKey && !event.ctrlKey) {
              if (event.key === "Escape") { event.preventDefault(); setDismissed(queryKey); return; }
              if (["ArrowDown", "ArrowUp"].includes(event.key) && catalog.items.length) { event.preventDefault(); setSelectedOption((option + (event.key === "ArrowDown" ? 1 : -1) + catalog.items.length) % catalog.items.length); return; }
              if (["Enter", "Tab"].includes(event.key) && !event.shiftKey && catalog.items[option]) { event.preventDefault(); selectReference(catalog.items[option]); return; }
            }
            if (event.key === "Enter" && !event.shiftKey && (event.metaKey || event.ctrlKey)) { event.preventDefault(); if (text.trim() || references.length) void send(); }
          }} />
        {inline && <ReferenceOptions catalog={catalog} option={option} onSelect={selectReference} onMore={() => { setSelectedOption(0); catalog.more(); }} />}
        <div className="desktop-composer-toolbar">
          <details className="desktop-add-menu" ref={addMenu} onKeyDown={event => { if (event.key === "Escape") { event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); } }} onBlur={event => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget as Node)) event.currentTarget.open = false; }}>
            <summary aria-label="添加引用或附件" title="添加引用或附件"><Plus size={19} /></summary>
            <div className="desktop-add-options">
              <span className="desktop-menu-label">添加</span>
              <button type="button" disabled={locked || references.length >= 32} onClick={() => { if (addMenu.current) addMenu.current.open = false; void attachFile(); }}><Paperclip size={17} /><span>文件或图片</span></button>
              <button type="button" disabled={locked || references.length >= 32} onClick={() => selectCatalog("skills")}><WandSparkles size={17} /><span>技能</span><kbd>$</kbd></button>
              <button type="button" disabled={locked || references.length >= 32} onClick={() => selectCatalog("mentions")}><AtSign size={17} /><span>插件与应用</span><kbd>@</kbd></button>
              <button type="button" disabled={locked} onClick={() => { if (addMenu.current) addMenu.current.open = false; void desktop.open(); }}><Target size={17} /><span>目标<small>在 Codex 中管理</small></span></button>
            </div>
          </details>
          <span className="desktop-composer-spacer" />
          <DesktopSettings sessionId={view!.sessionId} viewModel={view!.model} viewEffort={view!.reasoningEffort} viewMode={view!.mode} busy={locked} act={desktop.act} />
          {active && <><button type="button" disabled={locked || !text.trim() && !references.length} onClick={() => { void send(true); }}>补充</button><button type="button" className="icon-button compact" title="停止当前回合并暂停队列" disabled={locked} onClick={() => { void desktop.act({ type: "stop", turnId: view!.activeTurnId! }).catch(() => {}); }}><Square size={15} /></button></>}
          <button className="desktop-send" aria-label={active ? "排队发送" : "发送"} title={active ? "排队发送" : "发送（⌘ Enter）"} disabled={locked || !text.trim() && !references.length}><ArrowUp size={19} /></button>
        </div>
      </form>
      {catalogKind && <div className="desktop-reference-picker">
        <div className="desktop-reference-search"><select aria-label="引用类型" value={catalogKind} onChange={event => { setCatalogKind(event.target.value); setCatalogQuery(""); setSelectedOption(0); }}><option value="mentions">插件与应用</option><option value="skills">技能</option><option value="apps">应用</option><option value="plugins">插件</option></select><input aria-label="搜索引用" placeholder="搜索名称" value={catalogQuery} onChange={event => { setCatalogQuery(event.target.value); setSelectedOption(0); }} /><button className="icon-button compact" title="关闭引用" onClick={() => setCatalogKind("")}><X size={14} /></button></div>
        <ReferenceOptions catalog={catalog} option={option} onSelect={selectReference} onMore={() => { setSelectedOption(0); catalog.more(); }} />
      </div>}
    </>}
    {(localError || writable && desktop.error) && <p className="form-error" role="alert">{localError || desktop.error}</p>}
    {discard && <ConfirmDialog labelledBy="desktop-discard-title" busy={busy} onCancel={() => setDiscard(null)} returnFocus={discardTrigger}>
      <h3 id="desktop-discard-title">已在 Codex 中核对发送结果？</h3><p>移除此记录后，AgentHub 不会重新发送它。请先确认 Codex 中是否已有这条消息。</p>
      <div className="dialog-actions"><button disabled={busy} onClick={() => setDiscard(null)}>返回</button><button disabled={busy} onClick={() => { void desktop.act({ type: "dismissUnknown", messageId: discard }).then(() => setDiscard(null)).catch(() => {}); }}>已核对，移除记录</button></div>
    </ConfirmDialog>}
  </section>;
}

function useSettingsCatalog(sessionId: string, kind: string) {
  const [items, setItems] = useState<DesktopCatalogItem[]>([]);
  const [error, setError] = useState("");
  const [retryRequest, setRetryRequest] = useState({ sessionId: "", attempt: 0 });
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    let current = true; setLoading(true); setError(""); setItems([]);
    void desktopCatalog({ sessionId, projectHandle: null, kind, query: "", cursor: null, ...(retryRequest.sessionId === sessionId && retryRequest.attempt > 0 ? { forceRefetch: true } : {}) })
      .then(result => { if (current) { setItems(result.items); setError(""); } })
      .catch(reason => { if (current) setError(reason instanceof Error ? reason.message : "目录暂不可用"); })
      .finally(() => { if (current) setLoading(false); });
    return () => { current = false; };
  }, [sessionId, kind, retryRequest]);
  return { items, error, loading, retry: () => setRetryRequest(value => ({ sessionId, attempt: value.attempt + 1 })) };
}

function DesktopSettings({ sessionId, viewModel, viewEffort, viewMode, busy, act }: { sessionId: string; viewModel: string | null; viewEffort: string | null; viewMode: DesktopMode | null; busy: boolean; act: ReturnType<typeof useCodexDesktop>["act"] }) {
  const models = useSettingsCatalog(sessionId, "models");
  const modes = useSettingsCatalog(sessionId, "modes");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const currentSession = useRef(sessionId); currentSession.current = sessionId;
  useEffect(() => { setError(""); setNotice(""); }, [sessionId]);
  const selected = models.items.find(item => item.handle === viewModel);
  const availableModes = modes.items.filter(item => item.handle === "default" || item.handle === "plan");
  const update = (model: string, reasoningEffort: string | null, mode: DesktopMode) => {
    setError("");
    void act({ type: "settings", model, reasoningEffort, mode }).then(() => {
      if (currentSession.current === sessionId) setError("");
    }).catch(reason => {
      if (currentSession.current === sessionId) setError(reason instanceof Error ? reason.message : "设置未应用，请重试");
    });
  };
  return <details className="desktop-model-settings" onKeyDown={event => { if (event.key === "Escape") { event.currentTarget.open = false; event.currentTarget.querySelector("summary")?.focus(); } }} onBlur={event => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget as Node)) event.currentTarget.open = false; }}>
    <summary title="模型、思考强度与模式"><span>{selected?.name || viewModel || "模型设置"}</span>{viewEffort && <small>{viewEffort}</small>}{viewMode === "plan" && <small>Plan</small>}<ChevronDown size={13} /></summary>
    <div className="desktop-settings-options">
    {models.error && <><span role="alert" className="form-error">模型：{models.error}</span><button type="button" disabled={busy || models.loading} onClick={models.retry}>重试模型目录</button></>}
    {modes.error && <><span role="alert" className="form-error">模式：{modes.error}</span><button type="button" disabled={busy || modes.loading} onClick={modes.retry}>重试模式目录</button></>}
    {error && <span role="alert" className="form-error">{error}</span>}
    {notice && <span role="status">{notice}</span>}
    <label>模型<select aria-label="模型" disabled={busy || models.loading || !models.items.length} value={selected?.handle ?? ""} onChange={event => {
      const item = models.items.find(item => item.handle === event.target.value); if (item) { setNotice(""); update(item.handle, item.defaultReasoningEffort, viewMode ?? "default"); }
    }}><option value="" disabled>{viewModel || (models.loading ? "读取模型…" : "没有可用模型")}</option>{models.items.map(item => <option key={item.handle} value={item.handle}>{item.name}</option>)}</select></label>
    {selected && <label>思考强度<select aria-label="思考强度" disabled={busy || models.loading || !selected.reasoningEfforts.length} value={viewEffort ?? ""} onChange={event => { setNotice(""); update(selected.handle, event.target.value || null, viewMode ?? "default"); }}>
      <option value="">继承原生默认</option>{viewEffort && !selected.reasoningEfforts.includes(viewEffort) && <option value={viewEffort} disabled>{viewEffort}</option>}{selected.reasoningEfforts.map(effort => <option key={effort} value={effort}>{effort}</option>)}
    </select></label>}
    <label>模式<select aria-label="协作模式" disabled={busy || modes.loading || !selected || !availableModes.length} value={viewMode ?? ""} onChange={event => {
      if (!selected) return;
      const effort = viewEffort && selected.reasoningEfforts.includes(viewEffort) ? viewEffort : null;
      setNotice(viewEffort && effort === null ? "当前思考强度不可用，将继承原生默认" : "");
      update(selected.handle, effort, event.target.value as DesktopMode);
    }}><option value="" disabled>{modes.loading ? "读取模式…" : "没有可用模式"}</option>{viewMode && !availableModes.some(item => item.handle === viewMode) && <option value={viewMode} disabled>{viewMode === "plan" ? "Plan" : "默认"}</option>}{availableModes.map(item => <option key={item.handle} value={item.handle}>{item.name}</option>)}</select></label>
    </div>
  </details>;
}

function ReferenceOptions({ catalog, option, onSelect, onMore }: { catalog: ReturnType<typeof useDesktopReferenceCatalog>; option: number; onSelect: (item: DesktopChosenItem) => void; onMore: () => void }) {
  return <div className="desktop-reference-options" onMouseDown={event => event.preventDefault()}>
    {catalog.busy && <p className="muted-text" role="status">正在读取{catalog.loadingKinds?.map(kind => kind === "apps" ? "应用" : kind === "plugins" ? "插件" : "技能").join("、")}…</p>}
    {catalog.errors.map(error => <p role="alert" className="form-error" key={error.kind}>{error.kind === "apps" ? "应用：" : error.kind === "plugins" ? "插件：" : ""}{error.message}<button type="button" onClick={() => catalog.retry(error.kind)}>重试</button></p>)}
    <ul id="desktop-reference-options" role="listbox" aria-label="可用引用">
      {catalog.items.map((item, index) => <li id={`desktop-reference-option-${index}`} role="option" aria-selected={index === option} key={item.handle}><button type="button" onClick={() => onSelect(item)}><strong>{item.name}</strong><small>{item.kind === "plugin" ? "插件" : item.kind === "app" ? "应用" : "技能"}{item.description && ` · ${item.description}`}</small></button></li>)}
    </ul>
    {!catalog.busy && !catalog.errors.length && !catalog.items.length && <p className="muted-text">{catalog.emptyLabel || "没有可用引用"}</p>}
    {catalog.hasMore && <button type="button" disabled={catalog.busy} onClick={onMore}>下一页</button>}
  </div>;
}
