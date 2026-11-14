import { useContext, type ReactNode } from "react";
import { Brain, Sparkles } from "lucide-react";
import { ActivityDetails, DisclosureRevealContext } from "../common/ActivityDetails";
import { RunningIndicator } from "../common/RunningIndicator";
import type { ClaudeProcessGroup } from "../../lib/domain/claudeProcessGroups";

export function ClaudeProcessView({ group, children }: { group: ClaudeProcessGroup; children: () => ReactNode }) {
  const position = useContext(DisclosureRevealContext);
  const reveal = Boolean(
    position &&
      (group.sourceIds.includes(position) ||
        group.entries.some(entry =>
          entry.kind === "item"
            ? entry.key === position || entry.sourceIds.includes(position)
            : entry.kind === "metadata"
              ? entry.group.sourceIds.includes(position)
              : entry.group.commands.some(
                  command => command.id === position || command.sourceIds.includes(position),
                ),
        )),
  );
  const Icon = group.segmentCount > 0 && group.toolCount > 0 ? Sparkles : Brain;
  const label = group.running
    ? group.segmentCount > 0 && group.toolCount > 0
      ? "正在思考与执行"
      : group.segmentCount > 0
        ? "正在思考…"
        : "正在执行"
    : group.segmentCount > 0
      ? "已思考"
      : "过程";
  const metaParts: string[] = [];
  if (group.toolCount > 0) metaParts.push(`${group.toolCount} 项工具`);
  else if (group.segmentCount > 1) metaParts.push(`${group.segmentCount} 段`);

  return (
    <ActivityDetails
      className={`execution-group process-group claude-process-group${group.running ? " is-running" : ""}`}
      stateKey={group.stateKey}
      stateAliases={group.sourceIds.map(id => `claude-process:${group.turnId}:${id}`)}
      timelineId={group.id}
      initiallyOpen={false}
      reveal={reveal}
      summary={
        <>
          <Icon size={15} aria-hidden="true" />
          <span className="activity-label">{label}</span>
          {metaParts.length > 0 && <small>{metaParts.join(" · ")}</small>}
          {group.failedCount > 0 && (
            <span className="process-failure-badge form-error">{group.failedCount} 项失败</span>
          )}
          {group.needsReply && <small>待回复</small>}
          {group.hasPermission && <small>{group.needsPermission ? "待授权" : "授权记录"}</small>}
          {group.running ? (
            <RunningIndicator />
          ) : (
            (group.needsReply || group.needsPermission) && <small>展开查看</small>
          )}
        </>
      }
    >
      {() => <div className="claude-process-body process-group-body">{children()}</div>}
    </ActivityDetails>
  );
}
