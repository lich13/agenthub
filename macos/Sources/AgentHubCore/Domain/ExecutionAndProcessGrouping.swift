import Foundation

// MARK: - Conversation Block Classification & 60/3 Compaction (conversationViewModel.ts)

public enum ConversationViewModel {
    private static let internalPrefixes: [String] = [
        "<environment_context>", "<permissions instructions>", "<app-context>",
        "<collaboration_mode>", "<skills_instructions>", "<plugins_instructions>",
        "<subagent_notification>", "<subagent_context>", "<codex_internal_context",
        "<goal_context>", "<additional_context>", "<user_instructions>",
        "<turn_aborted>", "<user_shell_command>",
        "<legacy_unified_exec_process_limit_warning>",
        "<legacy_apply_patch_exec_command_warning>",
        "<legacy_model_mismatch_warning>",
        "========= memory_summary begins =========",
    ]

    public static func shouldAutoFollow(
        scrollTop: CGFloat,
        clientHeight: CGFloat,
        scrollHeight: CGFloat,
        threshold: CGFloat = 96
    ) -> Bool {
        scrollHeight - scrollTop - clientHeight <= threshold
    }

    public static func isPlanBlock(_ block: MessageBlock) -> Bool {
        if block.role == "user" { return false }
        let k = block.kind.lowercased()
        let dk = block.displayKind?.lowercased() ?? ""
        return dk == "plan" || k.contains("plan") || (block.text?.contains("<proposed_plan>") == true)
    }

    public static func isApprovalBlock(_ block: MessageBlock) -> Bool {
        let k = block.kind.lowercased()
        let dk = block.displayKind?.lowercased() ?? ""
        return dk == "approval" || k.contains("requestapproval") || k.contains("approval") || k.contains("permissions/request")
    }

    public static func isQuestionBlock(_ block: MessageBlock) -> Bool {
        let k = block.kind.lowercased()
        let dk = block.displayKind?.lowercased() ?? ""
        return dk == "question"
            || k.contains("request_user_input")
            || k.contains("requestuserinput")
            || !block.questions.isEmpty
    }

    public static func isQuestionResultBlock(_ block: MessageBlock) -> Bool {
        let k = block.kind.lowercased()
        let dk = block.displayKind?.lowercased() ?? ""
        return dk == "question_result"
            || k == "request_user_input_result"
            || !(block.answers ?? []).isEmpty
    }

    public static func isToolBlock(_ block: MessageBlock) -> Bool {
        let k = block.kind.lowercased()
        return block.role == "tool"
            || k.contains("tool")
            || k.contains("command")
            || k.contains("function_call")
            || k.contains("web_search")
    }

    public static func historyCollapseKind(_ block: MessageBlock) -> String? {
        switch block.kind.lowercased() {
        case "chat_history_collapsed": "chat"
        case "tool_history_collapsed": "tool"
        case "action_history_collapsed": "action"
        default: nil
        }
    }

    public static func isHistoryCollapsedBlock(_ block: MessageBlock) -> Bool {
        historyCollapseKind(block) != nil
    }

    public static func shouldRenderConversationMessage(_ block: MessageBlock) -> Bool {
        let role = block.role
        let k = block.kind.lowercased()
        guard role == "assistant" || role == "user" else { return false }
        if !block.questions.isEmpty { return false }
        if k == "request_user_input" || k == "requestuserinput" { return false }
        if isToolBlock(block) || isPlanBlock(block) || isApprovalBlock(block) { return false }
        let trimmedStart = (block.text ?? "").replacingOccurrences(of: "^\\s+", with: "", options: .regularExpression).lowercased()
        if role == "user" {
            if trimmedStart.hasPrefix("<user_shell_command>")
                || trimmedStart.hasPrefix("<turn_aborted>")
                || trimmedStart.hasPrefix("========= memory_summary begins =========")
            {
                return false
            }
        } else if internalPrefixes.contains(where: { trimmedStart.hasPrefix($0) }) {
            return false
        }
        return k != "reasoning" && k != "agent_reasoning" && k != "session_meta"
    }

    public static func shouldRenderConversationBlock(_ block: MessageBlock) -> Bool {
        if isApprovalBlock(block) { return false }
        if block.subagent != nil || isPlanBlock(block) || isQuestionBlock(block) || isQuestionResultBlock(block) {
            return true
        }
        return isToolBlock(block) || shouldRenderConversationMessage(block)
    }

    public static func isResolvedActionBlock(_ block: MessageBlock) -> Bool {
        if block.resolved == true { return true }
        let st = (block.planStatus ?? block.status ?? "").lowercased()
        return [
            "completed", "complete", "succeeded", "success", "done",
            "approved", "declined", "rejected", "cancelled", "canceled", "failed",
        ].contains(st)
    }

    public static func compactConversationBlocks(
        _ blocks: [MessageBlock],
        showAllHistory: Bool,
        maxChatMessages: Int = 60,
        maxActionBlocks: Int = 3,
        revealedBlockId: String? = nil
    ) -> [MessageBlock] {
        let renderable = blocks.filter(shouldRenderConversationBlock)
        if showAllHistory { return renderable }
        if renderable.contains(where: { historyCollapseKind($0) == "action" }) {
            return renderable
        }

        let actionIndices = renderable.enumerated().compactMap { index, b -> Int? in
            if b.id == revealedBlockId || isHistoryCollapsedBlock(b) { return nil }
            return (isPlanBlock(b) || isQuestionBlock(b) || isQuestionResultBlock(b)) ? index : nil
        }
        let actionCompacted = compactIndexed(
            renderable,
            indices: actionIndices,
            maxVisible: maxActionBlocks,
            id: "action-history-collapsed",
            kind: "action_history_collapsed",
            toolName: "action_history",
            label: "条历史计划/问题已折叠"
        )

        if actionCompacted.contains(where: { historyCollapseKind($0) == "chat" }) {
            return actionCompacted
        }
        let chatIndices = actionCompacted.enumerated().compactMap { index, b -> Int? in
            (b.id != revealedBlockId && shouldRenderConversationMessage(b)) ? index : nil
        }
        return compactIndexed(
            actionCompacted,
            indices: chatIndices,
            maxVisible: maxChatMessages,
            id: "chat-history-collapsed",
            kind: "chat_history_collapsed",
            toolName: "chat_history",
            label: "条历史对话已折叠"
        )
    }

    private static func compactIndexed(
        _ blocks: [MessageBlock],
        indices: [Int],
        maxVisible: Int,
        id: String,
        kind: String,
        toolName: String,
        label: String
    ) -> [MessageBlock] {
        guard indices.count > maxVisible else { return blocks }
        let keep = Set(indices.suffix(maxVisible))
        let hide = Set(indices.dropLast(maxVisible))
        var compacted: [MessageBlock] = []
        var segmentIds: [String] = []
        var segmentHeadId: String?
        var inserted = false

        let flushSegment = {
            guard !segmentIds.isEmpty, let headId = segmentHeadId else { return }
            let count = segmentIds.count
            let summaryText = "\(count) \(label)"
            compacted.append(MessageBlock(
                id: inserted ? "\(id):\(headId)" : id,
                role: "tool",
                kind: kind,
                status: "completed",
                text: summaryText,
                summary: summaryText,
                truncated: false,
                toolName: toolName,
                collapsedIds: segmentIds
            ))
            inserted = true
            segmentIds = []
            segmentHeadId = nil
        }

        for (idx, block) in blocks.enumerated() {
            if hide.contains(idx) && !keep.contains(idx) {
                if segmentHeadId == nil { segmentHeadId = block.id }
                segmentIds.append(block.id)
            } else {
                flushSegment()
                compacted.append(block)
            }
        }
        flushSegment()
        return compacted
    }

    public static func toolBlockDetailText(_ block: MessageBlock) -> String {
        var sections: [String] = []
        if let input = block.input?.trimmingCharacters(in: .whitespacesAndNewlines), !input.isEmpty {
            sections.append("Input\n\(input)")
        }
        let rawOut = block.text?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !rawOut.isEmpty {
            sections.append(sections.isEmpty ? rawOut : "Output\n\(rawOut)")
        }
        if block.truncated == true {
            sections.append("[output truncated]")
        }
        return sections.isEmpty ? "No output" : sections.joined(separator: "\n\n")
    }

    public static func messageBlockText(_ block: MessageBlock) -> String {
        VisibleMarkdownEngine.visibleMarkdown(block.text?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "")
    }
}

// MARK: - Execution & Process Grouping (executionGroups.ts, processGroups.ts, claudeProcessGroups.ts)

public enum ExecutionStatus: String, Equatable, Sendable {
    case running
    case failed
    case completed
    case unknown

    public var label: String {
        switch self {
        case .completed: "已完成"
        case .running: "运行中"
        case .failed: "失败"
        case .unknown: "未见结果"
        }
    }
}

public struct ExecutionSection: Equatable, Sendable {
    public var label: String
    public var text: String

    public init(label: String, text: String) {
        self.label = label
        self.text = text
    }
}

public struct ExecutionCommand: Equatable, Sendable, Identifiable {
    public var id: String
    public var sourceIds: [String]
    public var title: String
    public var preview: String?
    public var status: ExecutionStatus
    public var instructionFile: Bool
    public var sections: [ExecutionSection]

    public init(
        id: String,
        sourceIds: [String],
        title: String,
        preview: String? = nil,
        status: ExecutionStatus,
        instructionFile: Bool = false,
        sections: [ExecutionSection] = []
    ) {
        self.id = id
        self.sourceIds = sourceIds
        self.title = title
        self.preview = preview
        self.status = status
        self.instructionFile = instructionFile
        self.sections = sections
    }
}

public struct ExecutionGroup: Equatable, Sendable, Identifiable {
    public var id: String
    public var mergedIds: [String]
    public var commands: [ExecutionCommand]
    public var kind: String // "command" | "tool"
    public var provider: String
    public var running: Bool
    public var recordedOnly: Bool
    public var failedCount: Int
    public var turnId: String?

    public var initiallyOpen: Bool {
        !commands.allSatisfy(\.instructionFile) && (running || failedCount > 0)
    }

    public var summaryHeader: (label: String, icon: LucideIconName) {
        var kinds = Set<String>()
        for cmd in commands {
            let head = cmd.title.trimmingCharacters(in: .whitespacesAndNewlines)
                .components(separatedBy: .whitespaces).first?.lowercased() ?? ""
            let leaf = head.components(separatedBy: CharacterSet(charactersIn: "./:")).last ?? head
            if ["read", "read_file", "list", "list_files", "ls", "cat"].contains(leaf) {
                kinds.insert("read")
            } else if ["search", "search_query", "search_web", "grep", "glob", "find", "web_search", "ripgrep"].contains(leaf) {
                kinds.insert("search")
            } else if ["exec", "exec_command", "run_command", "execute", "bash", "shell", "terminal", "sleep", "js", "javascript", "python", "node"].contains(leaf) {
                kinds.insert("command")
            } else {
                kinds.insert("integration")
            }
        }
        var words: [String] = []
        if kinds.contains("read") { words.append("读取文件") }
        if kinds.contains("search") { words.append("搜索内容") }
        if kinds.contains("command") { words.append("运行命令") }
        if kinds.contains("integration") { words.append("调用工具") }
        let prefix: String = if recordedOnly {
            ""
        } else if kinds.contains("integration") {
            "\(running ? "正在使用" : "已使用") \(provider) 集成"
        } else {
            running ? "正在" : "已"
        }
        let icon: LucideIconName = if kinds.contains("integration") {
            .blocks
        } else if kinds.contains("command") {
            .terminal
        } else if kinds.contains("search") {
            .search
        } else {
            .bookOpen
        }
        return ("\(prefix)\(words.joined(separator: "、"))", icon)
    }
}

public enum ExecutionRenderItem<T: Equatable & Sendable>: Equatable, Sendable {
    case item(item: T, key: String, sourceIds: [String])
    case group(ExecutionGroup)
}

public enum ExecutionGroupingEngine {
    private static let runningSet: Set<String> = ["pending", "running", "in_progress", "inprogress", "active", "generating"]
    private static let failedSet: Set<String> = ["failed", "error", "cancelled", "canceled", "rejected"]
    private static let commandNames: Set<String> = [
        "exec", "exec_command", "run_command", "run_terminal_command", "sleep", "js", "javascript",
        "bash", "bash_execution", "bashexecution", "shell", "terminal", "python", "node",
        "execute", "command_execution", "commandexecution",
    ]

    public static func executionStatus(_ raw: String?) -> ExecutionStatus {
        let s = raw?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() ?? ""
        if runningSet.contains(s) { return .running }
        if failedSet.contains(s) { return .failed }
        return .completed
    }

    public static func isCommandText(_ raw: String?) -> Bool {
        let leaf = normalizedCommandLeaf(raw)
        return commandNames.contains(leaf)
    }

    public static func activityFingerprint(_ value: String) -> String {
        let ns = value as NSString
        var hash: UInt32 = 2_166_136_261
        for i in 0 ..< ns.length {
            hash ^= UInt32(ns.character(at: i))
            hash = hash &* 16_777_619
        }
        return String(hash, radix: 36)
    }

    private static func normalizedCommandLeaf(_ raw: String?) -> String {
        guard let raw else { return "" }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let head = trimmed.components(separatedBy: CharacterSet(charactersIn: " \t\n`(")).first ?? ""
        return head.replacingOccurrences(of: "__", with: "/")
            .components(separatedBy: CharacterSet(charactersIn: "./:")).last ?? ""
    }

    private static func commandTitle(_ values: [String?]) -> String {
        for v in values {
            let leaf = normalizedCommandLeaf(v)
            guard commandNames.contains(leaf) else { continue }
            if leaf == "bashexecution" || leaf == "bash_execution" { return "bash" }
            if leaf == "commandexecution" || leaf == "command_execution" { return "execute" }
            if leaf == "run_terminal_command" || leaf == "run_command" { return "exec" }
            return leaf
        }
        for v in values {
            if let t = v?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty {
                return t.components(separatedBy: .whitespaces).first ?? "命令"
            }
        }
        return "命令"
    }

    private static func activityPreview(_ sources: [String?]) -> String? {
        for source in sources {
            guard let source, !source.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }
            var text = source
            if source.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("{"),
               let data = source.data(using: .utf8),
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            {
                for key in ["cmd", "command", "title", "path", "file_path", "query", "code"] {
                    if let val = json[key] as? String, !val.isEmpty {
                        text = val
                        break
                    }
                }
            }
            let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
                .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !cleaned.isEmpty {
                let ns = cleaned as NSString
                return ns.length > 180 ? "\(ns.substring(to: 180))…" : cleaned
            }
        }
        return nil
    }

    private struct ActivityInput<T> {
        var item: T
        var key: String
        var sourceIds: [String]
        var callId: String?
        var turnId: String?
        var tool: Bool
        var command: Bool
        var instructionFile: Bool
        var title: String
        var preview: String?
        var phase: String // "call" | "result" | "update"
        var status: String?
        var sections: [ExecutionSection]
    }

    private enum FlatActivityEntry<T> {
        case item(item: T, key: String, sourceIds: [String])
        case command(index: Int, isCommand: Bool, newTurn: Bool, turnId: String?)
    }

    private static func groupActivities<T: Equatable & Sendable>(
        _ activities: [ActivityInput<T>],
        provider: String,
        recordedOnly: Bool = false
    ) -> [ExecutionRenderItem<T>] {
        var flatEntries: [FlatActivityEntry<T>] = []
        var commandsPool: [ExecutionCommand] = []
        var pendingByCallId: [String: Int] = [:]
        var occurrences: [String: Int] = [:]
        var turn: String?

        for act in activities {
            let occ = occurrences[act.key, default: 0]
            occurrences[act.key] = occ + 1
            let key = "\(act.key):\(occ)"
            let newTurn = (act.turnId != nil && turn != nil && act.turnId != turn)
            if newTurn { pendingByCallId.removeAll() }
            if let t = act.turnId { turn = t }

            if act.tool, let cid = act.callId, act.phase != "call", let prevIdx = pendingByCallId[cid] {
                commandsPool[prevIdx].instructionFile = commandsPool[prevIdx].instructionFile || act.instructionFile
                if act.status != nil || act.phase == "result" {
                    commandsPool[prevIdx].status = executionStatus(act.status)
                }
                for sec in act.sections where !commandsPool[prevIdx].sections.contains(sec) {
                    commandsPool[prevIdx].sections.append(sec)
                }
                for sid in act.sourceIds where !commandsPool[prevIdx].sourceIds.contains(sid) {
                    commandsPool[prevIdx].sourceIds.append(sid)
                }
                if act.phase == "result" { pendingByCallId.removeValue(forKey: cid) }
                continue
            }

            if !act.tool {
                flatEntries.append(.item(item: act.item, key: key, sourceIds: act.sourceIds))
                continue
            }

            var status: ExecutionStatus = if act.status != nil {
                executionStatus(act.status)
            } else {
                act.phase == "call" ? .running : .completed
            }
            if recordedOnly && status == .running {
                status = .unknown
            }
            let cmdIdx = commandsPool.count
            commandsPool.append(ExecutionCommand(
                id: key,
                sourceIds: act.sourceIds,
                title: act.title,
                preview: act.preview,
                status: status,
                instructionFile: act.instructionFile,
                sections: act.sections
            ))
            if let cid = act.callId, !cid.isEmpty {
                pendingByCallId[cid] = cmdIdx
            }
            flatEntries.append(.command(index: cmdIdx, isCommand: act.command, newTurn: newTurn, turnId: act.turnId ?? turn))
        }

        var output: [ExecutionRenderItem<T>] = []
        for entry in flatEntries {
            switch entry {
            case let .item(item, key, sourceIds):
                output.append(.item(item: item, key: key, sourceIds: sourceIds))
            case let .command(idx, isCommand, newTurn, turnId):
                let cmd = commandsPool[idx]
                if !newTurn, case var .group(lastGroup) = output.last {
                    lastGroup.commands.append(cmd)
                    if !isCommand { lastGroup.kind = "tool" }
                    if lastGroup.turnId == nil { lastGroup.turnId = turnId }
                    lastGroup.running = lastGroup.commands.contains { $0.status == .running }
                    lastGroup.failedCount = lastGroup.commands.filter { $0.status == .failed }.count
                    output[output.count - 1] = .group(lastGroup)
                } else {
                    let group = ExecutionGroup(
                        id: "group:\(cmd.id)",
                        mergedIds: [],
                        commands: [cmd],
                        kind: isCommand ? "command" : "tool",
                        provider: provider,
                        running: cmd.status == .running,
                        recordedOnly: recordedOnly,
                        failedCount: cmd.status == .failed ? 1 : 0,
                        turnId: turnId
                    )
                    output.append(.group(group))
                }
            }
        }
        return output
    }

    public static func groupCodexBlocks(_ blocks: [MessageBlock]) -> [ExecutionRenderItem<MessageBlock>] {
        let inputs = blocks.map { block -> ActivityInput<MessageBlock> in
            let isResult = block.kind.range(of: "output|result", options: [.regularExpression, .caseInsensitive]) != nil
            let isTool = block.subagent == nil
                && ConversationViewModel.isToolBlock(block)
                && !ConversationViewModel.isHistoryCollapsedBlock(block)
                && !ConversationViewModel.isPlanBlock(block)
                && !ConversationViewModel.isQuestionBlock(block)
                && !ConversationViewModel.isQuestionResultBlock(block)
            let detail = isTool ? ConversationViewModel.toolBlockDetailText(block) : ""
            return ActivityInput(
                item: block,
                key: "codex:\(block.turnId ?? ""):\(block.callId ?? block.itemId ?? "event"):\(block.id)",
                sourceIds: [block.id, block.callId, block.itemId].compactMap { $0 }.filter { !$0.isEmpty },
                callId: block.callId,
                turnId: block.turnId,
                tool: isTool,
                command: isTool && (isCommandText(block.toolName) || isCommandText(block.kind)),
                instructionFile: isTool && VisibleMarkdownEngine.isInstructionFileActivity([block.toolName, block.input, block.summary, detail]),
                title: commandTitle([block.toolName, block.kind]),
                preview: isTool ? activityPreview([block.input]) : nil,
                phase: isResult ? "result" : "call",
                status: block.status,
                sections: isTool ? [ExecutionSection(label: isResult ? "结果" : "命令详情", text: detail)] : []
            )
        }
        return groupActivities(inputs, provider: "Codex")
    }

    public static func groupClaudeEvents(_ events: [ClaudeHistoryEvent], recordedOnly: Bool = false) -> [ExecutionRenderItem<ClaudeHistoryEvent>] {
        let inputs = events.map { ev -> ActivityInput<ClaudeHistoryEvent> in
            let isTool = (ev.kind == "tool_call" || ev.kind == "tool_result")
            var secs: [ExecutionSection] = []
            if let d = ev.detail, !d.isEmpty { secs.append(ExecutionSection(label: "调用参数", text: d)) }
            if let r = ev.result, !r.isEmpty { secs.append(ExecutionSection(label: "结果", text: r)) }
            return ActivityInput(
                item: ev,
                key: "claude:\(ev.id)",
                sourceIds: [ev.id],
                callId: ev.callId,
                turnId: ev.turnId,
                tool: isTool,
                command: isCommandText(ev.role) || isCommandText(ev.text),
                instructionFile: VisibleMarkdownEngine.isInstructionFileActivity([ev.role, ev.text, ev.detail, ev.result]),
                title: ev.role ?? ev.text?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "工具活动",
                preview: activityPreview([ev.detail]),
                phase: ev.kind == "tool_result" ? "result" : "call",
                status: ev.status,
                sections: secs
            )
        }
        return groupActivities(inputs, provider: "Claude Code", recordedOnly: recordedOnly)
    }

    public static func groupGrokEvents(_ events: [GrokHistoryEvent]) -> [ExecutionRenderItem<GrokHistoryEvent>] {
        let inputs = events.enumerated().map { index, ev -> ActivityInput<GrokHistoryEvent> in
            let isTool = ev.kind.hasPrefix("tool_")
            let cmd = isTool && (isCommandText(ev.method) || isCommandText(ev.text))
            let title = cmd ? commandTitle([ev.method, ev.text]) : (ev.text?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "工具活动")
            var secs: [ExecutionSection] = []
            if cmd, let t = ev.text, !t.isEmpty, t != title {
                secs.append(ExecutionSection(label: "命令", text: t))
            }
            if let d = ev.detail, !d.isEmpty {
                secs.append(ExecutionSection(label: cmd ? "命令详情" : "工具详情", text: d))
            }
            let fp = activityFingerprint(isTool ? (ev.text ?? ev.detail ?? "") : "")
            let key = "grok:\(ev.id ?? ev.userMessage?.id ?? ev.callId ?? "\(ev.timestamp ?? ""):\(ev.kind):\(fp)")"
            return ActivityInput(
                item: ev,
                key: key,
                sourceIds: [ev.id, "event:\(index)", ev.callId].compactMap { $0 }.filter { !$0.isEmpty },
                callId: ev.callId,
                turnId: nil,
                tool: isTool,
                command: cmd,
                instructionFile: VisibleMarkdownEngine.isInstructionFileActivity([ev.method, ev.text, ev.detail]),
                title: title.isEmpty ? "工具活动" : title,
                preview: activityPreview([ev.kind == "tool_call" ? ev.detail : nil, (cmd && ev.text != title) ? ev.text : nil]),
                phase: ev.kind == "tool_call" ? "call" : (ev.kind == "tool_result" ? "result" : "update"),
                status: ev.status,
                sections: secs
            )
        }
        return groupActivities(inputs, provider: "Grok")
    }

    public static func groupProviderEvents(
        _ events: [ProviderHistoryEvent],
        provider: String
    ) -> [ExecutionRenderItem<ProviderHistoryEvent>] {
        let prefix = provider == "Antigravity" ? "antigravity" : (provider == "Grok Bot" ? "grokbot" : "dsh")
        let inputs = events.map { ev -> ActivityInput<ProviderHistoryEvent> in
            let isTool = (ev.kind == "tool_call" || ev.kind == "tool_result")
            var secs: [ExecutionSection] = []
            if let d = ev.detail, !d.isEmpty { secs.append(ExecutionSection(label: "调用参数", text: d)) }
            if let r = ev.result, !r.isEmpty { secs.append(ExecutionSection(label: "结果", text: r)) }
            let title = isCommandText(ev.role)
                ? commandTitle([ev.role, ev.text])
                : (ev.role ?? ev.text?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "工具活动")
            return ActivityInput(
                item: ev,
                key: "\(prefix):\(ev.id)",
                sourceIds: [ev.id, ev.callId].compactMap { $0 }.filter { !$0.isEmpty },
                callId: ev.callId,
                turnId: ev.turnId,
                tool: isTool,
                command: isCommandText(ev.role) || isCommandText(ev.text),
                instructionFile: VisibleMarkdownEngine.isInstructionFileActivity([ev.role, ev.text, ev.detail, ev.result]),
                title: title.isEmpty ? "工具活动" : title,
                preview: activityPreview([ev.detail, (ev.text != ev.role) ? ev.text : nil]),
                phase: ev.kind == "tool_result" ? "result" : "call",
                status: ev.status,
                sections: secs
            )
        }
        return groupActivities(inputs, provider: provider)
    }
}

// MARK: - Unified Process Folding (processGroups.ts)

public enum ProcessStep: Equatable, Sendable {
    case thinking(id: String, sourceIds: [String], text: String, running: Bool, turnId: String?)
    case execution(group: ExecutionGroup, turnId: String?)
    case detail(id: String, sourceIds: [String], title: String, detail: String, running: Bool, failed: Bool, turnId: String?)

    public var anchorId: String {
        switch self {
        case let .thinking(id, _, _, _, _): id
        case let .execution(group, _): group.id
        case let .detail(id, _, _, _, _, _, _): id
        }
    }

    public var stepTurnId: String? {
        switch self {
        case let .thinking(_, _, _, _, turnId): turnId
        case let .execution(group, turnId): turnId ?? group.turnId
        case let .detail(_, _, _, _, _, _, turnId): turnId
        }
    }
}

public struct UnifiedProcessGroup: Equatable, Sendable, Identifiable {
    public var id: String
    public var stateKey: String
    public var stateAliases: [String]
    public var timelineAliases: [String]
    public var sourceIds: [String]
    public var steps: [ProcessStep]
    public var thoughtCount: Int
    public var toolCount: Int
    public var failedCount: Int
    public var running: Bool

    public var summaryText: (primary: String, meta: String?) {
        ProcessGroupingEngine.processGroupSummaryText(self)
    }

    public var iconName: LucideIconName {
        thoughtCount > 0 ? (toolCount > 0 ? .sparkles : .brain) : .terminal
    }
}

public enum ProcessFoldItem<T: Equatable & Sendable>: Equatable, Sendable {
    case passthrough(T)
    case execution(ExecutionGroup)
    case process(UnifiedProcessGroup)
}

public enum ProcessClassifyResult: Equatable, Sendable {
    case step(ProcessStep)
    case skip
    case passthrough
}

public enum ProcessGroupingEngine {
    public static func uniqueStrings(_ values: [String]) -> [String] {
        var seen = Set<String>()
        var out: [String] = []
        for value in values where !value.isEmpty && seen.insert(value).inserted {
            out.append(value)
        }
        return out
    }

    public static func buildUnifiedProcessGroup(prefix: String, steps: [ProcessStep]) -> UnifiedProcessGroup {
        var thoughtCount = 0
        var toolCount = 0
        var failedCount = 0
        var running = false
        var rawSourceIds: [String] = []
        var rawTimelineAliases: [String] = []
        var rawStateAliases: [String] = []

        for step in steps {
            switch step {
            case let .thinking(id, sourceIds, _, stepRunning, _):
                thoughtCount += 1
                if stepRunning { running = true }
                rawSourceIds.append(id)
                rawSourceIds.append(contentsOf: sourceIds)
                rawTimelineAliases.append(id)
                rawTimelineAliases.append(contentsOf: sourceIds)
                rawStateAliases.append(id)
            case let .execution(group, _):
                toolCount += group.commands.count
                failedCount += group.failedCount
                if group.running { running = true }
                rawStateAliases.append(group.id)
                rawStateAliases.append(contentsOf: group.commands.map { "group:\($0.id)" })
                rawTimelineAliases.append(group.id)
                rawTimelineAliases.append(contentsOf: group.commands.map { "group:\($0.id)" })
                for cmd in group.commands {
                    rawSourceIds.append(cmd.id)
                    rawSourceIds.append(contentsOf: cmd.sourceIds)
                    rawTimelineAliases.append(cmd.id)
                    rawTimelineAliases.append(contentsOf: cmd.sourceIds)
                }
            case let .detail(id, sourceIds, _, _, stepRunning, stepFailed, _):
                toolCount += 1
                if stepFailed { failedCount += 1 }
                if stepRunning { running = true }
                rawSourceIds.append(id)
                rawSourceIds.append(contentsOf: sourceIds)
                rawTimelineAliases.append(id)
                rawTimelineAliases.append(contentsOf: sourceIds)
                rawStateAliases.append(id)
            }
        }

        let anchorId = steps.first?.anchorId ?? "empty"
        let id = "\(prefix):process:\(anchorId)"
        return UnifiedProcessGroup(
            id: id,
            stateKey: id,
            stateAliases: uniqueStrings(rawStateAliases),
            timelineAliases: uniqueStrings(rawTimelineAliases),
            sourceIds: uniqueStrings(rawSourceIds),
            steps: steps,
            thoughtCount: thoughtCount,
            toolCount: toolCount,
            failedCount: failedCount,
            running: running
        )
    }

    public static func foldProcessSequence<T: Equatable & Sendable>(
        prefix: String,
        items: [T],
        classify: (T) -> ProcessClassifyResult
    ) -> [ProcessFoldItem<T>] {
        var output: [ProcessFoldItem<T>] = []
        var pending: [ProcessStep] = []
        var activeTurnId: String?

        let flush = {
            guard !pending.isEmpty else { return }
            if pending.count == 1, case let .execution(group, _) = pending[0] {
                output.append(.execution(group))
            } else {
                output.append(.process(buildUnifiedProcessGroup(prefix: prefix, steps: pending)))
            }
            pending = []
            activeTurnId = nil
        }

        for item in items {
            switch classify(item) {
            case .skip:
                continue
            case let .step(step):
                let stepTurnId = step.stepTurnId
                if let stepTurnId, !stepTurnId.isEmpty, let activeTurnId, !activeTurnId.isEmpty, stepTurnId != activeTurnId {
                    flush()
                }
                if let stepTurnId, !stepTurnId.isEmpty {
                    activeTurnId = stepTurnId
                }
                pending.append(step)
            case .passthrough:
                flush()
                output.append(.passthrough(item))
            }
        }
        flush()
        return output
    }

    public static func thinkingStepFromText(
        id: String,
        sourceIds: [String],
        rawText: String?,
        running: Bool = false,
        turnId: String? = nil
    ) -> ProcessClassifyResult {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(rawText ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleaned.isEmpty else { return .skip }
        return .step(.thinking(
            id: id,
            sourceIds: uniqueStrings([id] + sourceIds),
            text: rawText ?? "",
            running: running,
            turnId: turnId
        ))
    }

    public static func processGroupSummaryText(_ group: UnifiedProcessGroup) -> (primary: String, meta: String?) {
        if group.running {
            if group.thoughtCount > 0 && group.toolCount > 0 {
                return ("正在思考与执行", "\(group.toolCount) 项操作")
            }
            if group.thoughtCount > 0 {
                return ("正在思考…", group.thoughtCount > 1 ? "\(group.thoughtCount) 段推理" : nil)
            }
            return ("正在执行", "\(group.toolCount) 项操作")
        }
        if group.thoughtCount > 0 && group.toolCount > 0 {
            return ("已思考", "\(group.toolCount) 项操作")
        }
        if group.thoughtCount > 0 {
            return ("已思考", group.thoughtCount > 1 ? "\(group.thoughtCount) 段推理" : nil)
        }
        return ("已执行操作", "\(group.toolCount) 项操作")
    }
}

// MARK: - Claude Turn & Metadata Process Grouping (claudeProcessGroups.ts)

public struct ClaudeMetadataGroup: Equatable, Sendable, Identifiable {
    public var id: String
    public var sourceIds: [String]
    public var label: String
    public var entries: [ExecutionRenderItem<ClaudeHistoryEvent>]
}

public enum ClaudeEventRenderItem: Equatable, Sendable {
    case item(item: ClaudeHistoryEvent, key: String, sourceIds: [String])
    case group(ExecutionGroup)
    case metadata(ClaudeMetadataGroup)
}

public struct ClaudeProcessGroup: Equatable, Sendable, Identifiable {
    public var id: String
    public var stateKey: String
    public var turnId: String
    public var sourceIds: [String]
    public var entries: [ClaudeEventRenderItem]
    public var segmentCount: Int
    public var toolCount: Int
    public var failedCount: Int
    public var running: Bool
    public var completed: Bool
    public var recordedOnly: Bool
    public var needsReply: Bool
    public var needsPermission: Bool
    public var hasPermission: Bool

    public var iconName: LucideIconName {
        segmentCount > 0 && toolCount > 0 ? .sparkles : .brain
    }

    public var labelText: String {
        if recordedOnly { return "工作记录" }
        if running {
            if segmentCount > 0 && toolCount > 0 { return "正在思考与执行" }
            if segmentCount > 0 { return "正在思考…" }
            return "正在执行"
        }
        return segmentCount > 0 ? "已思考" : "过程"
    }

    public var metaText: String? {
        if toolCount > 0 { return "\(toolCount) 项工具" }
        if segmentCount > 1 { return "\(segmentCount) 段" }
        return nil
    }
}

public enum ClaudeRenderItem: Equatable, Sendable {
    case item(item: ClaudeHistoryEvent, key: String, sourceIds: [String])
    case group(ExecutionGroup)
    case metadata(ClaudeMetadataGroup)
    case process(ClaudeProcessGroup)
}

public enum ClaudeProcessGroupingEngine {
    private static let metadataKinds: Set<String> = [
        "file-history-snapshot", "file-history-delta", "queue-operation",
        "cost-state", "atis-latch", "attachment", "compaction", "branch_summary",
    ]
    private static let automaticRoles: Set<String> = [
        "native_context", "local_command", "task_notification",
        "turn_duration", "away_summary", "stop_hook_summary",
    ]
    private static let processKinds: Set<String> = [
        "thinking", "tool_call", "tool_result", "tool_reference",
        "failure", "permission_request", "permission_response",
        "file-history-snapshot", "file-history-delta", "queue-operation",
        "cost-state", "atis-latch", "attachment",
    ]

    public static func isAutomatic(_ event: ClaudeHistoryEvent) -> Bool {
        metadataKinds.contains(event.kind) || (event.kind == "system" && automaticRoles.contains(event.role ?? ""))
    }

    public static func isProcess(_ event: ClaudeHistoryEvent) -> Bool {
        processKinds.contains(event.kind)
            || (event.kind == "system" && (automaticRoles.contains(event.role ?? "") || event.role == "api_error"))
    }

    public static func foldAutomatic(_ entries: [ExecutionRenderItem<ClaudeHistoryEvent>]) -> [ClaudeEventRenderItem] {
        var result: [ClaudeEventRenderItem] = []
        var pending: [(item: ClaudeHistoryEvent, key: String, sourceIds: [String])] = []

        let flush = {
            guard !pending.isEmpty else { return }
            let sourceIds = pending.flatMap(\.sourceIds)
            let label: String = if pending.allSatisfy({ $0.item.kind == "compaction" || $0.item.kind == "branch_summary" }) {
                "压缩摘要"
            } else if pending.allSatisfy({ $0.item.role == "native_context" }) {
                "上下文"
            } else {
                "系统活动"
            }
            let firstId = sourceIds.first ?? "meta"
            let wrapped = pending.map { ExecutionRenderItem.item(item: $0.item, key: $0.key, sourceIds: $0.sourceIds) }
            result.append(.metadata(ClaudeMetadataGroup(
                id: "claude-metadata:\(firstId)",
                sourceIds: sourceIds,
                label: label,
                entries: wrapped
            )))
            pending = []
        }

        for entry in entries {
            if case let .item(item, key, sourceIds) = entry, isAutomatic(item) {
                pending.append((item, key, sourceIds))
                continue
            }
            if case let .group(group) = entry,
               !pending.isEmpty,
               pending.allSatisfy({ $0.item.kind != "compaction" && $0.item.kind != "branch_summary" }),
               case var .group(prevGroup) = result.last,
               (prevGroup.turnId == nil || group.turnId == nil || prevGroup.turnId == group.turnId)
            {
                prevGroup.commands.append(contentsOf: group.commands)
                prevGroup.mergedIds.append(group.id)
                prevGroup.mergedIds.append(contentsOf: group.mergedIds)
                if group.kind == "tool" { prevGroup.kind = "tool" }
                if prevGroup.turnId == nil { prevGroup.turnId = group.turnId }
                prevGroup.running = prevGroup.commands.contains { $0.status == .running }
                prevGroup.failedCount = prevGroup.commands.filter { $0.status == .failed }.count
                result[result.count - 1] = .group(prevGroup)
                continue
            }
            flush()
            switch entry {
            case let .item(item, key, sourceIds):
                result.append(.item(item: item, key: key, sourceIds: sourceIds))
            case let .group(group):
                result.append(.group(group))
            }
        }
        flush()
        return result
    }

    private static func promote(_ items: [ClaudeEventRenderItem]) -> [ClaudeRenderItem] {
        items.map { entry in
            switch entry {
            case let .item(item, key, sourceIds): .item(item: item, key: key, sourceIds: sourceIds)
            case let .group(group): .group(group)
            case let .metadata(meta): .metadata(meta)
            }
        }
    }

    private static func groupTurn(
        _ events: [ClaudeHistoryEvent],
        turnId: String,
        ended: Bool,
        recordedOnly: Bool
    ) -> [ClaudeRenderItem] {
        let entries = ExecutionGroupingEngine.groupClaudeEvents(events, recordedOnly: recordedOnly)
        let questionIds = Set(events.filter { $0.role == "AskUserQuestion" }.map(\.id))
        var output: [ClaudeRenderItem] = []
        var finalIndex = -1
        for (idx, entry) in entries.enumerated() {
            if case let .item(item, _, _) = entry, item.assistantPhase == "final" {
                finalIndex = idx
            }
        }

        var pending: [ExecutionRenderItem<ClaudeHistoryEvent>] = []
        let flush = { (completed: Bool) in
            guard !pending.isEmpty else { return }
            let segment = pending
            pending = []
            if segment.allSatisfy({
                if case let .item(item, _, _) = $0 { return isAutomatic(item) }
                return false
            }) {
                output.append(contentsOf: promote(foldAutomatic(segment)))
                return
            }
            let commands = segment.flatMap { entry -> [ExecutionCommand] in
                if case let .group(g) = entry { return g.commands }
                return []
            }
            let processEvents = segment.compactMap { entry -> ClaudeHistoryEvent? in
                if case let .item(item, _, _) = entry { return item }
                return nil
            }
            let sourceIds = segment.flatMap { entry -> [String] in
                switch entry {
                case let .item(_, _, sids): sids
                case let .group(g): g.commands.flatMap(\.sourceIds)
                }
            }
            let closed = ended || completed || processEvents.contains { $0.kind == "failure" }
            let pendingEvent = { (ev: ClaudeHistoryEvent) -> Bool in
                !recordedOnly && !closed && ExecutionGroupingEngine.executionStatus(ev.status) == .running
            }
            let needsReply = !closed && commands.contains { cmd in
                cmd.status == .running && cmd.sourceIds.contains { questionIds.contains($0) }
            }
            let needsPermission = processEvents.contains { $0.kind == "permission_request" && pendingEvent($0) }
            let firstSid = sourceIds.first ?? "0"
            let stateKey = "claude-process:\(turnId):\(firstSid)"
            let failedCount = commands.filter { $0.status == .failed }.count
                + processEvents.filter { ExecutionGroupingEngine.executionStatus($0.status) == .failed }.count
            let isRunning = !closed && !needsReply && !needsPermission
                && (commands.contains { $0.status == .running } || processEvents.contains(where: pendingEvent))
            output.append(.process(ClaudeProcessGroup(
                id: "claude-process[\(turnId)][\(firstSid)]",
                stateKey: stateKey,
                turnId: turnId,
                sourceIds: sourceIds,
                entries: foldAutomatic(segment),
                segmentCount: processEvents.count,
                toolCount: commands.count,
                failedCount: failedCount,
                running: isRunning,
                completed: !recordedOnly && completed,
                recordedOnly: recordedOnly,
                needsReply: needsReply,
                needsPermission: needsPermission,
                hasPermission: processEvents.contains { $0.kind == "permission_request" }
            )))
        }

        for (index, entry) in entries.enumerated() {
            switch entry {
            case .group:
                pending.append(entry)
            case let .item(item, _, _):
                if isProcess(item) {
                    pending.append(entry)
                } else {
                    flush(finalIndex >= index)
                    output.append(contentsOf: promote(foldAutomatic([entry])))
                }
            }
        }
        flush(false)
        return output
    }

    public static func groupClaudeProcesses(_ events: [ClaudeHistoryEvent], recordedOnly: Bool = false) -> [ClaudeRenderItem] {
        var output: [ClaudeRenderItem] = []
        var turn: String?
        var hasTurn = false
        var pending: [ClaudeHistoryEvent] = []

        let flush = { (ended: Bool) in
            guard !pending.isEmpty else { return }
            if let turn, !turn.isEmpty {
                output.append(contentsOf: groupTurn(pending, turnId: turn, ended: ended, recordedOnly: recordedOnly))
            } else {
                output.append(contentsOf: promote(foldAutomatic(ExecutionGroupingEngine.groupClaudeEvents(pending, recordedOnly: recordedOnly))))
            }
            pending = []
        }

        for event in events {
            if !hasTurn || event.turnId != turn {
                if hasTurn { flush(true) }
                turn = event.turnId
                hasTurn = true
            }
            if event.kind == "user_message" || event.kind == "compaction" || event.kind == "branch_summary" {
                flush(true)
                output.append(contentsOf: promote(foldAutomatic(ExecutionGroupingEngine.groupClaudeEvents([event], recordedOnly: recordedOnly))))
            } else {
                pending.append(event)
            }
        }
        flush(false)
        return output
    }
}

// MARK: - Pure In-Memory FIFO 2000-Key Activity Disclosure Store (Cleared on Relaunch)

@MainActor
public final class ActivityDisclosureStore: ObservableObject {
    public static let maxEntries = 2000
    public static let shared = ActivityDisclosureStore()

    private var values: [String: Bool] = [:]
    private var order: [String] = []
    @Published public private(set) var revision: UInt64 = 0

    public init() {}

    public func isOpen(
        scope: String,
        stateKey: String,
        aliases: [String] = [],
        initiallyOpen: Bool
    ) -> Bool {
        let fullKey = "\(scope)::\(stateKey)"
        if let v = values[fullKey] { return v }
        for alias in aliases {
            if let v = values["\(scope)::\(alias)"] { return v }
        }
        return initiallyOpen
    }

    public func setOpen(
        scope: String,
        stateKey: String,
        isOpen: Bool
    ) {
        let fullKey = "\(scope)::\(stateKey)"
        if values[fullKey] == nil {
            if order.count >= Self.maxEntries, let oldest = order.first {
                order.removeFirst()
                values.removeValue(forKey: oldest)
            }
            order.append(fullKey)
        }
        values[fullKey] = isOpen
        revision &+= 1
    }

    public var count: Int { values.count }

    public func clearAll() {
        values.removeAll()
        order.removeAll()
        revision &+= 1
    }
}
