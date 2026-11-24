@testable import AgentHubCore
import AppKit
import Foundation
import SwiftUI
import XCTest

final class DomainAndMarkdownParitySuiteTests: XCTestCase {
    // MARK: - 1. visibleMarkdown.test.ts (14 cases)

    func testPorted_visibleMarkdown() throws {
        let cleanMatrix: [(id: String, input: String, expected: String)] = [
            ("plain", "Hello world", "Hello world"),
            ("strip-memory-citation", "Answer <oai-mem-citation>doc-1</oai-mem-citation> done", "Answer  done"),
            ("strip-citation-entries", "<citation_entries>a</citation_entries>Body", "Body"),
            ("strip-rollout-ids", "Start <rollout_ids>r-1</rollout_ids>", "Start"),
            ("strip-json-memory-envelope", "{\"memory_citation\":{\"ids\":[\"1\"]}}", ""),
            ("strip-standalone-memory-field", "memory_citation: {\"a\": 1}\nReal line", "Real line"),
            ("strip-external-open-page", "Before <external_codex_apps_open_page>{\"page_id\":\"p1\"}</external_codex_apps_open_page> After", "Before  After"),
            ("preserve-fenced-tag", "```xml\n<oai-mem-citation>keep</oai-mem-citation>\n```", "```xml\n<oai-mem-citation>keep</oai-mem-citation>\n```"),
            ("preserve-inline-code-tag", "Use `<oai-mem-citation>x</oai-mem-citation>` literally", "Use `<oai-mem-citation>x</oai-mem-citation>` literally"),
            ("preserve-blockquote-page-tag", "> <external_codex_apps_open_page>{\"page_id\":\"p1\"}</external_codex_apps_open_page>", "> <external_codex_apps_open_page>{\"page_id\":\"p1\"}</external_codex_apps_open_page>"),
            ("empty-string", "", ""),
        ]
        for c in cleanMatrix {
            XCTAssertEqual(VisibleMarkdownEngine.visibleMarkdown(c.input), c.expected, c.id)
        }

        XCTAssertTrue(VisibleMarkdownEngine.isInstructionFileActivity(["cat AGENTS.md"]))
        XCTAssertTrue(VisibleMarkdownEngine.isInstructionFileActivity(["read_file \"/repo/sub/AGENTS.md\""]))
        XCTAssertFalse(VisibleMarkdownEngine.isInstructionFileActivity(["cat README.md"]))

        let folded = VisibleMarkdownEngine.instructionSegments(
            "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nrule1\nrule2\n</INSTRUCTIONS>\n\nReal reply"
        )
        XCTAssertEqual(folded.map(\.kind), ["instructions", "markdown"])
        XCTAssertEqual(folded.last?.text.trimmingCharacters(in: .whitespacesAndNewlines), "Real reply")
    }

    // MARK: - 2. userContext.test.ts (13 cases)

    func testPorted_userContext() throws {
        let cases: [(String, String, Int, String)] = [
            ("plain-request", "Fix bug", 0, "Fix bug"),
            ("env-context-extracted", "<environment_context>\n<cwd>/tmp</cwd>\n</environment_context>\nRun tests", 1, "Run tests"),
            ("instructions-extracted", "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\na\n</INSTRUCTIONS>\n\nBuild app", 1, "Build app"),
            ("multiple-blocks", "<environment_context>\ncwd\n</environment_context>\n<permissions instructions>\nsandbox\n</permissions instructions>\nDeploy", 2, "Deploy"),
            ("preserve-fenced-env", "```\n<environment_context>literal</environment_context>\n```", 0, "```\n<environment_context>literal</environment_context>\n```"),
            ("preserve-inline-env", "Check `<environment_context>` tag", 0, "Check `<environment_context>` tag"),
            ("only-context", "<environment_context>\nonly\n</environment_context>", 1, ""),
            ("subagent-notification", "<subagent_notification>\ndone\n</subagent_notification>\nContinue", 1, "Continue"),
            ("collaboration-mode", "<collaboration_mode>plan</collaboration_mode>\nDraft plan", 1, "Draft plan"),
            ("skills-instructions", "<skills_instructions>\nskill\n</skills_instructions>\nUse skill", 1, "Use skill"),
            ("system-reminder", "<system-reminder>hook note</system-reminder>\nCheck output", 1, "Check output"),
        ]
        for (id, raw, expectedCtxCount, expectedBody) in cases {
            let proj = UserContextEngine.projectUserMessage(raw)
            XCTAssertEqual(proj.contextBlocks.count, expectedCtxCount, id)
            XCTAssertEqual(proj.text.trimmingCharacters(in: .whitespacesAndNewlines), expectedBody.trimmingCharacters(in: .whitespacesAndNewlines), id)
        }

        let explicit = UserContextEngine.projectUserMessage(
            "Explicit text",
            contextBlocks: [UserContextBlock(kind: "environment", text: "<environment_context>\n/repo\n</environment_context>")]
        )
        XCTAssertEqual(explicit.contextBlocks.count, 1)
        XCTAssertTrue(UserContextEngine.visibleContextText(explicit.contextBlocks[0].text).contains("/repo"))

        let whitespace = UserContextEngine.projectUserMessage("   \n  ")
        XCTAssertTrue(whitespace.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        XCTAssertTrue(whitespace.contextBlocks.isEmpty)
    }

    // MARK: - 3. userMessageViewModel.test.ts (7 cases)

    func testPorted_userMessageViewModel() throws {
        let plain = UserContextEngine.userMessageSegments("Simple message")
        XCTAssertEqual(plain.count, 1)

        let empty = UserContextEngine.userMessageSegments("   ")
        XCTAssertTrue(empty.isEmpty)

        let qa = "<send_user_message_question_reply>[{\"questionItemId\":\"q-port\",\"question\":\"Which port?\",\"answer\":\"8080\"}]</send_user_message_question_reply>"
        let qaSegs = UserContextEngine.userMessageSegments(qa)
        if case let .reply(_, q, a)? = qaSegs.first {
            XCTAssertEqual(q, "Which port?")
            XCTAssertEqual(a, "8080")
        } else {
            XCTFail("Expected .reply segment for <send_user_message_question_reply> envelope, got: \(qaSegs)")
        }

        let literalProtected = "```\n\(qa)\n```"
        let literalSegs = UserContextEngine.userMessageSegments(literalProtected)
        XCTAssertEqual(literalSegs, [.text(id: literalSegs[0].id, text: literalProtected)], "Fenced literal envelope must stay plain text")

        let mixed = "Intro line\n\n\(qa)\n\nOutro line"
        let mixedSegs = UserContextEngine.userMessageSegments(mixed)
        XCTAssertEqual(mixedSegs.count, 3)

        let ctxOnly = UserContextEngine.userMessageSegments("", contextBlocks: [UserContextBlock(kind: "instructions", text: "rules")])
        XCTAssertTrue(ctxOnly.isEmpty)

        let multiline = UserContextEngine.userMessageSegments("Line 1\nLine 2\nLine 3")
        XCTAssertEqual(multiline, [.text(id: multiline[0].id, text: "Line 1\nLine 2\nLine 3")])
    }

    // MARK: - 4. sessionStorage.test.ts (8 cases) — Verifies Ground-Truth 100/300 MiB Thresholds

    func testPorted_sessionStorage() throws {
        try runAsyncParityCheck {
            let mib: Int64 = 1024 * 1024
            let cases: [(String, SessionStorageSize?, String, String)] = [
                ("nil-unknown", nil, "—", "muted"),
                ("pending", SessionStorageSize(bytes: nil, scope: "file", status: "pending"), "—", "muted"),
                ("zero-bytes", SessionStorageSize(bytes: 0, scope: "file", status: "complete"), "0K", "success"),
                ("sub-kib", SessionStorageSize(bytes: 512, scope: "file", status: "complete"), "<1K", "success"),
                ("under-100mib-success", SessionStorageSize(bytes: 99 * mib, scope: "file", status: "complete"), "99M", "success"),
                ("100mib-warning", SessionStorageSize(bytes: 100 * mib, scope: "directory", status: "complete"), "100M", "warning"),
                ("300mib-danger", SessionStorageSize(bytes: 300 * mib, scope: "directory", status: "complete"), "300M", "danger"),
                ("partial-prefix", SessionStorageSize(bytes: 1536 * 1024, scope: "directory", status: "partial"), "≥1.5M", "success"),
                ("unsafe-integer", SessionStorageSize(bytes: 9_007_199_254_740_992, scope: "file", status: "complete"), "—", "muted"),
            ]
            let lightPalette = DesignPalette.resolve(scheme: .light, workspace: .codex)
            for (id, input, expectedLabel, expectedTone) in cases {
                let formatted = SessionStorageFormatter.format(input)
                XCTAssertEqual(formatted.label, expectedLabel, id)
                XCTAssertEqual(formatted.tone, expectedTone, id)
                let resolvedColor = SessionStorageBadgeView.resolvedColor(for: input, palette: lightPalette)
                let expectedColor: RGBAColor = switch expectedTone {
                case "danger": lightPalette.errFg
                case "warning": lightPalette.warnFg
                case "success": lightPalette.okFg
                default: lightPalette.textMuted
                }
                XCTAssertEqual(resolvedColor, expectedColor, id)
            }
        }
    }

    // MARK: - 5. sessionSelection.test.ts (9 cases) — Strict Active vs Archived Exclusivity

    func testPorted_sessionSelection() throws {
        let activeKeys = ["a1", "a2", "a3", "a4"]
        let archivedKeys = ["z1", "z2", "z3"]
        var part = PartitionedSessionSelectionState(activeScope: "s#active", archivedScope: "s#archived")

        // 1) Cmd+Click enters multi-select in Active and seeds activeKey
        let open1 = part.click(partition: .active, visible: activeKeys, key: "a3", click: SessionClick(detail: 1, metaKey: true), activeKey: "a1")
        XCTAssertFalse(open1)
        XCTAssertEqual(part.active.keys, ["a1", "a3"])
        XCTAssertTrue(part.archived.keys.isEmpty)

        // 2) Shift+Click extends range within Active only
        _ = part.click(partition: .active, visible: activeKeys, key: "a4", click: SessionClick(detail: 1, shiftKey: true))
        XCTAssertEqual(part.active.keys, ["a1", "a3", "a4"])

        // 3) Selecting in Archived strictly clears Active selection!
        _ = part.click(partition: .archived, visible: archivedKeys, key: "z1", click: SessionClick(detail: 1, metaKey: true))
        XCTAssertTrue(part.active.keys.isEmpty)
        XCTAssertFalse(part.active.selecting)
        XCTAssertEqual(part.archived.keys, ["z1"])
        XCTAssertEqual(part.focusPartition, .archived)

        // 4) Shift+Click in Archived NEVER crosses into Active items!
        _ = part.click(partition: .archived, visible: archivedKeys, key: "z3", click: SessionClick(detail: 1, shiftKey: true), activeKey: "a1")
        XCTAssertEqual(part.archived.keys, ["z1", "z2", "z3"])
        XCTAssertTrue(part.active.keys.isEmpty)

        // 5) Cmd+A when Archived has focus & is open selects ONLY Archived
        let target1 = part.selectAll(activeVisible: activeKeys, archivedVisible: archivedKeys, supportsArchive: true, archivedOpen: true)
        XCTAssertEqual(target1, .archived)
        XCTAssertEqual(part.archived.keys, archivedKeys)
        XCTAssertTrue(part.active.keys.isEmpty)

        // 6) Collapsing Archived drawer immediately clears Archived selection and restores Active focus!
        part.setArchivedOpen(false)
        XCTAssertTrue(part.archived.keys.isEmpty)
        XCTAssertFalse(part.archived.selecting)
        XCTAssertEqual(part.focusPartition, .active)

        // 7) Cmd+A with drawer collapsed selects ONLY Active
        let target2 = part.selectAll(activeVisible: activeKeys, archivedVisible: archivedKeys, supportsArchive: true, archivedOpen: false)
        XCTAssertEqual(target2, .active)
        XCTAssertEqual(part.active.keys, activeKeys)
        XCTAssertTrue(part.archived.keys.isEmpty)

        // 8) Ctrl+Click on macOS neither mutates multi-select nor opens detail
        let openCtrl = part.click(partition: .active, visible: activeKeys, key: "a2", click: SessionClick(detail: 1, ctrlKey: true), macPlatform: true)
        XCTAssertFalse(openCtrl)
        XCTAssertEqual(part.active.keys, activeKeys)

        // 9) Plain click clears multi-select and opens target detail
        let openPlain = part.click(partition: .active, visible: activeKeys, key: "a2", click: SessionClick(detail: 1))
        XCTAssertTrue(openPlain)
        XCTAssertTrue(part.active.keys.isEmpty)
        XCTAssertFalse(part.active.selecting)
    }

    // MARK: - 6. timelineViewModel.test.ts (8 cases)

    func testPorted_timelineViewModel() throws {
        let longPrompt = "Second user prompt with detailed architectural context exceeding eighty characters so preview slice is populated cleanly"
        let sources = [
            TimelineSourceItem(id: "u1", role: "user", kind: "message", text: "# Heading\nFirst prompt detail"),
            TimelineSourceItem(id: "a1", role: "assistant", kind: "message", text: "Reply 1"),
            TimelineSourceItem(id: "u2", role: "user", kind: "message", text: "<environment_context>\ncwd\n</environment_context>"),
            TimelineSourceItem(id: "u3", role: "user", kind: "message", text: longPrompt),
        ]
        let entries = TimelineViewModel.userTimelineEntries(provider: .codex, sources: sources)
        XCTAssertEqual(entries.map(\.id), ["u1", "u3"])
        XCTAssertEqual(entries[0].title, "# Heading First prompt detail")
        XCTAssertNotNil(entries[1].preview)

        XCTAssertTrue(TimelineViewModel.matchesTimelinePosition("u1", id: "u1"))
        XCTAssertTrue(TimelineViewModel.matchesTimelinePosition("u1", id: "turn:u1"))
        XCTAssertTrue(TimelineViewModel.matchesTimelinePosition("cmd-2", id: "grp-1", aliases: ["cmd-1", "cmd-2"]))
        XCTAssertFalse(TimelineViewModel.matchesTimelinePosition(nil, id: "u1"))

        let anchors: [(id: String, top: CGFloat)] = [("u1", 0), ("u2", 240), ("u3", 600)]
        XCTAssertEqual(TimelineViewModel.activeUserTimelineId(anchors: anchors, viewportTop: 260), "u2")
    }

    // MARK: - 7. historyCursor.test.ts (6 cases)

    func testPorted_historyCursor() throws {
        XCTAssertEqual(HistoryAndResumeHelpers.historyCursorIndex("b:42:msg-1"), 42)
        XCTAssertEqual(HistoryAndResumeHelpers.historyCursorIndex("idx:18"), 18)
        XCTAssertEqual(HistoryAndResumeHelpers.historyCursorIndex(nil), .infinity)

        let stable = HistoryAndResumeHelpers.stableHistoryCursor(
            history: HistoryMetadata(epoch: "e1", revision: "2"),
            blocks: [MessageBlock(id: "blk-10", role: "user", kind: "message", text: "hi")],
            beforeCursor: "b:10"
        )
        XCTAssertEqual(stable, "b:10:blk-10")
        XCTAssertEqual(HistoryAndResumeHelpers.threadResumeCommand("th-99"), "codex resume th-99")
        XCTAssertTrue(HistoryAndResumeHelpers.claudeResumeCommand(cwd: "/tmp/my dir", id: "cl-1").contains("'"))
    }

    // MARK: - 8. fileLinks.test.ts (5 cases)

    func testPorted_fileLinks() throws {
        let absTarget = FileLinksAndPlanExport.fileLinkTarget("/Users/example/src/main.rs#L42", cwd: "/Users/example")
        XCTAssertEqual(absTarget?.path, "/Users/example/src/main.rs")
        XCTAssertEqual(absTarget?.resolved, true)

        let relResolved = FileLinksAndPlanExport.fileLinkTarget("src/lib.rs:12:4", cwd: "/Users/example/proj")
        XCTAssertEqual(relResolved?.path, "/Users/example/proj/src/lib.rs")
        XCTAssertEqual(relResolved?.resolved, true)

        let relUnresolved = FileLinksAndPlanExport.fileLinkTarget("src/lib.rs:12", cwd: nil)
        XCTAssertEqual(relUnresolved?.resolved, false)

        XCTAssertNil(FileLinksAndPlanExport.fileLinkTarget("https://example.invalid/src/lib.rs"))

        let spans = FileLinksAndPlanExport.fileLinkSpans("Inspect [app.swift](/Users/example/app.swift#L88) for details")
        XCTAssertEqual(spans.count, 1)
    }

    // MARK: - 9. planExport.test.ts (3 cases)

    func testPorted_planExport() throws {
        let fromHeading = FileLinksAndPlanExport.planFilename(markdown: "# Native Swift Migration\nStep 1", fallbackTitle: "Fallback")
        XCTAssertTrue(fromHeading.hasSuffix(".md"))
        XCTAssertTrue(fromHeading.lowercased().contains("native"))

        let fromFallback = FileLinksAndPlanExport.planFilename(markdown: "No heading body", fallbackTitle: "Cleanup Plan / v2")
        XCTAssertFalse(fromFallback.contains("/"))
        XCTAssertTrue(fromFallback.hasSuffix(".md"))

        let emptyFallback = FileLinksAndPlanExport.planFilename(markdown: "   ", fallbackTitle: "   ")
        XCTAssertTrue(emptyFallback.hasSuffix(".md"))
        XCTAssertGreaterThan(emptyFallback.count, 3)
    }

    // MARK: - 10. executionGroups.test.ts (11 cases)

    func testPorted_executionGroups() throws {
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus("completed"), .completed)
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus("in_progress"), .running)
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus("failed"), .failed)
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus(nil), .completed)

        XCTAssertTrue(ExecutionGroupingEngine.isCommandText("bash -lc cargo test"))
        XCTAssertFalse(ExecutionGroupingEngine.isCommandText("   "))

        let fp1 = ExecutionGroupingEngine.activityFingerprint("same-output")
        let fp2 = ExecutionGroupingEngine.activityFingerprint("same-output")
        XCTAssertEqual(fp1, fp2)

        let codexBlocks = [
            MessageBlock(id: "c1", role: "tool", kind: "command_execution", status: "completed", text: "ls"),
            MessageBlock(id: "c2", role: "tool", kind: "command_execution", status: "failed", text: "pwd"),
            MessageBlock(id: "m1", role: "assistant", kind: "message", text: "Done"),
        ]
        let grouped = ExecutionGroupingEngine.groupCodexBlocks(codexBlocks)
        XCTAssertEqual(grouped.count, 2)
        if case let .group(g) = grouped[0] {
            XCTAssertEqual(g.commands.count, 2)
            XCTAssertEqual(g.failedCount, 1)
            XCTAssertTrue(g.initiallyOpen)
        } else {
            XCTFail("Expected execution group")
        }

        let instrBlocks = [
            MessageBlock(id: "i1", role: "tool", kind: "command_execution", status: "failed", text: "cat AGENTS.md"),
        ]
        if case let .group(ig)? = ExecutionGroupingEngine.groupCodexBlocks(instrBlocks).first {
            XCTAssertFalse(ig.initiallyOpen)
        }

        let grokGrouped = ExecutionGroupingEngine.groupGrokEvents([
            GrokHistoryEvent(id: "g1", kind: "tool_call", text: "bash", callId: "call-1", status: "completed", detail: "ok"),
        ])
        XCTAssertEqual(grokGrouped.count, 1)
    }

    // MARK: - 11. processGroups.test.ts (7 cases)

    func testPorted_processGroups() throws {
        let blocks: [MessageBlock] = (0 ..< 70).map { i in
            MessageBlock(
                id: "b-\(i)",
                role: i % 2 == 0 ? "user" : "assistant",
                kind: "message",
                text: "Msg \(i)"
            )
        }
        let compacted = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: false)
        XCTAssertLessThan(compacted.count, blocks.count)
        XCTAssertTrue(compacted.contains(where: { ConversationViewModel.isHistoryCollapsedBlock($0) }))

        let expanded = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: true)
        XCTAssertEqual(expanded.count, blocks.count)

        let revealed = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: false, revealedBlockId: "b-2")
        XCTAssertTrue(revealed.contains(where: { $0.id == "b-2" }))

        XCTAssertTrue(ConversationViewModel.shouldAutoFollow(scrollTop: 500, clientHeight: 400, scrollHeight: 950))
        XCTAssertFalse(ConversationViewModel.shouldAutoFollow(scrollTop: 200, clientHeight: 400, scrollHeight: 950))
        XCTAssertTrue(ConversationViewModel.isPlanBlock(MessageBlock(id: "p1", role: "assistant", kind: "plan", text: "# Plan")))
        XCTAssertFalse(ConversationViewModel.shouldRenderConversationBlock(MessageBlock(id: "ap1", role: "tool", kind: "approval_request")))
    }

    // MARK: - 12. claudeProcessGroups.test.ts (7 cases)

    func testPorted_claudeProcessGroups() throws {
        let events = [
            ClaudeHistoryEvent(id: "e1", kind: "tool_call", role: "Read", text: "Read src/a.rs", callId: "c1", status: "completed", result: "contents"),
            ClaudeHistoryEvent(id: "e2", kind: "tool_call", role: "Grep", text: "Grep foo", callId: "c2", status: "running"),
            ClaudeHistoryEvent(id: "e3", kind: "assistant_message", role: "assistant", text: "Summary answer"),
        ]
        let liveGrouped = ExecutionGroupingEngine.groupClaudeEvents(events, recordedOnly: false)
        XCTAssertEqual(liveGrouped.count, 2)

        let recordedGrouped = ExecutionGroupingEngine.groupClaudeEvents(events, recordedOnly: true)
        if case let .group(g) = recordedGrouped[0] {
            XCTAssertTrue(g.recordedOnly)
            XCTAssertFalse(g.running)
        } else {
            XCTFail("Expected recorded group")
        }

        let claudeRows = ConversationRowBuilder.buildClaudeRows(events: events, recordedOnly: true, attachmentsAsNamesOnly: true)
        XCTAssertEqual(claudeRows.count, 2)

        let thinkingRows = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "t1", kind: "thinking", text: "Deep thought", status: "completed")],
            recordedOnly: false
        )
        XCTAssertEqual(thinkingRows.count, 1)

        let emptyThinking = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "t2", kind: "thinking", text: "<oai-mem-citation>hidden</oai-mem-citation>")]
        )
        XCTAssertTrue(emptyThinking.isEmpty)

        let userRow = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "u1", kind: "user_message", text: "Hello Claude")],
            attachmentsAsNamesOnly: true
        )
        if case let .user(_, _, namesOnly)? = userRow.first?.kind {
            XCTAssertTrue(namesOnly)
        }

        let noticeRow = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "s1", kind: "compaction", detail: "Compacted 12 turns")]
        )
        XCTAssertEqual(noticeRow.count, 1)
    }

    // MARK: - 13. contractRegistry.test.ts (6 cases)

    func testPorted_contractRegistry() throws {
        let repoRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let contractData = try Data(contentsOf: repoRoot.appendingPathComponent("contracts/agenthub-contract.json"))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: contractData) as? [String: Any])
        let actions = try XCTUnwrap(json["actions"] as? [[String: Any]])
        XCTAssertGreaterThanOrEqual(actions.count, 50)

        let names = Set(actions.compactMap { $0["id"] as? String })
        XCTAssertEqual(names.count, actions.count)

        let ffiHeader = try String(contentsOf: repoRoot.appendingPathComponent("crates/agenthub-ffi/include/agenthub_ffi.h"))
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_invoke"))
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_desktop_attach"))
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_desktop_paste"))
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_parse_gfm_markdown"))
    }

    // MARK: - 14. desktopReferences.test.ts (15 cases)

    func testPorted_desktopReferences() throws {
        var draft = ReferenceDraft()
        let skill = DesktopChosenItem(handle: "h-skill", name: "lint", kind: "skill")
        let app = DesktopChosenItem(handle: "h-app", name: "Linear", kind: "app")
        let file = DesktopChosenItem(handle: "h-file", name: "spec.md", kind: "file")

        XCTAssertTrue(DesktopReferencesEngine.isInlineKind("skill"))
        XCTAssertTrue(DesktopReferencesEngine.isInlineKind("app"))
        XCTAssertTrue(DesktopReferencesEngine.isInlineKind("plugin"))
        XCTAssertFalse(DesktopReferencesEngine.isInlineKind("file"))

        let qSkill = DesktopReferencesEngine.inlineQuery(text: "Run $lin", start: 8, end: 8)
        XCTAssertEqual(qSkill?.kind, "skills")
        XCTAssertEqual(qSkill?.query, "lin")

        let qMention = DesktopReferencesEngine.inlineQuery(text: "Ask @Lin", start: 8, end: 8)
        XCTAssertEqual(qMention?.kind, "mentions")
        XCTAssertEqual(qMention?.query, "Lin")

        XCTAssertNil(DesktopReferencesEngine.inlineQuery(text: "user@example.invalid", start: 20, end: 20))
        XCTAssertNil(DesktopReferencesEngine.inlineQuery(text: "Select $lin", start: 7, end: 11))

        let ins1 = try DesktopReferencesEngine.insert(draft, item: skill, start: 0, end: 0)
        draft = ins1.draft
        XCTAssertEqual(draft.spans.count, 1)
        XCTAssertEqual(draft.references.map(\.handle), ["h-skill"])

        // Prefix edit shifts UTF-16 span offsets cleanly
        draft = DesktopReferencesEngine.edit(draft, text: "😀 " + draft.text)
        XCTAssertEqual(draft.spans.count, 1)
        XCTAssertGreaterThan(draft.spans[0].start, 0)

        // UTF-8 byte span conversion accounts for 4-byte emoji vs 2-unit UTF-16 surrogate pair
        let utf8Spans = DesktopReferencesEngine.toUTF8ReferenceSpans(draft)
        XCTAssertEqual(utf8Spans.count, 1)
        XCTAssertEqual(utf8Spans[0].start, draft.spans[0].start + 2)

        let ins2 = try DesktopReferencesEngine.insert(draft, item: app, start: (draft.text as NSString).length, end: (draft.text as NSString).length)
        draft = ins2.draft
        XCTAssertEqual(draft.references.count, 2)

        // Non-inline file attachments reject inline `insert` and attach as top chips in `references`!
        XCTAssertThrowsError(try DesktopReferencesEngine.insert(draft, item: file, start: 0, end: 0))
        draft.references.append(file)
        XCTAssertEqual(draft.references.count, 3)

        draft = DesktopReferencesEngine.remove(draft, handle: "h-skill")
        XCTAssertFalse(draft.references.contains(where: { $0.handle == "h-skill" }))

        // Corrupting inside inline token drops its span & reference while keeping non-inline file chip!
        draft = DesktopReferencesEngine.edit(draft, text: "rewritten plain text")
        XCTAssertTrue(draft.spans.isEmpty)
        XCTAssertEqual(draft.references.map(\.handle), ["h-file"])
    }

    // MARK: - 15. ClaudeProcessView.test.tsx (9 cases)

    func testPorted_claudeProcessView() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let conn = RuntimeConnectionStore(transport: transport, defaults: UserDefaults(suiteName: "com.lich13.agenthub.tests.cpv.\(UUID().uuidString)")!)
            let store = ActivityDisclosureStore()
            let coord = ConversationTableCoordinator(store: conn, disclosureStore: store)
            coord.sessionScopeKey = "claude:s1"

            // 1) Completed tool group starts collapsed by default
            let completedRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "c1", kind: "tool_call", role: "Bash", text: "cargo test", callId: "call-1", status: "completed", result: "ok"),
            ], recordedOnly: false)
            let completedRow = try XCTUnwrap(completedRows.first)
            if case let .executionGroup(g) = completedRow.kind {
                XCTAssertFalse(store.isOpen(scope: "claude:s1", stateKey: g.id, initiallyOpen: g.initiallyOpen))
            } else {
                XCTFail("Expected executionGroup")
            }

            // 2) Running live tool group starts expanded by default
            let runningRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "c2", kind: "tool_call", role: "Bash", text: "cargo build", callId: "call-2", status: "running"),
            ], recordedOnly: false)
            if case let .executionGroup(rg)? = runningRows.first?.kind {
                XCTAssertTrue(rg.running)
                XCTAssertTrue(store.isOpen(scope: "claude:s1", stateKey: rg.id, initiallyOpen: rg.initiallyOpen))
            } else {
                XCTFail("Expected running executionGroup")
            }

            // 3) Failed tool group starts expanded and marks failure
            let failedRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "c3", kind: "tool_call", role: "Bash", text: "exit 1", callId: "call-3", status: "failed", result: "error: build failed"),
            ], recordedOnly: false)
            if case let .executionGroup(fg)? = failedRows.first?.kind {
                XCTAssertGreaterThan(fg.failedCount, 0)
                XCTAssertTrue(store.isOpen(scope: "claude:s1", stateKey: fg.id, initiallyOpen: fg.initiallyOpen))
            } else {
                XCTFail("Expected failed executionGroup")
            }

            // 4) recordedOnly unfinished tool call renders "未见结果" instead of live spinner
            let recordedRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "c4", kind: "tool_call", role: "Read", text: "src/lib.rs", callId: "call-4", status: "running"),
            ], recordedOnly: true)
            let recCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ClaudeRecCell"))
            let recRow = try XCTUnwrap(recordedRows.first)
            store.setOpen(scope: "claude:s1", stateKey: recRow.id, isOpen: true)
            recCell.frame = NSRect(x: 0, y: 0, width: 768, height: 220)
            recCell.configure(item: recRow, leadingInset: 0, trailingInset: 0, contentWidth: 768, highlighted: false, coordinator: coord)
            @MainActor func allCellStrings(_ v: NSView) -> String {
                var s: [String] = []
                if let tf = v as? NSTextField { s.append(tf.stringValue) }
                if let btn = v as? NSButton { s.append(btn.title); s.append(btn.attributedTitle.string) }
                if let tv = v as? NSTextView { s.append(tv.string) }
                for sub in v.subviews { s.append(allCellStrings(sub)) }
                return s.joined(separator: " ")
            }
            XCTAssertTrue(allCellStrings(recCell).contains("未见结果"))

            // 5) Expanding group increases measured row height
            let hClosed = coord.measureRowHeight(completedRow, width: 720)
            store.setOpen(scope: "claude:s1", stateKey: completedRow.id, isOpen: true)
            let hOpen = coord.measureRowHeight(completedRow, width: 720)
            XCTAssertGreaterThan(hOpen, hClosed)

            // 6) Scope isolation between two Claude sessions
            XCTAssertFalse(store.isOpen(scope: "claude:s2", stateKey: completedRow.id, initiallyOpen: false))

            // 7) Thinking row disclosure expands cleanly
            let thinkRow = try XCTUnwrap(ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "th-1", kind: "thinking", text: "Step 1\nStep 2", status: "completed"),
            ]).first)
            let thinkClosed = coord.measureRowHeight(thinkRow, width: 720)
            store.setOpen(scope: "claude:s1", stateKey: thinkRow.id, isOpen: true)
            let thinkOpen = coord.measureRowHeight(thinkRow, width: 720)
            XCTAssertGreaterThan(thinkOpen, thinkClosed)

            // 8) Compaction event folds into ClaudeMetadataGroup ("压缩摘要") collapsed by default, and renders detail when expanded
            let compRow = try XCTUnwrap(ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "cmp-1", kind: "compaction", text: "上下文已压缩", detail: "已压缩早期上下文"),
            ]).first)
            let compCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ClaudeCompCell"))
            compCell.configure(item: compRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            XCTAssertTrue(allCellStrings(compCell).contains("压缩摘要"))
            store.setOpen(scope: coord.sessionScopeKey, stateKey: compRow.id, isOpen: true)
            store.setOpen(scope: coord.sessionScopeKey, stateKey: "cmp-1", isOpen: true)
            compCell.configure(item: compRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            XCTAssertTrue(allCellStrings(compCell).contains("已压缩早期上下文"))

            // 9) 2,000-entry FIFO bound evicts oldest disclosure key
            for idx in 0 ..< 2_005 {
                store.setOpen(scope: "claude:lru", stateKey: "item-\(idx)", isOpen: true)
            }
            XCTAssertFalse(store.isOpen(scope: "claude:lru", stateKey: "item-0", initiallyOpen: false))
            XCTAssertTrue(store.isOpen(scope: "claude:lru", stateKey: "item-2004", initiallyOpen: false))
        }
    }

    // MARK: - 16. ExecutionGroupView.test.tsx (11 cases)

    func testPorted_executionGroupView() throws {
        try runAsyncParityCheck {
            let kinds: [(String, String, String, String)] = [
                ("read", "file_read", "read_file", "读取文件"),
                ("search", "web_search", "search_query", "搜索内容"),
                ("exec", "command_execution", "bash", "运行命令"),
                ("mcp", "tool_call", "mcp.linear.list_issues", "调用工具"),
            ]
            for (id, blkKind, toolName, expectedWord) in kinds {
                let blocks = [MessageBlock(id: id, role: "tool", kind: blkKind, status: "completed", text: "arg", toolName: toolName)]
                if case let .group(g)? = ExecutionGroupingEngine.groupCodexBlocks(blocks).first {
                    XCTAssertTrue(g.summaryHeader.label.contains(expectedWord), "\(id): \(g.summaryHeader.label)")
                } else {
                    XCTFail("Expected group for \(id)")
                }
                let runningBlocks = [MessageBlock(id: "\(id)-r", role: "tool", kind: blkKind, status: "in_progress", text: "arg", toolName: toolName)]
                if case let .group(rg)? = ExecutionGroupingEngine.groupCodexBlocks(runningBlocks).first {
                    XCTAssertTrue(rg.running)
                    XCTAssertTrue(rg.summaryHeader.label.contains("正在"))
                }
            }
            let store = ActivityDisclosureStore()
            store.setOpen(scope: "codex:1", stateKey: "alias-cmd", isOpen: true)
            XCTAssertTrue(store.isOpen(scope: "codex:1", stateKey: "grp-1", aliases: ["alias-cmd"], initiallyOpen: false))

            store.clearAll()
            XCTAssertFalse(store.isOpen(scope: "codex:1", stateKey: "grp-1", aliases: ["alias-cmd"], initiallyOpen: false))

            let emptyOutHeight = ConversationTableCoordinator.measureMonospaceBoxHeight(text: "暂无输出", width: 500)
            XCTAssertGreaterThanOrEqual(emptyOutHeight, 48)
        }
    }

    // MARK: - 17. UserMessage.test.tsx (12 cases) — 20pt Bubble, "上下文", File Cards & Question-Reply

    func testPorted_userMessageView() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.um.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
            let coord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())

            let msg = UserMessageContent(
                id: "um-1",
                text: "<environment_context>\n/repo\n</environment_context>\n<send_user_message_question_reply>[{\"questionItemId\":\"q-target\",\"question\":\"Which target?\",\"answer\":\"arm64-apple-darwin\"}]</send_user_message_question_reply>",
                attachments: [
                    UserAttachment(id: "a1", name: "Spec.pdf", kind: "file", path: "/Users/example/Spec.pdf", reason: nil),
                    UserAttachment(id: "a2", name: "Broken.png", kind: "image", path: nil, reason: "文件已移除"),
                ],
                contextBlocks: nil
            )
            let row = ConversationRowItem(id: "r-um-1", timelineId: "um-1", kind: .user(message: msg, rawText: msg.text, attachmentsAsNamesOnly: false))
            let cell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("TestUserCell"))
            cell.frame = NSRect(x: 0, y: 0, width: 860, height: 320)
            cell.configure(item: row, leadingInset: 46, trailingInset: 46, contentWidth: 768, highlighted: false, coordinator: coord)
            cell.layoutSubtreeIfNeeded()

            @MainActor func collectLabels(_ v: NSView) -> [String] {
                var out: [String] = []
                if let tf = v as? NSTextField { out.append(tf.stringValue) }
                if let btn = v as? NSButton, !btn.title.isEmpty { out.append(btn.title) }
                if let btn = v as? NSButton, !btn.attributedTitle.string.isEmpty { out.append(btn.attributedTitle.string) }
                if let tv = v as? NSTextView { out.append(tv.string) }
                for sub in v.subviews { out.append(contentsOf: collectLabels(sub)) }
                return out
            }
            @MainActor func collectCornerRadii(_ v: NSView) -> [CGFloat] {
                var out: [CGFloat] = []
                if let r = v.layer?.cornerRadius, r > 0 { out.append(r) }
                for sub in v.subviews { out.append(contentsOf: collectCornerRadii(sub)) }
                return out
            }

            let allText = collectLabels(cell).joined(separator: " | ")
            let radii = collectCornerRadii(cell)

            XCTAssertFalse(allText.contains("📎"), "Must never render paperclip emoji")
            XCTAssertTrue(allText.contains("Spec.pdf"))
            XCTAssertTrue(allText.contains("图片不可用"))
            XCTAssertTrue(allText.contains("上下文"))
            XCTAssertFalse(allText.contains("▸"), "Must use Lucide ChevronRight icon, never ASCII triangle")
            XCTAssertFalse(allText.contains("展开"), "Must not append invented '展开' suffix")
            XCTAssertFalse(allText.contains("上下文 ("))
            XCTAssertTrue(radii.contains(20), "User bubble must use 20pt cornerRadius")
            XCTAssertTrue(allText.contains("arm64-apple-darwin"))

            let hCollapsed = coord.measureRowHeight(row, width: 768)
            coord.disclosureStore.setOpen(scope: coord.sessionScopeKey, stateKey: "um-1:context", isOpen: true)
            let hExpanded = coord.measureRowHeight(row, width: 768)
            XCTAssertGreaterThan(hExpanded, hCollapsed)

            let namesOnlyRow = ConversationRowItem(id: "r-um-2", timelineId: "um-2", kind: .user(message: msg, rawText: "Hi", attachmentsAsNamesOnly: true))
            let cellNames = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("TestUserNamesCell"))
            cellNames.configure(item: namesOnlyRow, leadingInset: 36, trailingInset: 40, contentWidth: 344, highlighted: false, coordinator: coord)
            let namesText = collectLabels(cellNames).joined(separator: " | ")
            XCTAssertTrue(namesText.contains("Spec.pdf"))

            let narrowH = coord.measureRowHeight(row, width: 340)
            XCTAssertGreaterThan(narrowH, 48)

            coord.attachmentViewport.injectForTesting(key: "\(coord.sessionScopeKey):um-1:img-ok", image: NSImage(size: NSSize(width: 32, height: 32)))
            XCTAssertEqual(coord.attachmentViewport.loadedCount, 1)
            coord.attachmentViewport.evictOutside(visibleKeys: [])
            XCTAssertEqual(coord.attachmentViewport.loadedCount, 0)

            // Question-Reply left 30x30pt copy button: hidden when unhovered on desktop, revealed on hover, copies ONLY answer text!
            XCTAssertEqual(cell.questionReplyCopyButtons.count, 1)
            let qrBtn = cell.questionReplyCopyButtons[0]
            XCTAssertEqual(qrBtn.frame.width, 30, accuracy: 0.5)
            XCTAssertEqual(qrBtn.frame.height, 30, accuracy: 0.5)
            cell.setSimulatedHoverForTesting(false)
            XCTAssertEqual(qrBtn.alphaValue, 0.0, accuracy: 0.01)
            cell.setSimulatedHoverForTesting(true)
            XCTAssertEqual(qrBtn.alphaValue, 1.0, accuracy: 0.01)

            let pb = NSPasteboard.general
            let saved = pb.string(forType: .string)
            defer {
                pb.clearContents()
                if let saved { pb.setString(saved, forType: .string) }
            }
            pb.clearContents()
            qrBtn.performClick(nil)
            XCTAssertEqual(pb.string(forType: .string), "arm64-apple-darwin")
        }
    }

    // MARK: - 18. v2.1.36 Thinking-Block Default Folding (F1-F4, F6-F8), Collaboration Error Guard & Light/Dark Render

    func testThinkingFoldingRootCausesAndLightDarkOffscreenRenderRegression() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.tf.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
            let disc = ActivityDisclosureStore()
            let coord = ConversationTableCoordinator(store: store, disclosureStore: disc)
            coord.sessionScopeKey = "local::codex:th-fold"

            @MainActor func collectAllText(_ v: NSView) -> String {
                var out: [String] = []
                if let tf = v as? NSTextField { out.append(tf.stringValue) }
                if let tv = v as? NSTextView { out.append(tv.string) }
                for sub in v.subviews { out.append(collectAllText(sub)) }
                return out.joined(separator: "\n")
            }

            // F8 + F1a + F1b: Codex commentary + failed rg command + final answer
            let codexBlocks: [MessageBlock] = [
                MessageBlock(
                    id: "commentary-1",
                    role: "assistant",
                    kind: "message",
                    phase: "commentary",
                    text: "先用 rg 检查旧调用点分布。\n第二行隐藏推理细节不该默认撑开。",
                    turnId: "turn-1"
                ),
                MessageBlock(
                    id: "call-rg-1",
                    role: "tool",
                    kind: "command_execution",
                    status: "failed",
                    text: "exit code 1 (no matches)",
                    input: "{\"cmd\":\"rg GlassHeaderSurface\"}",
                    toolName: "exec_command",
                    callId: "call-rg-1",
                    turnId: "turn-1"
                ),
                MessageBlock(
                    id: "final-1",
                    role: "assistant",
                    kind: "message",
                    phase: "final_answer",
                    text: "已完成全仓核验，零残留引用。",
                    turnId: "turn-1"
                ),
            ]
            let codexRows = ConversationRowBuilder.buildCodexRows(
                blocks: codexBlocks,
                showAllHistory: true,
                planFallbackTitle: "Plan",
                subagentsSupported: true
            )
            XCTAssertEqual(codexRows.count, 2, "Commentary + tool must fold into 1 UnifiedProcessGroup before final reply")
            guard case let .processGroup(proc) = codexRows[0].kind else {
                return XCTFail("Expected first row to be .processGroup")
            }
            XCTAssertEqual(proc.thoughtCount, 1)
            XCTAssertEqual(proc.toolCount, 1)
            XCTAssertEqual(proc.failedCount, 1)
            XCTAssertEqual(proc.nonInstructionFailedCount, 1)
            XCTAssertFalse(proc.initiallyOpen, "F1a: Process group containing thoughts must default to closed even if rg exited 1")
            XCTAssertEqual(coord.measureRowHeight(codexRows[0], width: 720), 34, accuracy: 0.5)

            // Pure AGENTS.md instruction failure in a tool-only UnifiedProcessGroup also stays closed
            let agentsCmd = ExecutionCommand(
                id: "cmd-agents",
                sourceIds: ["cmd-agents"],
                title: "cat AGENTS.md",
                status: .failed,
                instructionFile: true
            )
            let agentsGroup = ExecutionGroup(
                id: "group:agents",
                mergedIds: [],
                commands: [agentsCmd],
                kind: "command",
                provider: "Codex",
                running: false,
                recordedOnly: false,
                failedCount: 1
            )
            let instrOnlyProc = ProcessGroupingEngine.buildUnifiedProcessGroup(
                prefix: "codex",
                steps: [.execution(group: agentsGroup, turnId: nil)]
            )
            XCTAssertEqual(instrOnlyProc.nonInstructionFailedCount, 0)
            XCTAssertFalse(instrOnlyProc.initiallyOpen)

            // F1b: Opening outer group keeps inner thinking in 32pt compact second-level disclosure while failed tool expands
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: proc.stateKey, aliases: proc.stateAliases, isOpen: true)
            let openOuterCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("OpenOuterProcCell"))
            openOuterCell.configure(item: codexRows[0], leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            let openOuterText = collectAllText(openOuterCell)
            XCTAssertTrue(openOuterText.contains("思考 先用 rg 检查旧调用点分布。"), "F1b: Inner thinking row shows single-line snippet")
            XCTAssertFalse(openOuterText.contains("第二行隐藏推理细节不该默认撑开。"), "F1b: Full thought prose must remain unmounted until inner row opened")
            XCTAssertTrue(openOuterText.contains("exit code 1"), "Failed non-instruction tool inside opened group still expands")

            // Single-segment pure thinking group opens directly without second-level click
            let singleThinkRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "pure-th-1", kind: "thinking", text: "第一行纯思考\n第二行直读详情", status: "completed"),
            ])
            XCTAssertEqual(singleThinkRows.count, 1)
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: singleThinkRows[0].id, aliases: singleThinkRows[0].timelineAliases, isOpen: true)
            let pureCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("PureThinkCell"))
            pureCell.configure(item: singleThinkRows[0], leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            XCTAssertTrue(collectAllText(pureCell).contains("第二行直读详情"))

            // F2: Standalone .thinking(..., running: true) defaults to 34pt closed
            let runningThinkRow = ConversationRowItem(
                id: "standalone-running-think",
                kind: .thinking(title: "已思考", text: "正在流式生成的长思考段落\n第二行\n第三行", running: true)
            )
            XCTAssertEqual(coord.measureRowHeight(runningThinkRow, width: 720), 34, accuracy: 0.5)
            let runningThinkCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("RunningThinkCell"))
            runningThinkCell.configure(item: runningThinkRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            XCTAssertFalse(collectAllText(runningThinkCell).contains("第二行"))

            // F6 + F8 (Claude): Turnless thinking + intermediate prose + tool fold into UnifiedProcessGroup; turned intermediate joins ClaudeProcessGroup
            let turnlessClaudeRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "cl-th", kind: "thinking", text: "无 turnId 思考", status: "running"),
                ClaudeHistoryEvent(id: "cl-mid", kind: "assistant_message", role: "assistant", text: "无 turnId 过渡说明", assistantPhase: "intermediate"),
                ClaudeHistoryEvent(id: "cl-tc", kind: "tool_call", role: "Bash", text: "ls", callId: "c1"),
                ClaudeHistoryEvent(id: "cl-tr", kind: "tool_result", role: "Bash", callId: "c1", status: "completed", detail: "Cargo.toml"),
                ClaudeHistoryEvent(id: "cl-fin", kind: "assistant_message", role: "assistant", text: "最终结论", assistantPhase: "final"),
            ])
            XCTAssertEqual(turnlessClaudeRows.count, 2, "F6/F8: Turnless Claude process items must coalesce before final reply")
            XCTAssertEqual(coord.measureRowHeight(turnlessClaudeRows[0], width: 720), 34, accuracy: 0.5)

            let turnedClaudeRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "t-th", kind: "thinking", text: "回合思考", turnId: "t1", status: "completed"),
                ClaudeHistoryEvent(id: "t-mid", kind: "assistant_message", role: "assistant", text: "回合中间说明\n隐藏次行", turnId: "t1", assistantPhase: "intermediate"),
                ClaudeHistoryEvent(id: "t-fin", kind: "assistant_message", role: "assistant", text: "回合最终答复", turnId: "t1", assistantPhase: "final"),
            ])
            XCTAssertEqual(turnedClaudeRows.count, 2)
            guard case let .claudeProcessGroup(cProc, _) = turnedClaudeRows[0].kind else {
                return XCTFail("Expected .claudeProcessGroup for turned Claude events")
            }
            XCTAssertEqual(cProc.segmentCount, 2)

            // F3: Monotonic seq + alias sync survives reverse pagination changing first step anchorId AND group merges
            let paginationDisc = ActivityDisclosureStore()
            let tailProc = ProcessGroupingEngine.buildUnifiedProcessGroup(
                prefix: "codex",
                steps: [.detail(id: "step-2", sourceIds: ["step-2"], title: "rg", detail: "err", running: false, failed: true, turnId: "t1")]
            )
            XCTAssertTrue(tailProc.initiallyOpen)
            // User manually closes tailProc
            paginationDisc.setOpen(scope: "s-pag", stateKey: tailProc.stateKey, aliases: tailProc.stateAliases, isOpen: false)
            // Reverse pagination prepends step-1, shifting stateKey from codex:process:step-2 to codex:process:step-1
            let prependedProc = ProcessGroupingEngine.buildUnifiedProcessGroup(
                prefix: "codex",
                steps: [
                    .detail(id: "step-1", sourceIds: ["step-1"], title: "ls", detail: "ok", running: false, failed: false, turnId: "t1"),
                    .detail(id: "step-2", sourceIds: ["step-2"], title: "rg", detail: "err", running: false, failed: true, turnId: "t1"),
                ]
            )
            XCTAssertEqual(prependedProc.stateKey, "codex:process:step-1")
            XCTAssertFalse(
                paginationDisc.isOpen(scope: "s-pag", stateKey: prependedProc.stateKey, aliases: prependedProc.stateAliases, initiallyOpen: prependedProc.initiallyOpen),
                "F3: Manual close must survive reverse pagination prepending an older anchor step"
            )
            // Separate open group (seq 2) + separate closed group (seq 3) merge -> stays open, then closing merged group closes all
            paginationDisc.setOpen(scope: "s-merge", stateKey: "gA", isOpen: true)
            paginationDisc.setOpen(scope: "s-merge", stateKey: "gB", isOpen: false)
            XCTAssertTrue(paginationDisc.isOpen(scope: "s-merge", stateKey: "g-merged", aliases: ["gA", "gB"], initiallyOpen: false))
            paginationDisc.setOpen(scope: "s-merge", stateKey: "g-merged", aliases: ["gA", "gB"], isOpen: false)
            XCTAssertFalse(paginationDisc.isOpen(scope: "s-merge", stateKey: "g-merged", aliases: ["gA", "gB"], initiallyOpen: true))

            // F4 + F7: Real NSTableView host verifying cross-session cell/height cache isolation & strict scrollToPositionKey
            let scrollView = NSScrollView(frame: NSRect(x: 0, y: 0, width: 860, height: 520))
            let tableView = NSTableView(frame: NSRect(x: 0, y: 0, width: 860, height: 520))
            let col = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("MainCol"))
            col.width = 860
            tableView.addTableColumn(col)
            tableView.headerView = nil
            tableView.dataSource = coord
            tableView.delegate = coord
            scrollView.documentView = tableView
            coord.scrollView = scrollView
            coord.tableView = tableView
            coord.column = col

            let sharedCollisionRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "", kind: "thinking", text: "跨会话同 ID 思考块\n第二行长文本\n第三行长文本", status: "completed"),
            ])
            XCTAssertEqual(sharedCollisionRows.count, 1)
            coord.sessionScopeKey = "local::claude:session-A"
            disc.setOpen(scope: "local::claude:session-A", stateKey: sharedCollisionRows[0].id, aliases: sharedCollisionRows[0].timelineAliases, isOpen: true)
            coord.updateRowsPreservingAnchor(sharedCollisionRows, forceScrollToBottom: false)
            let heightInA = coord.tableView(tableView, heightOfRow: 0)
            XCTAssertGreaterThan(heightInA, 60)

            // Switch to session-B WITHOUT forceScrollToBottom: height & cached cell must NOT leak session-A's open state!
            coord.sessionScopeKey = "local::claude:session-B"
            coord.updateRowsPreservingAnchor(sharedCollisionRows, forceScrollToBottom: false)
            let heightInB = coord.tableView(tableView, heightOfRow: 0)
            XCTAssertEqual(heightInB, 34, accuracy: 0.5, "F4: Row height cache must isolate by sessionScopeKey")
            if let cellInB = tableView.view(atColumn: 0, row: 0, makeIfNecessary: true) {
                XCTAssertFalse(collectAllText(cellInB).contains("第二行长文本"), "F4: Cell cache must re-configure on sessionScopeKey change")
            }

            // F7: Short/structural positionKeys never false-match process groups; hitting inner commentary opens BOTH outer & inner
            let searchDisc = ActivityDisclosureStore()
            coord.disclosureStore = searchDisc
            coord.sessionScopeKey = "local::codex:search-test"
            coord.updateRowsPreservingAnchor(codexRows, forceScrollToBottom: true)
            XCTAssertFalse(coord.scrollToPositionKey("0", reduceMotion: true))
            XCTAssertFalse(coord.scrollToPositionKey("process", reduceMotion: true))
            XCTAssertFalse(coord.scrollToPositionKey("event", reduceMotion: true))
            XCTAssertEqual(coord.tableView(tableView, heightOfRow: 0), 34, accuracy: 0.5, "F7: Short positionKey must not expand processGroup")

            XCTAssertTrue(coord.scrollToPositionKey("commentary-1", reduceMotion: true))
            if let expandedCell = tableView.view(atColumn: 0, row: 0, makeIfNecessary: true) {
                XCTAssertTrue(
                    collectAllText(expandedCell).contains("第二行隐藏推理细节不该默认撑开。"),
                    "F7/F1b: Jumping to inner commentary must open both outer processGroup and target inner thinking disclosure"
                )
            }

            // P3: CodexDesktopStore.act records actionError on pre-validation throw
            let desktopStore = CodexDesktopStore(connection: store, threadId: "th-err")
            do {
                try await desktopStore.act(.send(messageId: "m-1", text: "hello", references: [], referenceSpans: []))
                XCTFail("Expected disconnected act() to throw")
            } catch {
                XCTAssertEqual(desktopStore.actionError, "尚未连接本机 Codex 会话")
            }

            // Light & Dark offscreen rasterization + non-blank foreground glyph/icon ink assertions
            guard ProcessInfo.processInfo.environment["AGENTHUB_RENDER_TESTS"] == "1" else { return }
            let renderDisc = ActivityDisclosureStore()
            coord.disclosureStore = renderDisc
            coord.sessionScopeKey = "local::codex:render-check"
            // Open outer process group on row 0 so the 32pt compact inner thinking disclosure + failed rg box + final reply all render
            renderDisc.setOpen(scope: coord.sessionScopeKey, stateKey: proc.stateKey, aliases: proc.stateAliases, isOpen: true)
            for (scheme, appearanceName) in [(ColorSchemeKind.light, NSAppearance.Name.aqua), (.dark, .darkAqua)] {
                coord.scheme = scheme
                coord.workspace = .codex
                let pal = DesignPalette.resolve(scheme: scheme, workspace: .codex)
                let win = NSWindow(
                    contentRect: NSRect(x: 0, y: 0, width: 860, height: 520),
                    styleMask: [.borderless],
                    backing: .buffered,
                    defer: false
                )
                win.isOpaque = true
                win.backgroundColor = pal.background.nsColor
                win.appearance = NSAppearance(named: appearanceName)
                win.contentView = scrollView
                coord.updateRowsPreservingAnchor(codexRows, forceScrollToBottom: false)
                scrollView.layoutSubtreeIfNeeded()
                win.displayIfNeeded()

                let pixels = try OffscreenRenderHelper.renderNSView(
                    scrollView,
                    size: NSSize(width: 860, height: 520),
                    appearance: appearanceName
                )
                var inkCount = 0
                let bg = pal.background
                let totalPx = 860 * 520
                for p in 0 ..< totalPx {
                    let off = p * 4
                    if abs(Int(pixels[off]) - Int(bg.r)) > 14
                        || abs(Int(pixels[off + 1]) - Int(bg.g)) > 14
                        || abs(Int(pixels[off + 2]) - Int(bg.b)) > 14
                    {
                        inkCount += 1
                    }
                }
                XCTAssertGreaterThan(inkCount, 600, "Reader process group + 2nd-level thinking disclosure must render non-blank ink in \(scheme)")
                win.contentView = nil
                win.orderOut(nil)
            }
        }
    }

    func testReviewerProcessDisclosureEdgeCasesAndReversePaginationHeightParity() throws {
        try runAsyncParityCheck {
            let transport = MockRuntimeTransport { _, _ in Data("{}".utf8) }
            let defaults = UserDefaults(suiteName: "com.lich13.agenthub.tests.rev.\(UUID().uuidString)")!
            let store = RuntimeConnectionStore(transport: transport, defaults: defaults)
            let disc = ActivityDisclosureStore()
            let coord = ConversationTableCoordinator(store: store, disclosureStore: disc)
            coord.sessionScopeKey = "local::codex:rev-audit"

            @MainActor func collectAllText(_ v: NSView) -> String {
                var out: [String] = []
                if let tf = v as? NSTextField { out.append(tf.stringValue) }
                if let tv = v as? NSTextView { out.append(tv.string) }
                for sub in v.subviews { out.append(collectAllText(sub)) }
                return out.joined(separator: "\n")
            }

            // 1. Outer UnifiedProcessGroup stateAliases never collide with inner .detail rows:
            //    (a) Opening outer group keeps completed .detail rows closed;
            //    (b) Closing an inner failed .detail row inside an initiallyOpen group keeps outer group open.
            let detailMixedProc = ProcessGroupingEngine.buildUnifiedProcessGroup(
                prefix: "claude",
                steps: [
                    .thinking(id: "th-det-1", sourceIds: ["th-det-1"], text: "思考步骤\n隐藏第二行", running: false, turnId: nil),
                    .detail(id: "det-ok", sourceIds: ["det-ok"], title: "local_command", detail: "completed detail output", running: false, failed: false, turnId: nil),
                ]
            )
            let detailMixedRow = ConversationRowItem(
                id: detailMixedProc.id,
                timelineId: detailMixedProc.id,
                timelineAliases: detailMixedProc.timelineAliases,
                kind: .processGroup(detailMixedProc)
            )
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: detailMixedProc.stateKey, aliases: detailMixedProc.stateAliases, isOpen: true)
            let mixedCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("MixedDetailCell"))
            mixedCell.configure(item: detailMixedRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            XCTAssertFalse(
                collectAllText(mixedCell).contains("completed detail output"),
                "Opening outer UnifiedProcessGroup must not forcibly expand completed inner .detail rows"
            )

            let failedDetailProc = ProcessGroupingEngine.buildUnifiedProcessGroup(
                prefix: "claude",
                steps: [
                    .detail(id: "det-1", sourceIds: ["det-1"], title: "check", detail: "ok out", running: false, failed: false, turnId: nil),
                    .detail(id: "det-fail", sourceIds: ["det-fail"], title: "hook", detail: "fatal hook detail", running: false, failed: true, turnId: nil),
                ]
            )
            XCTAssertTrue(failedDetailProc.initiallyOpen)
            let failedDetailRow = ConversationRowItem(
                id: failedDetailProc.id,
                timelineId: failedDetailProc.id,
                timelineAliases: failedDetailProc.timelineAliases,
                kind: .processGroup(failedDetailProc)
            )
            // User collapses only the inner failed .detail row while outer group was initiallyOpen
            coord.toggleDisclosurePreservingAnchor(stateKey: "det-fail", aliases: ["det-fail"], currentlyOpen: true)
            XCTAssertTrue(
                disc.isOpen(
                    scope: coord.sessionScopeKey,
                    stateKey: failedDetailProc.stateKey,
                    aliases: failedDetailProc.stateAliases,
                    initiallyOpen: failedDetailProc.initiallyOpen
                ),
                "Collapsing inner .detail row must not collapse outer UnifiedProcessGroup"
            )
            let afterInnerCloseCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("AfterInnerCloseCell"))
            afterInnerCloseCell.configure(item: failedDetailRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            let afterCloseText = collectAllText(afterInnerCloseCell)
            XCTAssertTrue(afterCloseText.contains("hook"), "Outer group summary + inner headers remain visible")
            XCTAssertFalse(afterCloseText.contains("fatal hook detail"), "Inner failed .detail body is collapsed")

            // 2. ClaudeProcessGroup:
            //    (a) Single thinking + file-history-snapshot (0 tools, segmentCount == 2) opens thought directly;
            //    (b) permission_request / failure inside tool-bearing ClaudeProcessGroup renders directly without brain/thinking label.
            let snapAndSingleThought = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "snap-1", kind: "file-history-snapshot", turnId: "t-snap"),
                ClaudeHistoryEvent(id: "th-only", kind: "thinking", text: "首行思考\n次行应直读展示", turnId: "t-snap", status: "completed"),
                ClaudeHistoryEvent(id: "ans-1", kind: "assistant_message", role: "assistant", text: "结论", turnId: "t-snap", assistantPhase: "final"),
            ])
            guard case let .claudeProcessGroup(snapProc, _) = snapAndSingleThought[0].kind else {
                return XCTFail("Expected .claudeProcessGroup")
            }
            XCTAssertEqual(snapProc.segmentCount, 2)
            XCTAssertFalse(snapProc.wrapsThinkingDisclosures, "Single thought + metadata snapshot with 0 tools must open directly")
            disc.setOpen(
                scope: coord.sessionScopeKey,
                stateKey: snapProc.stateKey,
                aliases: snapProc.sourceIds.map { "claude-process:\(snapProc.turnId):\($0)" },
                isOpen: true
            )
            let snapCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("SnapThoughtCell"))
            snapCell.configure(item: snapAndSingleThought[0], leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            XCTAssertTrue(collectAllText(snapCell).contains("次行应直读展示"))

            let permClaudeRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "p-th", kind: "thinking", text: "思考一\n隐藏思考正文", turnId: "t-perm", status: "completed"),
                ClaudeHistoryEvent(id: "p-tc", kind: "tool_call", role: "Bash", text: "rm /tmp/x", turnId: "t-perm", callId: "pc1", status: "running"),
                ClaudeHistoryEvent(id: "p-req", kind: "permission_request", role: "permission", text: "请求授权执行命令\n请确认路径安全", turnId: "t-perm", status: "running"),
            ])
            guard case let .claudeProcessGroup(permProc, _) = permClaudeRows[0].kind else {
                return XCTFail("Expected .claudeProcessGroup for permission turn")
            }
            XCTAssertTrue(permProc.wrapsThinkingDisclosures)
            disc.setOpen(
                scope: coord.sessionScopeKey,
                stateKey: permProc.stateKey,
                aliases: permProc.sourceIds.map { "claude-process:\(permProc.turnId):\($0)" },
                isOpen: true
            )
            let permCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("PermClaudeCell"))
            permCell.configure(item: permClaudeRows[0], leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            let permText = collectAllText(permCell)
            XCTAssertTrue(permText.contains("请确认路径安全"), "Permission request prose must be immediately visible when outer group opens")
            XCTAssertFalse(permText.contains("思考 请求授权执行命令"), "Permission request must never be mislabeled as a thinking row")
            XCTAssertFalse(permText.contains("隐藏思考正文"), "Actual thinking segment stays collapsed in 2nd-level disclosure")

            // 3. Grok + Codex reverse pagination:
            //    (a) Grok positional index shift does NOT leak open thinking state to newly prepended event:0;
            //    (b) Codex reverse pagination shifting outer anchor AND :occ collision index preserves inner :thinking open state
            //        and measureRowHeight >= laid-out fittingSize.height.
            coord.sessionScopeKey = "local::grok:pag"
            let grokTailRows = ConversationRowBuilder.buildGrokRows(
                events: [
                    GrokHistoryEvent(id: "grok:500", kind: "thinking", text: "尾部过渡说明\n尾部第二行详情"),
                    GrokHistoryEvent(id: "grok:510", kind: "tool_call", text: "Read file", callId: "gc-2", status: "completed", detail: "ok"),
                ],
                fallbackTitle: "Grok"
            )
            guard case let .processGroup(grokTailProc) = grokTailRows[0].kind,
                  case let .thinking(gTailThinkId, gTailSids, _, _, _) = grokTailProc.steps[0]
            else {
                return XCTFail("Expected Grok UnifiedProcessGroup")
            }
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: grokTailProc.stateKey, aliases: grokTailProc.stateAliases, isOpen: true)
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: "\(gTailThinkId):thinking", aliases: gTailSids.map { "\($0):thinking" }, isOpen: true)

            let grokPrependedRows = ConversationRowBuilder.buildGrokRows(
                events: [
                    GrokHistoryEvent(id: "grok:100", kind: "thinking", text: "新前插早期说明\n绝不能被 event:0 误撑开"),
                    GrokHistoryEvent(id: "grok:500", kind: "thinking", text: "尾部过渡说明\n尾部第二行详情"),
                    GrokHistoryEvent(id: "grok:510", kind: "tool_call", text: "Read file", callId: "gc-2", status: "completed", detail: "ok"),
                ],
                fallbackTitle: "Grok"
            )
            let grokAfterCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("GrokAfterPrependCell"))
            grokAfterCell.configure(item: grokPrependedRows[0], leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            let grokAfterText = collectAllText(grokAfterCell)
            XCTAssertTrue(grokAfterText.contains("尾部第二行详情"), "Previously opened Grok inner thinking remains open after prepend")
            XCTAssertFalse(grokAfterText.contains("绝不能被 event:0 误撑开"), "Newly prepended Grok thinking at index 0 stays closed")

            coord.sessionScopeKey = "local::codex:pag-height"
            let codexTailBlocks: [MessageBlock] = [
                MessageBlock(id: "cm-keep", role: "assistant", kind: "message", phase: "commentary", text: "已手动点开的中间推理\n展开多行正文一\n展开多行正文二", turnId: "t1"),
                MessageBlock(id: "call-2", role: "tool", kind: "command_execution", status: "completed", text: "done", input: "{\"cmd\":\"ls\"}", toolName: "exec_command", callId: "call-2", turnId: "t1"),
            ]
            let tailCodexRows = ConversationRowBuilder.buildCodexRows(blocks: codexTailBlocks, showAllHistory: true, planFallbackTitle: "P", subagentsSupported: true)
            guard case let .processGroup(tailCodexProc) = tailCodexRows[0].kind,
                  case let .thinking(cThinkId, cThinkSids, _, _, _) = tailCodexProc.steps[0]
            else {
                return XCTFail("Expected Codex processGroup")
            }
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: tailCodexProc.stateKey, aliases: tailCodexProc.stateAliases, isOpen: true)
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: "\(cThinkId):thinking", aliases: cThinkSids.map { "\($0):thinking" }, isOpen: true)

            let codexPrependedBlocks: [MessageBlock] = [
                MessageBlock(id: "cm-older", role: "assistant", kind: "message", phase: "commentary", text: "前插旧推理\n保持闭合次行", turnId: "t1"),
                MessageBlock(id: "cm-keep", role: "assistant", kind: "message", phase: "commentary", text: "已手动点开的中间推理\n展开多行正文一\n展开多行正文二", turnId: "t1"),
                MessageBlock(id: "call-2", role: "tool", kind: "command_execution", status: "completed", text: "done", input: "{\"cmd\":\"ls\"}", toolName: "exec_command", callId: "call-2", turnId: "t1"),
            ]
            let prepCodexRows = ConversationRowBuilder.buildCodexRows(blocks: codexPrependedBlocks, showAllHistory: true, planFallbackTitle: "P", subagentsSupported: true)
            let measuredH = coord.measureRowHeight(prepCodexRows[0], width: 720)
            let heightCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("CodexHeightParityCell"))
            heightCell.frame = NSRect(x: 0, y: 0, width: 760, height: measuredH)
            heightCell.configure(item: prepCodexRows[0], leadingInset: 20, trailingInset: 20, contentWidth: 720, highlighted: false, coordinator: coord)
            heightCell.layoutSubtreeIfNeeded()
            let heightCellText = collectAllText(heightCell)
            XCTAssertTrue(heightCellText.contains("展开多行正文二"))
            XCTAssertFalse(heightCellText.contains("保持闭合次行"))
            let heightStack = try XCTUnwrap(heightCell.subviews.first)
            XCTAssertGreaterThanOrEqual(
                measuredH,
                heightStack.fittingSize.height + 2,
                "measureRowHeight must never clip expanded inner thinking prose"
            )
            XCTAssertLessThanOrEqual(
                measuredH,
                heightStack.fittingSize.height + 14,
                "Mixed processGroup measured height must stay tightly bounded above laid-out stack height"
            )

            // 3c. Multi-item inner thinking disclosures (UnifiedProcessGroup & ClaudeProcessGroup) have 0.0pt per-row header drift
            //     with tight lower and upper bounds in both all-collapsed and partially-expanded states.
            let multiSteps: [ProcessStep] = (0 ..< 5).map { idx in
                .thinking(
                    id: "m-th-\(idx)",
                    sourceIds: ["m-th-\(idx)"],
                    text: "第 \(idx) 段推理摘要\n隐藏详情行 A-\(idx)\n隐藏详情行 B-\(idx) 含 `inline_code_\(idx)`",
                    running: idx == 4,
                    turnId: "t-multi"
                )
            }
            let multiThinkProc = ProcessGroupingEngine.buildUnifiedProcessGroup(prefix: "codex", steps: multiSteps)
            let multiThinkRow = ConversationRowItem(
                id: multiThinkProc.id,
                timelineId: multiThinkProc.id,
                timelineAliases: multiThinkProc.timelineAliases,
                kind: .processGroup(multiThinkProc)
            )
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: multiThinkProc.stateKey, aliases: multiThinkProc.stateAliases, isOpen: true)
            let multiCollapsedH = coord.measureRowHeight(multiThinkRow, width: 720)
            let multiCollapsedCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("MultiCollapsedThinkCell"))
            multiCollapsedCell.frame = NSRect(x: 0, y: 0, width: 760, height: multiCollapsedH)
            multiCollapsedCell.configure(item: multiThinkRow, leadingInset: 20, trailingInset: 20, contentWidth: 720, highlighted: false, coordinator: coord)
            multiCollapsedCell.layoutSubtreeIfNeeded()
            let multiCollapsedStack = try XCTUnwrap(multiCollapsedCell.subviews.first)
            XCTAssertEqual(
                multiCollapsedH,
                multiCollapsedStack.fittingSize.height + 2,
                accuracy: 1.0,
                "5 collapsed 32pt inner thinking disclosures must match measureRowHeight within 1pt (zero 2pt/row drift)"
            )

            for openIdx in [0, 2, 4] {
                disc.setOpen(
                    scope: coord.sessionScopeKey,
                    stateKey: "m-th-\(openIdx):thinking",
                    aliases: ["m-th-\(openIdx):thinking"],
                    isOpen: true
                )
            }
            let multiExpandedH = coord.measureRowHeight(multiThinkRow, width: 720)
            let multiExpandedCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("MultiExpandedThinkCell"))
            multiExpandedCell.frame = NSRect(x: 0, y: 0, width: 760, height: multiExpandedH)
            multiExpandedCell.configure(item: multiThinkRow, leadingInset: 20, trailingInset: 20, contentWidth: 720, highlighted: false, coordinator: coord)
            multiExpandedCell.layoutSubtreeIfNeeded()
            let multiExpandedStack = try XCTUnwrap(multiExpandedCell.subviews.first)
            XCTAssertGreaterThanOrEqual(multiExpandedH, multiExpandedStack.fittingSize.height + 2)
            XCTAssertLessThanOrEqual(
                multiExpandedH,
                multiExpandedStack.fittingSize.height + 16,
                "3 expanded + 2 collapsed inner thinking items must stay within 4pt/prose-view cushion"
            )

            let claudeMultiRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "cm-0", kind: "thinking", text: "Claude 思考零\n次行零", turnId: "t-cm", status: "completed"),
                ClaudeHistoryEvent(id: "cm-1", kind: "assistant_message", role: "assistant", text: "Claude 过渡一\n次行一", turnId: "t-cm", assistantPhase: "intermediate"),
                ClaudeHistoryEvent(id: "cm-2", kind: "thinking", text: "Claude 思考二\n次行二\n三行二", turnId: "t-cm", status: "completed"),
                ClaudeHistoryEvent(id: "cm-3", kind: "thinking", text: "Claude 思考三\n次行三", turnId: "t-cm", status: "running"),
                ClaudeHistoryEvent(id: "cm-fin", kind: "assistant_message", role: "assistant", text: "结论", turnId: "t-cm", assistantPhase: "final"),
            ])
            guard case let .claudeProcessGroup(cmProc, _) = claudeMultiRows[0].kind else {
                return XCTFail("Expected multi-segment .claudeProcessGroup")
            }
            disc.setOpen(
                scope: coord.sessionScopeKey,
                stateKey: cmProc.stateKey,
                aliases: cmProc.sourceIds.map { "claude-process:\(cmProc.turnId):\($0)" },
                isOpen: true
            )
            let cmCollapsedH = coord.measureRowHeight(claudeMultiRows[0], width: 720)
            let cmCollapsedCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ClaudeMultiCollapsedCell"))
            cmCollapsedCell.frame = NSRect(x: 0, y: 0, width: 760, height: cmCollapsedH)
            cmCollapsedCell.configure(item: claudeMultiRows[0], leadingInset: 20, trailingInset: 20, contentWidth: 720, highlighted: false, coordinator: coord)
            cmCollapsedCell.layoutSubtreeIfNeeded()
            let cmCollapsedStack = try XCTUnwrap(cmCollapsedCell.subviews.first)
            XCTAssertEqual(cmCollapsedH, cmCollapsedStack.fittingSize.height + 2, accuracy: 1.0)

            disc.setOpen(scope: coord.sessionScopeKey, stateKey: "claude:cm-0:0:thinking", aliases: ["cm-0:thinking"], isOpen: true)
            disc.setOpen(scope: coord.sessionScopeKey, stateKey: "claude:cm-2:0:thinking", aliases: ["cm-2:thinking"], isOpen: true)
            let cmExpandedH = coord.measureRowHeight(claudeMultiRows[0], width: 720)
            let cmExpandedCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ClaudeMultiExpandedCell"))
            cmExpandedCell.frame = NSRect(x: 0, y: 0, width: 760, height: cmExpandedH)
            cmExpandedCell.configure(item: claudeMultiRows[0], leadingInset: 20, trailingInset: 20, contentWidth: 720, highlighted: false, coordinator: coord)
            cmExpandedCell.layoutSubtreeIfNeeded()
            let cmExpandedStack = try XCTUnwrap(cmExpandedCell.subviews.first)
            XCTAssertGreaterThanOrEqual(cmExpandedH, cmExpandedStack.fittingSize.height + 2)
            XCTAssertLessThanOrEqual(cmExpandedH, cmExpandedStack.fittingSize.height + 12)

            // 3d. Unified .executionGroup timelineAliases across Codex, Claude, Grok, and Antigravity/DSH + scrollViewFrameDidChange alias anchor lock
            let codexExecRows = ConversationRowBuilder.buildCodexRows(
                blocks: [
                    MessageBlock(id: "cx-cmd-1", role: "tool", kind: "command_execution", status: "completed", text: "ok", input: "{\"cmd\":\"pwd\"}", toolName: "exec_command", callId: "cx-call-1", turnId: "t-exec"),
                ],
                showAllHistory: true,
                planFallbackTitle: "P",
                subagentsSupported: true
            )
            XCTAssertEqual(codexExecRows.count, 1)
            XCTAssertTrue(codexExecRows[0].timelineAliases.contains("group:cx-call-1"))
            XCTAssertTrue(codexExecRows[0].timelineAliases.contains("cx-call-1"))
            XCTAssertEqual(codexExecRows[0].timelineAliases.count, Set(codexExecRows[0].timelineAliases).count)

            let claudeExecRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "cl-cmd-0", kind: "tool_call", role: "Bash", text: "ls", callId: "cl-call-0", status: "completed", detail: "a"),
                ClaudeHistoryEvent(id: "cl-empty-th", kind: "thinking", text: "   ", status: "completed"),
                ClaudeHistoryEvent(id: "cl-cmd-1", kind: "tool_call", role: "Bash", text: "pwd", callId: "cl-call-1", status: "completed", detail: "/tmp"),
            ])
            XCTAssertEqual(claudeExecRows.count, 1)
            XCTAssertTrue(claudeExecRows[0].timelineAliases.contains("group:claude:cl-cmd-1:0"), "Merged Claude ExecutionGroup must retain merged group ID")
            XCTAssertTrue(claudeExecRows[0].timelineAliases.contains("group:cl-cmd-1"))
            XCTAssertTrue(claudeExecRows[0].timelineAliases.contains("cl-cmd-1"))
            XCTAssertEqual(claudeExecRows[0].timelineAliases.count, Set(claudeExecRows[0].timelineAliases).count)

            let grokTailExec = ConversationRowBuilder.buildGrokRows(
                events: [
                    GrokHistoryEvent(id: "grok:exec:500", kind: "tool_call", text: "Read Cargo.toml", callId: "gk-call-2", status: "completed", detail: "[workspace]"),
                ],
                fallbackTitle: "Grok"
            )
            let grokPrependExec = ConversationRowBuilder.buildGrokRows(
                events: [
                    GrokHistoryEvent(id: "grok:exec:100", kind: "tool_call", text: "Read README.md", callId: "gk-call-1", status: "completed", detail: "# AgentHub"),
                    GrokHistoryEvent(id: "grok:exec:500", kind: "tool_call", text: "Read Cargo.toml", callId: "gk-call-2", status: "completed", detail: "[workspace]"),
                ],
                fallbackTitle: "Grok"
            )
            XCTAssertTrue(grokPrependExec[0].timelineAliases.contains("group:grok:exec:500"))
            XCTAssertTrue(grokPrependExec[0].timelineAliases.contains("group:gk-call-2"))
            XCTAssertEqual(grokTailExec[0].timelineAliases.count, Set(grokTailExec[0].timelineAliases).count)

            let agyExecRows = ConversationRowBuilder.buildProviderRows(
                events: [
                    ProviderHistoryEvent(id: "agy:step:9", kind: "tool_call", role: "tool", text: "run_command", callId: "agy-call-9", status: "completed", detail: "exit 0"),
                ],
                providerLabel: "Antigravity"
            )
            XCTAssertEqual(agyExecRows.count, 1)
            XCTAssertTrue(agyExecRows[0].timelineAliases.contains("group:agy:step:9"))
            XCTAssertTrue(agyExecRows[0].timelineAliases.contains("group:agy-call-9"))
            XCTAssertTrue(agyExecRows[0].timelineAliases.contains("agy-call-9"))

            let dshExecRows = ConversationRowBuilder.buildProviderRows(
                events: [
                    ProviderHistoryEvent(id: "dsh:step:3", kind: "tool_call", role: "tool", text: "bash", callId: "dsh-call-3", status: "completed", detail: "ok"),
                ],
                providerLabel: "DeepSeek Harness"
            )
            XCTAssertEqual(dshExecRows.count, 1)
            XCTAssertTrue(dshExecRows[0].timelineAliases.contains("group:dsh:step:3"))
            XCTAssertTrue(dshExecRows[0].timelineAliases.contains("group:dsh-call-3"))
            XCTAssertTrue(dshExecRows[0].timelineAliases.contains("dsh-call-3"))

            // Verify updateRowsPreservingAnchor, restoreExplicitAnchor, and scrollViewFrameDidChange resolve alias anchors cleanly
            let resizeSV = NSScrollView(frame: NSRect(x: 0, y: 0, width: 760, height: 220))
            let resizeTV = NSTableView(frame: NSRect(x: 0, y: 0, width: 760, height: 220))
            let resizeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("ResizeCol"))
            resizeCol.width = 760
            resizeTV.addTableColumn(resizeCol)
            resizeTV.headerView = nil
            let resizeCoord = ConversationTableCoordinator(store: store, disclosureStore: ActivityDisclosureStore())
            resizeCoord.sessionScopeKey = "local::codex:resize-alias"
            resizeTV.dataSource = resizeCoord
            resizeTV.delegate = resizeCoord
            resizeSV.documentView = resizeTV
            resizeCoord.scrollView = resizeSV
            resizeCoord.tableView = resizeTV
            resizeCoord.column = resizeCol

            let fillerRows: [ConversationRowItem] = (0 ..< 8).map { i in
                ConversationRowItem(
                    id: "filler-\(i)",
                    kind: .assistant(markdown: "前序长段落 \(i)\n第二行内容 \(i)\n第三行内容 \(i)", foldInstructions: false)
                )
            }
            let mergedExecGroup = ExecutionGroup(
                id: "group:older-cmd",
                mergedIds: ["group:anchor-before-merge"],
                commands: [
                    ExecutionCommand(id: "older-cmd", sourceIds: ["older-cmd"], title: "ls", preview: nil, status: .completed, instructionFile: false, sections: []),
                    ExecutionCommand(id: "anchor-cmd", sourceIds: ["anchor-src"], title: "pwd", preview: nil, status: .completed, instructionFile: false, sections: []),
                ],
                kind: "command",
                provider: "Codex",
                running: false,
                recordedOnly: false,
                failedCount: 0
            )
            let mergedExecRow = ConversationRowItem(
                id: mergedExecGroup.id,
                timelineId: mergedExecGroup.id,
                timelineAliases: ProcessGroupingEngine.uniqueStrings(
                    mergedExecGroup.stateAliases + mergedExecGroup.commands.flatMap { [$0.id] + $0.sourceIds }
                ),
                kind: .executionGroup(mergedExecGroup)
            )
            let tailFillerRows: [ConversationRowItem] = (0 ..< 8).map { i in
                ConversationRowItem(
                    id: "tail-\(i)",
                    kind: .assistant(markdown: "后续段落 \(i)\n第二行 \(i)\n第三行 \(i)", foldInstructions: false)
                )
            }
            let preMergeRow = ConversationRowItem(
                id: "group:anchor-before-merge",
                timelineId: "group:anchor-before-merge",
                timelineAliases: [],
                kind: .executionGroup(mergedExecGroup)
            )
            resizeCoord.updateRowsPreservingAnchor(fillerRows + [preMergeRow] + tailFillerRows, forceScrollToBottom: false)
            resizeSV.frame = NSRect(x: 0, y: 0, width: 760, height: 220)
            resizeCoord.scrollViewFrameDidChange(Notification(name: NSView.frameDidChangeNotification, object: resizeSV))
            let targetIdx = fillerRows.count
            let preMergeRect = resizeTV.rect(ofRow: targetIdx)
            resizeSV.contentView.setBoundsOrigin(NSPoint(x: 0, y: preMergeRect.minY - 18))
            resizeSV.reflectScrolledClipView(resizeSV.contentView)

            // Replace preMergeRow with mergedExecRow (whose timelineId changed to "group:older-cmd" while "group:anchor-before-merge" & "group:anchor-src" live in timelineAliases)
            let extraPrependedFiller = ConversationRowItem(
                id: "prepended-top",
                kind: .assistant(markdown: "顶部前插多行一\n顶部前插多行二\n顶部前插多行三", foldInstructions: false)
            )
            resizeCoord.updateRowsPreservingAnchor([extraPrependedFiller] + fillerRows + [mergedExecRow] + tailFillerRows, forceScrollToBottom: false)
            let shiftedIdx = targetIdx + 1
            XCTAssertEqual(
                resizeSV.contentView.bounds.origin.y,
                resizeTV.rect(ofRow: shiftedIdx).minY - 18,
                accuracy: 1.0,
                "updateRowsPreservingAnchor must lock 0.0pt onto merged ExecutionGroup via timelineAliases"
            )
            resizeCoord.restoreExplicitAnchor(
                ConversationTableCoordinator.ScrollAnchorSnapshot(
                    timelineId: "group:anchor-src",
                    deltaY: 24,
                    savedScrollTop: 0,
                    savedContentHeight: resizeTV.frame.height
                )
            )
            XCTAssertEqual(resizeSV.contentView.bounds.origin.y, resizeTV.rect(ofRow: shiftedIdx).minY - 24, accuracy: 1.0)
            resizeSV.frame = NSRect(x: 0, y: 0, width: 460, height: 220)
            resizeCoord.scrollViewFrameDidChange(Notification(name: NSView.frameDidChangeNotification, object: resizeSV))
            XCTAssertEqual(
                resizeSV.contentView.bounds.origin.y,
                resizeTV.rect(ofRow: shiftedIdx).minY - 24,
                accuracy: 1.0,
                "scrollViewFrameDidChange must preserve pixel deltaY across width reflow"
            )

            // 4. ActivityDisclosureStore refreshes LRU position when re-toggling an existing key,
            //    and MessageBlock.phase wire JSON is forward/backward compatible with 2.1.35.
            let lruStore = ActivityDisclosureStore()
            lruStore.setOpen(scope: "s", stateKey: "early-key", isOpen: true)
            for i in 0 ..< 1500 {
                lruStore.setOpen(scope: "s", stateKey: "mid-\(i)", isOpen: true)
            }
            // Re-toggle early-key so it moves to the newest position in order
            lruStore.setOpen(scope: "s", stateKey: "early-key", isOpen: false)
            for i in 0 ..< 600 {
                lruStore.setOpen(scope: "s", stateKey: "late-\(i)", isOpen: true)
            }
            XCTAssertFalse(
                lruStore.isOpen(scope: "s", stateKey: "early-key", initiallyOpen: true),
                "Recently updated disclosure key must survive eviction of older entries"
            )

            let legacyBlockJSON = Data(#"{"id":"b1","role":"assistant","kind":"message","text":"legacy 2.1.35","answers":[],"questions":[]}"#.utf8)
            let decodedLegacy = try JSONDecoder().decode(MessageBlock.self, from: legacyBlockJSON)
            XCTAssertNil(decodedLegacy.phase)
            let commentaryBlock = MessageBlock(id: "b2", role: "assistant", kind: "message", phase: "commentary", text: "mid")
            let encodedCommentary = try JSONEncoder().encode(commentaryBlock)
            let roundtripped = try JSONDecoder().decode(MessageBlock.self, from: encodedCommentary)
            XCTAssertEqual(roundtripped.phase, "commentary")

            // 5. ConversationReaderTableView.makeNSView syncs sessionScopeKey & scheme BEFORE initial row height measurement
            let mountDisc = ActivityDisclosureStore()
            let mountCoord = ConversationTableCoordinator(store: store, disclosureStore: mountDisc)
            mountCoord.sessionScopeKey = "local:subagent:root:old-sub"
            let subRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "", kind: "thinking", text: "子代理一长思考\n第二行\n第三行", status: "completed"),
            ])
            mountDisc.setOpen(scope: "local:subagent:root:old-sub", stateKey: subRows[0].id, aliases: subRows[0].timelineAliases, isOpen: true)
            let readerHost = NSHostingView(
                rootView: ConversationReaderTableView(
                    coordinator: mountCoord,
                    rows: subRows,
                    scheme: .dark,
                    workspace: .claude,
                    sessionScopeKey: "local:subagent:root:new-sub",
                    sessionProvider: .claudeCode,
                    sessionKey: "new-sub",
                    hasTimelineRail: true,
                    isSubagent: true
                )
                .frame(width: 480, height: 320)
            )
            readerHost.frame = NSRect(x: 0, y: 0, width: 480, height: 320)
            readerHost.layoutSubtreeIfNeeded()
            XCTAssertEqual(mountCoord.sessionScopeKey, "local:subagent:root:new-sub")
            XCTAssertEqual(mountCoord.scheme, .dark)
            if let tv = mountCoord.tableView {
                XCTAssertEqual(mountCoord.tableView(tv, heightOfRow: 0), 34, accuracy: 0.5, "Initial mount must measure row height under new sessionScopeKey")
            }

            // 6. Regression coverage for synthetic process/metadata timelineAliases, monospace line-height parity,
            //    narrow-width (<=520pt) CJK + inline-code wrapping, empty-entry Claude spacing, and MenuAnchorProbeNSView:
            // 6a. UnifiedProcessGroup, ClaudeProcessGroup, and ClaudeMetadataGroup preserve synthetic timelineId aliases across prepend
            let tailProcRows = ConversationRowBuilder.buildCodexRows(
                blocks: [
                    MessageBlock(id: "p-tail-th", role: "assistant", kind: "message", phase: "commentary", text: "尾部推理", turnId: "t-p"),
                    MessageBlock(id: "p-tail-cmd", role: "tool", kind: "command_execution", status: "completed", text: "ok", input: "{\"cmd\":\"ls\"}", toolName: "exec_command", callId: "p-tail-cmd", turnId: "t-p"),
                ],
                showAllHistory: true,
                planFallbackTitle: "P",
                subagentsSupported: true
            )
            let prependedProcRows = ConversationRowBuilder.buildCodexRows(
                blocks: [
                    MessageBlock(id: "p-head-th", role: "assistant", kind: "message", phase: "commentary", text: "前插推理", turnId: "t-p"),
                    MessageBlock(id: "p-tail-th", role: "assistant", kind: "message", phase: "commentary", text: "尾部推理", turnId: "t-p"),
                    MessageBlock(id: "p-tail-cmd", role: "tool", kind: "command_execution", status: "completed", text: "ok", input: "{\"cmd\":\"ls\"}", toolName: "exec_command", callId: "p-tail-cmd", turnId: "t-p"),
                ],
                showAllHistory: true,
                planFallbackTitle: "P",
                subagentsSupported: true
            )
            XCTAssertTrue(
                prependedProcRows[0].timelineAliases.contains(tailProcRows[0].timelineId),
                "UnifiedProcessGroup must retain pre-prepend synthetic timelineId in timelineAliases"
            )

            let tailClaudeProc = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "cp-2", kind: "thinking", text: "后段思考", turnId: "t-cp", status: "completed"),
                ClaudeHistoryEvent(id: "cp-fin", kind: "assistant_message", role: "assistant", text: "结语", turnId: "t-cp", assistantPhase: "final"),
            ])
            let prepClaudeProc = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "cp-1", kind: "thinking", text: "前插思考", turnId: "t-cp", status: "completed"),
                ClaudeHistoryEvent(id: "cp-2", kind: "thinking", text: "后段思考", turnId: "t-cp", status: "completed"),
                ClaudeHistoryEvent(id: "cp-fin", kind: "assistant_message", role: "assistant", text: "结语", turnId: "t-cp", assistantPhase: "final"),
            ])
            XCTAssertTrue(
                prepClaudeProc[0].timelineAliases.contains(tailClaudeProc[0].timelineId),
                "ClaudeProcessGroup must retain pre-prepend claude-process[turnId][sid] timelineId in timelineAliases"
            )

            let tailClaudeMeta = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "meta-2", kind: "system", role: "native_context", text: "上下文二", detail: "自动上下文二"),
            ])
            let prepClaudeMeta = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "meta-1", kind: "system", role: "native_context", text: "上下文一", detail: "自动上下文一"),
                ClaudeHistoryEvent(id: "meta-2", kind: "system", role: "native_context", text: "上下文二", detail: "自动上下文二"),
            ])
            XCTAssertEqual(prepClaudeMeta.count, 1)
            XCTAssertTrue(
                prepClaudeMeta[0].timelineAliases.contains(tailClaudeMeta[0].timelineId),
                "ClaudeMetadataGroup must retain pre-prepend claude-metadata:<sid> timelineId in timelineAliases"
            )

            // 6b. Multi-line expanded monospace box & 12-command group match laid-out stack height without row overlap or 2pt*N drift
            let tenLineOut = (1 ... 10).map { "line \($0): cargo test --workspace --locked -> ok" }.joined(separator: "\n")
            let manyCmds: [ExecutionCommand] = (0 ..< 12).map { idx in
                ExecutionCommand(
                    id: "mc-\(idx)",
                    sourceIds: ["mc-\(idx)"],
                    title: "exec_command",
                    preview: "step \(idx)",
                    status: idx == 0 ? .failed : .completed,
                    instructionFile: false,
                    sections: [ExecutionSection(label: "输出", text: idx == 0 ? tenLineOut : "ok")]
                )
            }
            let manyExecGroup = ExecutionGroup(
                id: "group:mc-0",
                mergedIds: [],
                commands: manyCmds,
                kind: "command",
                provider: "Codex",
                running: false,
                recordedOnly: false,
                failedCount: 1
            )
            let manyExecRow = ConversationRowItem(
                id: manyExecGroup.id,
                timelineId: manyExecGroup.id,
                timelineAliases: manyExecGroup.stateAliases,
                kind: .executionGroup(manyExecGroup)
            )
            let manyExecH = coord.measureRowHeight(manyExecRow, width: 680)
            let manyExecCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ManyExecParityCell"))
            manyExecCell.frame = NSRect(x: 0, y: 0, width: 720, height: manyExecH)
            manyExecCell.configure(item: manyExecRow, leadingInset: 20, trailingInset: 20, contentWidth: 680, highlighted: false, coordinator: coord)
            manyExecCell.layoutSubtreeIfNeeded()
            let manyExecStack = try XCTUnwrap(manyExecCell.subviews.first)
            XCTAssertEqual(
                manyExecH,
                manyExecStack.fittingSize.height + 4,
                accuracy: 2.0,
                "10-line expanded monospace output + 11 collapsed 30pt tool rows must never clip or drift by 2pt*N"
            )

            // 6c. Claude single visible thought + whitespace/citation-only thought opens directly with 0pt phantom spacing
            let singleVisibleClaudeRows = ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "sv-empty", kind: "thinking", text: "   \n  ", turnId: "t-sv", status: "completed"),
                ClaudeHistoryEvent(id: "sv-cite", kind: "thinking", text: "<oai-mem-citation>hidden</oai-mem-citation>", turnId: "t-sv", status: "completed"),
                ClaudeHistoryEvent(id: "sv-real", kind: "thinking", text: "唯一可见思考正文\n无需二次点开", turnId: "t-sv", status: "completed"),
                ClaudeHistoryEvent(id: "sv-fin", kind: "assistant_message", role: "assistant", text: "答复", turnId: "t-sv", assistantPhase: "final"),
            ])
            guard case let .claudeProcessGroup(svProc, _) = singleVisibleClaudeRows[0].kind else {
                return XCTFail("Expected .claudeProcessGroup")
            }
            XCTAssertFalse(svProc.wrapsThinkingDisclosures, "Single visible thinking block must open directly without redundant inner disclosure")
            disc.setOpen(
                scope: coord.sessionScopeKey,
                stateKey: svProc.stateKey,
                aliases: svProc.sourceIds.map { "claude-process:\(svProc.turnId):\($0)" },
                isOpen: true
            )
            let svH = coord.measureRowHeight(singleVisibleClaudeRows[0], width: 720)
            let svCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ClaudeSingleVisibleCell"))
            svCell.frame = NSRect(x: 0, y: 0, width: 760, height: svH)
            svCell.configure(item: singleVisibleClaudeRows[0], leadingInset: 20, trailingInset: 20, contentWidth: 720, highlighted: false, coordinator: coord)
            svCell.layoutSubtreeIfNeeded()
            XCTAssertTrue(collectAllText(svCell).contains("无需二次点开"))
            let svStack = try XCTUnwrap(svCell.subviews.first)
            XCTAssertGreaterThanOrEqual(svH, svStack.fittingSize.height + 2)
            XCTAssertLessThanOrEqual(svH, svStack.fittingSize.height + 8)

            // 6d. Narrow reading column (<= 520pt) wrapping CJK + long unbroken inline `code` neither clips descenders nor jumps scroll,
            //     and scrollToPositionKey opens inner ClaudeMetadataGroup items without firing spurious onLoadOlderRequested.
            let narrowLongThought = "窄阅读列折行核验：包含超长内联代码 `ConversationTableCoordinator_scrollViewFrameDidChange_findAnchorRowIndex_0pt_parity_check` 与多行中文段落。"
            disc.setOpen(scope: resizeCoord.sessionScopeKey, stateKey: "codex:process:narrow-th", aliases: ["codex:process:narrow-th"], isOpen: true)
            disc.setOpen(scope: resizeCoord.sessionScopeKey, stateKey: "narrow-th:thinking", aliases: ["narrow-th:thinking"], isOpen: true)
            let narrowProc = ProcessGroupingEngine.buildUnifiedProcessGroup(
                prefix: "codex",
                steps: [
                    .thinking(id: "narrow-th", sourceIds: ["narrow-th"], text: narrowLongThought, running: false, turnId: "t-n"),
                    .thinking(id: "narrow-th-2", sourceIds: ["narrow-th-2"], text: "闭合次段", running: false, turnId: "t-n"),
                ]
            )
            let narrowRow = ConversationRowItem(id: narrowProc.id, timelineId: narrowProc.id, timelineAliases: narrowProc.timelineAliases, kind: .processGroup(narrowProc))
            let narrowColW: CGFloat = 380
            let narrowMeasuredH = resizeCoord.measureRowHeight(narrowRow, width: narrowColW)
            let narrowCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("NarrowWrapCell"))
            narrowCell.frame = NSRect(x: 0, y: 0, width: 420, height: narrowMeasuredH)
            narrowCell.configure(item: narrowRow, leadingInset: 20, trailingInset: 20, contentWidth: narrowColW, highlighted: false, coordinator: resizeCoord)
            narrowCell.layoutSubtreeIfNeeded()
            let narrowStack = try XCTUnwrap(narrowCell.subviews.first)
            XCTAssertGreaterThanOrEqual(narrowMeasuredH, narrowStack.fittingSize.height + 2)
            XCTAssertLessThanOrEqual(narrowMeasuredH, narrowStack.fittingSize.height + 10)

            var loadOlderCount = 0
            resizeCoord.canLoadOlder = true
            resizeCoord.onLoadOlderRequested = { loadOlderCount += 1 }
            resizeCoord.updateRowsPreservingAnchor(prepClaudeMeta + tailFillerRows, forceScrollToBottom: true)
            XCTAssertTrue(resizeCoord.scrollToPositionKey("meta-2", reduceMotion: true))
            XCTAssertEqual(loadOlderCount, 0, "scrollToPositionKey jump near top must not spuriously fire onLoadOlderRequested")
            XCTAssertTrue(
                resizeCoord.disclosureStore.isOpen(scope: resizeCoord.sessionScopeKey, stateKey: "claude:meta-2:0", aliases: ["meta-2"], initiallyOpen: false),
                "scrollToPositionKey must open the matched inner ClaudeMetadataGroup detail entry"
            )

            // 6e. MenuAnchorProbeNSView.resolveAnchorView never promotes an unlaid-out probe to a full-window root superview
            let fakeWindowRoot = NSView(frame: NSRect(x: 0, y: 0, width: 1280, height: 820))
            let unlaidOutProbe = NSView(frame: .zero)
            fakeWindowRoot.addSubview(unlaidOutProbe)
            XCTAssertTrue(MenuAnchorProbeNSView.resolveAnchorView(unlaidOutProbe) === unlaidOutProbe)
            let buttonHost = NSView(frame: NSRect(x: 0, y: 0, width: 28, height: 28))
            let buttonProbe = NSView(frame: .zero)
            buttonHost.addSubview(buttonProbe)
            XCTAssertTrue(MenuAnchorProbeNSView.resolveAnchorView(buttonProbe) === buttonHost)
        }
    }
}
