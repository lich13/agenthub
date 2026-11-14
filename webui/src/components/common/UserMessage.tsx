import { File, ImageOff, X } from "lucide-react";
import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { SessionProvider } from "../../lib/api/sessions";
import type { UserAttachment, UserMessageContent } from "../../types";
import { readAttachment } from "../../lib/query/attachments";
import { userMessageSegments } from "../../lib/domain/userMessageViewModel";
import { projectUserMessage, visibleContextText } from "../../lib/domain/userContext";
import { ActivityDetails, DisclosureRevealContext } from "./ActivityDetails";
import { matchesTimelinePosition } from "../../lib/domain/timelineViewModel";
import { CopyReplyButton } from "./CopyReplyButton";
import { FilePathLink } from "./FilePathLink";

export const UserMessageScope = createContext<{ provider: SessionProvider; sessionKey: string; rootThreadId?: string; attachments?: "names" } | null>(null);

export function UserMessage({ message, text = "", activityId = "user-message", timelineId, timelineAliases = [] }: { message?: UserMessageContent | null; text?: string; activityId?: string; timelineId?: string; timelineAliases?: string[] }) {
  const scope = useContext(UserMessageScope);
  const body = message?.text ?? text;
  const projection = useMemo(() => projectUserMessage(body, message?.contextBlocks), [body, message?.contextBlocks]);
  const segments = useMemo(() => userMessageSegments(projection.text, projection.contextBlocks), [projection]);
  const identity = message?.id ?? activityId;
  const positionKey = useContext(DisclosureRevealContext);
  const reveal = matchesTimelinePosition(positionKey, timelineId ?? identity, timelineAliases);
  if (!segments.length && !message?.attachments.length && !projection.contextBlocks.length) return null;
  return <article className="user-message" data-timeline-id={timelineId ?? identity} data-timeline-aliases={timelineAliases.length ? timelineAliases.join(" ") : undefined}>
    {!!message?.attachments.length && <div className="user-attachments" aria-label="消息附件">{message.attachments.map(attachment => scope?.attachments === "names" ? <div key={attachment.id} className="user-attachment file-attachment"><File size={18} /><span>{attachment.name}</span></div> : <Attachment key={attachment.id} messageId={message.id} attachment={attachment} />)}</div>}
    {!!projection.contextBlocks.length && <ActivityDetails className="user-instructions instruction-file execution-command" stateKey={`${identity}:context`} initiallyOpen={false} reveal={reveal} summary={<span className="tool-title">上下文</span>}>
      {() => <div className="user-instructions-body instruction-file-body">{projection.contextBlocks.map(block => visibleContextText(block.text)).join("\n\n")}</div>}
    </ActivityDetails>}
    {segments.map(segment => segment.kind === "reply" ? <div className="user-question-reply" key={segment.id}>
        <CopyReplyButton text={segment.answer} label="复制回答" copiedLabel="已复制回答" />
        <div className="user-message-bubble user-answer-bubble">
          {!!segment.question.trim() && <ActivityDetails className="user-question-context" stateKey={`${identity}:${segment.id}:question`} initiallyOpen={false} reveal={reveal} summary={<span>{segment.question}</span>}>
            {() => <div className="user-question-full">{segment.question}</div>}
          </ActivityDetails>}
          <div className="user-answer">{segment.answer}</div>
        </div>
      </div> : <div key={segment.id} className="user-message-bubble">{segment.text}</div>)}
  </article>;
}

function Attachment({ attachment, messageId }: { attachment: UserAttachment; messageId: string }) {
  const scope = useContext(UserMessageScope);
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const [visible, setVisible] = useState(false);
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  const [preview, setPreview] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const { provider, sessionKey, rootThreadId } = scope ?? {};
  useEffect(() => {
    if (!container.current || attachment.kind !== "image" || attachment.reason) return;
    if (typeof IntersectionObserver === "undefined") { setVisible(true); return; }
    const observer = new IntersectionObserver(entries => setVisible(entries.some(entry => entry.isIntersecting)), { rootMargin: "240px" });
    observer.observe(container.current);
    return () => observer.disconnect();
  }, [attachment.id, attachment.kind, attachment.reason]);
  useEffect(() => {
    if ((!visible && !preview) || !provider || !sessionKey || attachment.reason || attachment.kind !== "image") { setUrl(""); return; }
    let cancelled = false;
    setError("");
    readAttachment({ provider, sessionKey, messageId, attachmentId: attachment.id, ...(rootThreadId ? { rootThreadId } : {}) }).then(data => {
      if (!cancelled) setUrl(data);
    }, reason => { if (!cancelled) setError(reason instanceof Error ? reason.message : "图片暂时无法读取"); });
    return () => { cancelled = true; };
  }, [visible, preview, provider, sessionKey, rootThreadId, messageId, attachment.id, attachment.kind, attachment.reason, attempt]);
  useEffect(() => {
    if (preview) dialog.current?.showModal();
    else if (dialog.current?.open) dialog.current.close();
  }, [preview]);
  const reason = attachment.reason || error;
  const image = attachment.kind === "image";
  return <div ref={container} className={`user-attachment ${image ? "image-attachment" : "file-attachment"}`}>
    {image ? <button ref={trigger} type="button" className="attachment-thumbnail" aria-label={`预览图片：${attachment.name}`} title={reason || attachment.name} disabled={!url || !!reason} onClick={() => setPreview(true)}>
      {url && !reason ? <img src={url} alt={attachment.name} onError={() => setError("图片内容损坏，无法预览")} /> : <><ImageOff size={23} /><span>{reason ? "图片不可用" : "图片"}</span></>}
    </button> : <File size={18} />}
    {(!image || reason) && <div className="attachment-description">
      {attachment.path ? <FilePathLink href={attachment.path}>{attachment.name}</FilePathLink> : <span>{attachment.name}</span>}
      {reason && <small>{reason}</small>}
      {error && !attachment.reason && <button type="button" className="file-path-label" onClick={() => setAttempt(value => value + 1)}>重试预览</button>}
    </div>}
    {image && <dialog ref={dialog} className="attachment-preview" aria-label={`图片预览：${attachment.name}`} onClose={() => { setPreview(false); trigger.current?.focus(); }} onClick={event => { if (event.target === event.currentTarget) setPreview(false); }}>
      <header><span>{attachment.name}</span><button type="button" className="icon-button" aria-label="关闭图片预览" onClick={() => setPreview(false)} autoFocus><X size={20} /></button></header>
      {preview && url && <img src={url} alt={attachment.name} />}
    </dialog>}
  </div>;
}
