import { Check, Copy } from "lucide-react";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { visibleMarkdown } from "../../lib/domain/visibleMarkdown";

export function CopyReplyButton({
  text,
  label = "复制回复",
  copiedLabel = "已复制回复",
}: {
  text: string;
  label?: string;
  copiedLabel?: string;
}) {
  const { state, copy } = useCopyFeedback(2000);
  const cleaned = visibleMarkdown(text);
  if (!cleaned.trim()) return null;

  return (
    <div
      className="reply-actions"
      data-feedback={state === "idle" ? undefined : state}
    >
      <button
        type="button"
        className="icon-button copy-reply"
        aria-label={state === "copied" ? copiedLabel : label}
        title={state === "copied" ? "已复制" : label}
        onClick={() => {
          void copy(cleaned);
        }}
      >
        {state === "copied" ? <Check size={17} /> : <Copy size={17} />}
      </button>
      <span
        aria-live="polite"
        className={state === "failed" ? "reply-copy-error" : "visually-hidden"}
      >
        {state === "failed" ? "复制失败，请重试或手动选择文本复制。" : ""}
      </span>
    </div>
  );
}
