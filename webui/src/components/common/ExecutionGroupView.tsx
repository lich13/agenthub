import { Blocks, BookOpen, Search, Terminal } from "lucide-react";
import { memo, useContext } from "react";
import { ToolOutput } from "./FilePathLink";
import { ActivityDetails, DisclosureRevealContext } from "./ActivityDetails";
import { matchesTimelinePosition } from "../../lib/domain/timelineViewModel";
import { visibleMarkdown } from "../../lib/domain/visibleMarkdown";
import { activityFingerprint, type ExecutionCommand, type ExecutionGroup } from "../../lib/domain/executionGroups";
import { RunningIndicator } from "./RunningIndicator";

export function executionSummary(group: ExecutionGroup) {
  const kinds = new Set(group.commands.map(command => {
    const name = command.title.trim().split(/\s+/, 1)[0].toLowerCase().split(/[.\/:]/).pop() ?? "";
    if (/^(read|read_file|list|list_files|ls|cat)$/.test(name)) return "read";
    if (/^(search|search_query|search_web|grep|glob|find|web_search|ripgrep)$/.test(name)) return "search";
    if (/^(exec|exec_command|run_command|execute|bash|shell|terminal|sleep|js|javascript|python|node)$/.test(name)) return "command";
    return "integration";
  }));
  const words = [kinds.has("read") && "读取文件", kinds.has("search") && "搜索内容", kinds.has("command") && "运行命令", kinds.has("integration") && "调用工具"].filter(Boolean);
  const prefix = group.recordedOnly ? "" : kinds.has("integration") ? `${group.running ? "正在使用" : "已使用"} ${group.provider} 集成` : group.running ? "正在" : "已";
  return { label: `${prefix}${words.join("、")}`, Icon: kinds.has("integration") ? Blocks : kinds.has("command") ? Terminal : kinds.has("search") ? Search : BookOpen };
}

export const ExecutionCommandRow = memo(function CommandView({ command }: { command: ExecutionCommand }) {
  return <ActivityDetails timelineId={command.id} timelineAliases={command.sourceIds} className="execution-command" stateKey={command.id} initiallyOpen={!command.instructionFile && (command.status === "failed" || command.status === "running")} summary={<>
    <Terminal size={15} aria-hidden="true" /><span className="tool-title" title={command.instructionFile ? "AGENTS.md" : visibleMarkdown(`${command.title} ${command.preview ?? ""}`)}>{command.instructionFile ? "AGENTS.md" : <>{visibleMarkdown(command.title)}{command.preview && <span className="command-preview"> {visibleMarkdown(command.preview)}</span>}</>}</span>
    {command.status === "running" && <RunningIndicator />}
    <small>{{ completed: "已完成", running: "运行中", failed: "失败", unknown: "未见结果" }[command.status]}</small>
  </>}>
    {() => command.sections.length ? command.sections.map(section => <div className="execution-section" key={`${section.label}:${activityFingerprint(section.text)}`}>
      <span>{section.label}</span><ToolOutput text={section.text} />
    </div>) : <p className="muted-text">暂无输出</p>}
  </ActivityDetails>;
});

export const ExecutionGroupView = memo(function ExecutionGroupView({ group }: { group: ExecutionGroup }) {
  const { label, Icon } = executionSummary(group);
  const positionKey = useContext(DisclosureRevealContext);
  const reveal = Boolean(positionKey && group.commands.some(command => matchesTimelinePosition(positionKey, command.id, command.sourceIds)));
  const groupAliases = [...(group.mergedIds ?? []), ...group.commands.map(command => `group:${command.id}`)];
  return <ActivityDetails timelineId={group.id} timelineAliases={groupAliases} className="execution-group" stateKey={group.id} stateAliases={groupAliases} reveal={reveal} initiallyOpen={!group.commands.every(command => command.instructionFile) && (group.running || group.failedCount > 0)} summary={<>
    <Icon size={15} aria-hidden="true" />
    <span className="activity-label">{label}</span>
    <small>{group.commands.length} {group.kind === "tool" ? "项工具" : "条命令"}{group.failedCount ? ` · ${group.failedCount} 条失败` : ""}</small>
    {group.running && <RunningIndicator />}
  </>}>
    {() => <div className="execution-rows" data-history-scroll-container="commands">{group.commands.map(command => <ExecutionCommandRow key={command.id} command={command} />)}</div>}
  </ActivityDetails>;
});
