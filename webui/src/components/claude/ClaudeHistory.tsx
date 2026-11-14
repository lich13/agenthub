import type { ReactNode } from "react";
import type { ClaudeRenderItem } from "../../lib/domain/claudeProcessGroups";
import { foldProcessSequence, thinkingStepFromText } from "../../lib/domain/processGroups";
import { ActivityDetails } from "../common/ActivityDetails";
import { ExecutionGroupView } from "../common/ExecutionGroupView";
import { ProcessGroupView } from "../common/ProcessGroupView";
import { StandardProviderEventView } from "../common/WorkspaceSplitShell";
import { ClaudeProcessView } from "./ClaudeProcessView";

function isFoldableClaudeCore(entry: ClaudeRenderItem): boolean {
  if (entry.kind === "group") return true;
  if (entry.kind !== "item") return false;
  if (entry.item.kind === "thinking") return true;
  return Boolean(
    entry.item.detail &&
      entry.item.kind !== "user_message" &&
      entry.item.kind !== "plan" &&
      !entry.item.kind.startsWith("assistant_message"),
  );
}

function claudeEntryTurnId(entry: ClaudeRenderItem): string | null | undefined {
  if (entry.kind === "process") return entry.process.turnId;
  if (entry.kind === "group") return entry.group.turnId;
  if (entry.kind === "item") return entry.item.turnId;
  const first = entry.group.entries[0];
  return first?.kind === "item" ? first.item.turnId : first?.group.turnId;
}

function isNonCompactionMetadata(
  entry: ClaudeRenderItem | undefined,
): entry is Extract<ClaudeRenderItem, { kind: "metadata" }> {
  return Boolean(entry && entry.kind === "metadata" && entry.group.label !== "压缩摘要");
}

function sameOrUnsetTurn(a: string | null | undefined, b: string | null | undefined): boolean {
  return !a || !b || a === b;
}

export function renderClaudeEvents(
  entries: ClaudeRenderItem[],
  title: string,
  insideProcess = false,
): ReactNode {
  if (!insideProcess) {
    const indexMap = new Map<ClaudeRenderItem, number>();
    entries.forEach((entry, idx) => indexMap.set(entry, idx));

    const folded = foldProcessSequence("claude", entries, entry => {
      if (entry.kind === "group") {
        return { kind: "execution", group: entry.group, turnId: entry.group.turnId };
      }
      if (
        entry.kind === "item" &&
        entry.item.kind === "thinking"
      ) {
        return thinkingStepFromText(entry.key, entry.sourceIds, entry.item.text, false, entry.item.turnId);
      }
      if (isNonCompactionMetadata(entry)) {
        const idx = indexMap.get(entry) ?? -1;
        const turnId = claudeEntryTurnId(entry);
        let left = idx - 1;
        while (
          left >= 0 &&
          isNonCompactionMetadata(entries[left]) &&
          sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[left]))
        ) {
          left -= 1;
        }
        let right = idx + 1;
        while (
          right < entries.length &&
          isNonCompactionMetadata(entries[right]) &&
          sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[right]))
        ) {
          right += 1;
        }
        const attachedLeft =
          left >= 0 && isFoldableClaudeCore(entries[left]) && sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[left]));
        const attachedRight =
          right < entries.length &&
          isFoldableClaudeCore(entries[right]) &&
          sameOrUnsetTurn(turnId, claudeEntryTurnId(entries[right]));
        if (attachedLeft || attachedRight) {
          const detailText = entry.group.entries
            .map(sub => (sub.kind === "item" ? sub.item.detail || sub.item.text || "" : ""))
            .filter(Boolean)
            .join("\n\n");
          return {
            kind: "detail",
            id: entry.group.id,
            sourceIds: entry.group.sourceIds,
            title: entry.group.label,
            detail: detailText || entry.group.label,
            turnId,
          };
        }
      }
      if (
        entry.kind === "item" &&
        entry.item.detail &&
        entry.item.kind !== "user_message" &&
        entry.item.kind !== "plan" &&
        !entry.item.kind.startsWith("assistant_message")
      ) {
        return {
          kind: "detail",
          id: entry.key,
          sourceIds: entry.sourceIds,
          title: entry.item.text ?? "活动记录",
          detail: entry.item.detail,
          failed: entry.item.kind === "failure" || /^(failed|error)$/i.test(entry.item.status ?? ""),
          turnId: entry.item.turnId,
        };
      }
      return null;
    });

    return folded.map(entry =>
      entry.kind === "process" ? (
        <ProcessGroupView key={entry.process.id} group={entry.process} />
      ) : entry.kind === "execution" ? (
        <ExecutionGroupView key={entry.group.id} group={entry.group} />
      ) : (
        renderSingleClaudeEntry(entry.item, title, false)
      ),
    );
  }

  return entries.map(entry => renderSingleClaudeEntry(entry, title, true));
}

function renderSingleClaudeEntry(entry: ClaudeRenderItem, title: string, insideProcess: boolean): ReactNode {
  if (entry.kind === "process") {
    return (
      <ClaudeProcessView key={entry.process.id} group={entry.process}>
        {() => renderClaudeEvents(entry.process.entries, title, true)}
      </ClaudeProcessView>
    );
  }
  if (entry.kind === "metadata") {
    return (
      <ActivityDetails
        key={entry.group.id}
        className="execution-group claude-metadata-group"
        stateKey={entry.group.id}
        stateAliases={entry.group.sourceIds.map(id => `claude-metadata:${id}`)}
        timelineId={entry.group.id}
        timelineAliases={entry.group.sourceIds}
        initiallyOpen={false}
        summary={<span className="tool-title">{entry.group.label}</span>}
      >
        {() => renderClaudeEvents(entry.group.entries, title, insideProcess)}
      </ActivityDetails>
    );
  }
  if (entry.kind === "group") {
    return <ExecutionGroupView key={entry.group.id} group={entry.group} />;
  }
  return (
    <StandardProviderEventView
      key={entry.key}
      activityId={entry.key}
      event={entry.item}
      title={title}
      providerLabel="Claude Code"
      timelineAliases={entry.sourceIds}
      insideProcess={insideProcess}
    />
  );
}
