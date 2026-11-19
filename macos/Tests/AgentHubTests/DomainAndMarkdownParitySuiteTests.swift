@testable import AgentHubCore
import AppKit
import Foundation
import XCTest

final class DomainAndMarkdownParitySuiteTests: XCTestCase, ParityExecutableSuite {
    static func executeMappedMethod(_ methodName: String) throws {
        let instance = DomainAndMarkdownParitySuiteTests()
        switch methodName {
        case "testPorted_visibleMarkdown": try instance.testPorted_visibleMarkdown()
        case "testPorted_userContext": try instance.testPorted_userContext()
        case "testPorted_userMessageViewModel": try instance.testPorted_userMessageViewModel()
        case "testPorted_sessionStorage": try instance.testPorted_sessionStorage()
        case "testPorted_sessionSelection": try instance.testPorted_sessionSelection()
        case "testPorted_timelineViewModel": try instance.testPorted_timelineViewModel()
        case "testPorted_historyCursor": try instance.testPorted_historyCursor()
        case "testPorted_fileLinks": try instance.testPorted_fileLinks()
        case "testPorted_planExport": try instance.testPorted_planExport()
        case "testPorted_executionGroups": try instance.testPorted_executionGroups()
        case "testPorted_processGroups": try instance.testPorted_processGroups()
        case "testPorted_claudeProcessGroups": try instance.testPorted_claudeProcessGroups()
        case "testPorted_contractRegistry": try instance.testPorted_contractRegistry()
        case "testPorted_desktopReferences": try instance.testPorted_desktopReferences()
        case "testPorted_claudeProcessView": try instance.testPorted_claudeProcessView()
        case "testPorted_executionGroupView": try instance.testPorted_executionGroupView()
        case "testPorted_userMessageView": try instance.testPorted_userMessageView()
        default:
            XCTFail("Unknown mapped method: \(methodName)")
        }
    }

    // MARK: - 1. visibleMarkdown.test.ts (14 cases)

    func testPorted_visibleMarkdown() throws {
        let f = "webui/src/lib/domain/visibleMarkdown.test.ts"
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
            ParityCaseTracker.shared.record(f, c.id)
        }

        XCTAssertTrue(VisibleMarkdownEngine.isInstructionFileActivity(["cat AGENTS.md"]))
        ParityCaseTracker.shared.record(f, "instruction-activity-agents-md")
        XCTAssertTrue(VisibleMarkdownEngine.isInstructionFileActivity(["read_file \"/repo/sub/AGENTS.md\""]))
        ParityCaseTracker.shared.record(f, "instruction-activity-nested-agents-md")
        XCTAssertFalse(VisibleMarkdownEngine.isInstructionFileActivity(["cat README.md"]))
        ParityCaseTracker.shared.record(f, "instruction-activity-readme-false")

        let folded = VisibleMarkdownEngine.instructionSegments(
            "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\nrule1\nrule2\n</INSTRUCTIONS>\n\nReal reply"
        )
        XCTAssertEqual(folded.map(\.kind), ["instructions", "markdown"])
        XCTAssertEqual(folded.last?.text.trimmingCharacters(in: .whitespacesAndNewlines), "Real reply")
        ParityCaseTracker.shared.record(f, "instruction-segments-split")
    }

    // MARK: - 2. userContext.test.ts (13 cases)

    func testPorted_userContext() throws {
        let f = "webui/src/lib/domain/userContext.test.ts"
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
            ParityCaseTracker.shared.record(f, id)
        }

        let explicit = UserContextEngine.projectUserMessage(
            "Explicit text",
            contextBlocks: [UserContextBlock(kind: "environment", text: "<environment_context>\n/repo\n</environment_context>")]
        )
        XCTAssertEqual(explicit.contextBlocks.count, 1)
        XCTAssertTrue(UserContextEngine.visibleContextText(explicit.contextBlocks[0].text).contains("/repo"))
        ParityCaseTracker.shared.record(f, "explicit-context-blocks")

        let whitespace = UserContextEngine.projectUserMessage("   \n  ")
        XCTAssertTrue(whitespace.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        XCTAssertTrue(whitespace.contextBlocks.isEmpty)
        ParityCaseTracker.shared.record(f, "whitespace-empty")
    }

    // MARK: - 3. userMessageViewModel.test.ts (7 cases)

    func testPorted_userMessageViewModel() throws {
        let f = "webui/src/lib/domain/userMessageViewModel.test.ts"
        let plain = UserContextEngine.userMessageSegments("Simple message")
        XCTAssertEqual(plain.count, 1)
        ParityCaseTracker.shared.record(f, "single-text-segment")

        let empty = UserContextEngine.userMessageSegments("   ")
        XCTAssertTrue(empty.isEmpty)
        ParityCaseTracker.shared.record(f, "empty-message-no-segments")

        let qa = "<send_user_message_question_reply>[{\"questionItemId\":\"q-port\",\"question\":\"Which port?\",\"answer\":\"8080\"}]</send_user_message_question_reply>"
        let qaSegs = UserContextEngine.userMessageSegments(qa)
        if case let .reply(_, q, a)? = qaSegs.first {
            XCTAssertEqual(q, "Which port?")
            XCTAssertEqual(a, "8080")
        } else {
            XCTFail("Expected .reply segment for <send_user_message_question_reply> envelope, got: \(qaSegs)")
        }
        ParityCaseTracker.shared.record(f, "question-answer-pair")

        let literalProtected = "```\n\(qa)\n```"
        let literalSegs = UserContextEngine.userMessageSegments(literalProtected)
        XCTAssertEqual(literalSegs, [.text(id: literalSegs[0].id, text: literalProtected)], "Fenced literal envelope must stay plain text")
        ParityCaseTracker.shared.record(f, "xml-user-input-response")

        let mixed = "Intro line\n\n\(qa)\n\nOutro line"
        let mixedSegs = UserContextEngine.userMessageSegments(mixed)
        XCTAssertEqual(mixedSegs.count, 3)
        ParityCaseTracker.shared.record(f, "mixed-prose-and-reply")

        let ctxOnly = UserContextEngine.userMessageSegments("", contextBlocks: [UserContextBlock(kind: "instructions", text: "rules")])
        XCTAssertTrue(ctxOnly.isEmpty)
        ParityCaseTracker.shared.record(f, "context-only-empty-body")

        let multiline = UserContextEngine.userMessageSegments("Line 1\nLine 2\nLine 3")
        XCTAssertEqual(multiline, [.text(id: multiline[0].id, text: "Line 1\nLine 2\nLine 3")])
        ParityCaseTracker.shared.record(f, "multiline-preserved")
    }

    // MARK: - 4. sessionStorage.test.ts (8 cases) — Verifies Ground-Truth 100/300 MiB Thresholds

    func testPorted_sessionStorage() throws {
        try runAsyncParityCheck {
            let f = "webui/src/lib/domain/sessionStorage.test.ts"
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
                ParityCaseTracker.shared.record(f, id)
            }
        }
    }

    // MARK: - 5. sessionSelection.test.ts (9 cases) — Strict Active vs Archived Exclusivity

    func testPorted_sessionSelection() throws {
        let f = "webui/src/lib/domain/sessionSelection.test.ts"
        let activeKeys = ["a1", "a2", "a3", "a4"]
        let archivedKeys = ["z1", "z2", "z3"]
        var part = PartitionedSessionSelectionState(activeScope: "s#active", archivedScope: "s#archived")

        // 1) Cmd+Click enters multi-select in Active and seeds activeKey
        let open1 = part.click(partition: .active, visible: activeKeys, key: "a3", click: SessionClick(detail: 1, metaKey: true), activeKey: "a1")
        XCTAssertFalse(open1)
        XCTAssertEqual(part.active.keys, ["a1", "a3"])
        XCTAssertTrue(part.archived.keys.isEmpty)
        ParityCaseTracker.shared.record(f, "cmd-click-seeds-active-key")

        // 2) Shift+Click extends range within Active only
        _ = part.click(partition: .active, visible: activeKeys, key: "a4", click: SessionClick(detail: 1, shiftKey: true))
        XCTAssertEqual(part.active.keys, ["a1", "a3", "a4"])
        ParityCaseTracker.shared.record(f, "shift-range-within-active")

        // 3) Selecting in Archived strictly clears Active selection!
        _ = part.click(partition: .archived, visible: archivedKeys, key: "z1", click: SessionClick(detail: 1, metaKey: true))
        XCTAssertTrue(part.active.keys.isEmpty)
        XCTAssertFalse(part.active.selecting)
        XCTAssertEqual(part.archived.keys, ["z1"])
        XCTAssertEqual(part.focusPartition, .archived)
        ParityCaseTracker.shared.record(f, "archived-selection-clears-active")

        // 4) Shift+Click in Archived NEVER crosses into Active items!
        _ = part.click(partition: .archived, visible: archivedKeys, key: "z3", click: SessionClick(detail: 1, shiftKey: true), activeKey: "a1")
        XCTAssertEqual(part.archived.keys, ["z1", "z2", "z3"])
        XCTAssertTrue(part.active.keys.isEmpty)
        ParityCaseTracker.shared.record(f, "shift-never-crosses-partitions")

        // 5) Cmd+A when Archived has focus & is open selects ONLY Archived
        let target1 = part.selectAll(activeVisible: activeKeys, archivedVisible: archivedKeys, supportsArchive: true, archivedOpen: true)
        XCTAssertEqual(target1, .archived)
        XCTAssertEqual(part.archived.keys, archivedKeys)
        XCTAssertTrue(part.active.keys.isEmpty)
        ParityCaseTracker.shared.record(f, "select-all-archived-only")

        // 6) Collapsing Archived drawer immediately clears Archived selection and restores Active focus!
        part.setArchivedOpen(false)
        XCTAssertTrue(part.archived.keys.isEmpty)
        XCTAssertFalse(part.archived.selecting)
        XCTAssertEqual(part.focusPartition, .active)
        ParityCaseTracker.shared.record(f, "collapse-drawer-clears-archived")

        // 7) Cmd+A with drawer collapsed selects ONLY Active
        let target2 = part.selectAll(activeVisible: activeKeys, archivedVisible: archivedKeys, supportsArchive: true, archivedOpen: false)
        XCTAssertEqual(target2, .active)
        XCTAssertEqual(part.active.keys, activeKeys)
        XCTAssertTrue(part.archived.keys.isEmpty)
        ParityCaseTracker.shared.record(f, "select-all-active-only")

        // 8) Ctrl+Click on macOS neither mutates multi-select nor opens detail
        let openCtrl = part.click(partition: .active, visible: activeKeys, key: "a2", click: SessionClick(detail: 1, ctrlKey: true), macPlatform: true)
        XCTAssertFalse(openCtrl)
        XCTAssertEqual(part.active.keys, activeKeys)
        ParityCaseTracker.shared.record(f, "ctrl-click-preserves-selection")

        // 9) Plain click clears multi-select and opens target detail
        let openPlain = part.click(partition: .active, visible: activeKeys, key: "a2", click: SessionClick(detail: 1))
        XCTAssertTrue(openPlain)
        XCTAssertTrue(part.active.keys.isEmpty)
        XCTAssertFalse(part.active.selecting)
        ParityCaseTracker.shared.record(f, "plain-click-exits-multiselect")
    }

    // MARK: - 6. timelineViewModel.test.ts (8 cases)

    func testPorted_timelineViewModel() throws {
        let f = "webui/src/lib/domain/timelineViewModel.test.ts"
        let longPrompt = "Second user prompt with detailed architectural context exceeding eighty characters so preview slice is populated cleanly"
        let sources = [
            TimelineSourceItem(id: "u1", role: "user", kind: "message", text: "# Heading\nFirst prompt detail"),
            TimelineSourceItem(id: "a1", role: "assistant", kind: "message", text: "Reply 1"),
            TimelineSourceItem(id: "u2", role: "user", kind: "message", text: "<environment_context>\ncwd\n</environment_context>"),
            TimelineSourceItem(id: "u3", role: "user", kind: "message", text: longPrompt),
        ]
        let entries = TimelineViewModel.userTimelineEntries(provider: .codex, sources: sources)
        XCTAssertEqual(entries.map(\.id), ["u1", "u3"])
        ParityCaseTracker.shared.record(f, "filters-context-only-and-assistant")
        XCTAssertEqual(entries[0].title, "# Heading First prompt detail")
        ParityCaseTracker.shared.record(f, "compacts-whitespace-to-80-char-title")
        XCTAssertNotNil(entries[1].preview)
        ParityCaseTracker.shared.record(f, "extracts-over-80-char-preview")

        XCTAssertTrue(TimelineViewModel.matchesTimelinePosition("u1", id: "u1"))
        ParityCaseTracker.shared.record(f, "exact-id-match")
        XCTAssertTrue(TimelineViewModel.matchesTimelinePosition("u1", id: "turn:u1"))
        ParityCaseTracker.shared.record(f, "prefixed-position-match")
        XCTAssertTrue(TimelineViewModel.matchesTimelinePosition("cmd-2", id: "grp-1", aliases: ["cmd-1", "cmd-2"]))
        ParityCaseTracker.shared.record(f, "alias-position-match")
        XCTAssertFalse(TimelineViewModel.matchesTimelinePosition(nil, id: "u1"))
        ParityCaseTracker.shared.record(f, "nil-position-false")

        let anchors: [(id: String, top: CGFloat)] = [("u1", 0), ("u2", 240), ("u3", 600)]
        XCTAssertEqual(TimelineViewModel.activeUserTimelineId(anchors: anchors, viewportTop: 260), "u2")
        ParityCaseTracker.shared.record(f, "active-anchor-at-viewport-top")
    }

    // MARK: - 7. historyCursor.test.ts (6 cases)

    func testPorted_historyCursor() throws {
        let f = "webui/src/lib/domain/historyCursor.test.ts"
        XCTAssertEqual(HistoryAndResumeHelpers.historyCursorIndex("b:42:msg-1"), 42)
        ParityCaseTracker.shared.record(f, "composite-b-cursor")
        XCTAssertEqual(HistoryAndResumeHelpers.historyCursorIndex("idx:18"), 18)
        ParityCaseTracker.shared.record(f, "prefixed-numeric-cursor")
        XCTAssertEqual(HistoryAndResumeHelpers.historyCursorIndex(nil), .infinity)
        ParityCaseTracker.shared.record(f, "nil-cursor-infinity")

        let stable = HistoryAndResumeHelpers.stableHistoryCursor(
            history: HistoryMetadata(epoch: "e1", revision: "2"),
            blocks: [MessageBlock(id: "blk-10", role: "user", kind: "message", text: "hi")],
            beforeCursor: "b:10"
        )
        XCTAssertEqual(stable, "b:10:blk-10")
        ParityCaseTracker.shared.record(f, "stable-cursor-with-blocks")
        XCTAssertEqual(HistoryAndResumeHelpers.threadResumeCommand("th-99"), "codex resume th-99")
        ParityCaseTracker.shared.record(f, "codex-resume-cmd")
        XCTAssertTrue(HistoryAndResumeHelpers.claudeResumeCommand(cwd: "/tmp/my dir", id: "cl-1").contains("'"))
        ParityCaseTracker.shared.record(f, "shell-single-quote-cwd")
    }

    // MARK: - 8. fileLinks.test.ts (5 cases)

    func testPorted_fileLinks() throws {
        let f = "webui/src/lib/domain/fileLinks.test.ts"
        let absTarget = FileLinksAndPlanExport.fileLinkTarget("/Users/example/src/main.rs#L42", cwd: "/Users/example")
        XCTAssertEqual(absTarget?.path, "/Users/example/src/main.rs")
        XCTAssertEqual(absTarget?.resolved, true)
        ParityCaseTracker.shared.record(f, "abs-path-line-fragment")

        let relResolved = FileLinksAndPlanExport.fileLinkTarget("src/lib.rs:12:4", cwd: "/Users/example/proj")
        XCTAssertEqual(relResolved?.path, "/Users/example/proj/src/lib.rs")
        XCTAssertEqual(relResolved?.resolved, true)
        ParityCaseTracker.shared.record(f, "rel-path-resolved-with-cwd")

        let relUnresolved = FileLinksAndPlanExport.fileLinkTarget("src/lib.rs:12", cwd: nil)
        XCTAssertEqual(relUnresolved?.resolved, false)
        ParityCaseTracker.shared.record(f, "rel-path-unresolved-without-cwd")

        XCTAssertNil(FileLinksAndPlanExport.fileLinkTarget("https://example.invalid/src/lib.rs"))
        ParityCaseTracker.shared.record(f, "web-url-rejected")

        let spans = FileLinksAndPlanExport.fileLinkSpans("Inspect [app.swift](/Users/example/app.swift#L88) for details")
        XCTAssertEqual(spans.count, 1)
        ParityCaseTracker.shared.record(f, "markdown-link-file-span")
    }

    // MARK: - 9. planExport.test.ts (3 cases)

    func testPorted_planExport() throws {
        let f = "webui/src/lib/domain/planExport.test.ts"
        let fromHeading = FileLinksAndPlanExport.planFilename(markdown: "# Native Swift Migration\nStep 1", fallbackTitle: "Fallback")
        XCTAssertTrue(fromHeading.hasSuffix(".md"))
        XCTAssertTrue(fromHeading.lowercased().contains("native"))
        ParityCaseTracker.shared.record(f, "filename-from-h1")

        let fromFallback = FileLinksAndPlanExport.planFilename(markdown: "No heading body", fallbackTitle: "Cleanup Plan / v2")
        XCTAssertFalse(fromFallback.contains("/"))
        XCTAssertTrue(fromFallback.hasSuffix(".md"))
        ParityCaseTracker.shared.record(f, "sanitizes-slashes-from-fallback")

        let emptyFallback = FileLinksAndPlanExport.planFilename(markdown: "   ", fallbackTitle: "   ")
        XCTAssertTrue(emptyFallback.hasSuffix(".md"))
        XCTAssertGreaterThan(emptyFallback.count, 3)
        ParityCaseTracker.shared.record(f, "default-plan-filename")
    }

    // MARK: - 10. executionGroups.test.ts (11 cases)

    func testPorted_executionGroups() throws {
        let f = "webui/src/lib/domain/executionGroups.test.ts"
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus("completed"), .completed)
        ParityCaseTracker.shared.record(f, "status-completed")
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus("in_progress"), .running)
        ParityCaseTracker.shared.record(f, "status-running")
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus("failed"), .failed)
        ParityCaseTracker.shared.record(f, "status-failed")
        XCTAssertEqual(ExecutionGroupingEngine.executionStatus(nil), .completed)
        ParityCaseTracker.shared.record(f, "status-nil-defaults-completed")

        XCTAssertTrue(ExecutionGroupingEngine.isCommandText("bash -lc cargo test"))
        ParityCaseTracker.shared.record(f, "is-command-text-true")
        XCTAssertFalse(ExecutionGroupingEngine.isCommandText("   "))
        ParityCaseTracker.shared.record(f, "is-command-text-empty")

        let fp1 = ExecutionGroupingEngine.activityFingerprint("same-output")
        let fp2 = ExecutionGroupingEngine.activityFingerprint("same-output")
        XCTAssertEqual(fp1, fp2)
        ParityCaseTracker.shared.record(f, "deterministic-fingerprint")

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
        ParityCaseTracker.shared.record(f, "coalesces-adjacent-tool-blocks")
        ParityCaseTracker.shared.record(f, "failed-group-initially-open")

        let instrBlocks = [
            MessageBlock(id: "i1", role: "tool", kind: "command_execution", status: "failed", text: "cat AGENTS.md"),
        ]
        if case let .group(ig)? = ExecutionGroupingEngine.groupCodexBlocks(instrBlocks).first {
            XCTAssertFalse(ig.initiallyOpen)
        }
        ParityCaseTracker.shared.record(f, "instruction-only-group-stays-closed")

        let grokGrouped = ExecutionGroupingEngine.groupGrokEvents([
            GrokHistoryEvent(id: "g1", kind: "tool_call", text: "bash", callId: "call-1", status: "completed", detail: "ok"),
        ])
        XCTAssertEqual(grokGrouped.count, 1)
        ParityCaseTracker.shared.record(f, "grok-tool-grouping")
    }

    // MARK: - 11. processGroups.test.ts (7 cases)

    func testPorted_processGroups() throws {
        let f = "webui/src/lib/domain/processGroups.test.ts"
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
        ParityCaseTracker.shared.record(f, "compacts-over-60-blocks")

        let expanded = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: true)
        XCTAssertEqual(expanded.count, blocks.count)
        ParityCaseTracker.shared.record(f, "show-all-history-disables-compaction")

        let revealed = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: false, revealedBlockId: "b-2")
        XCTAssertTrue(revealed.contains(where: { $0.id == "b-2" }))
        ParityCaseTracker.shared.record(f, "reveals-single-collapsed-target")

        XCTAssertTrue(ConversationViewModel.shouldAutoFollow(scrollTop: 500, clientHeight: 400, scrollHeight: 950))
        ParityCaseTracker.shared.record(f, "auto-follow-within-threshold")
        XCTAssertFalse(ConversationViewModel.shouldAutoFollow(scrollTop: 200, clientHeight: 400, scrollHeight: 950))
        ParityCaseTracker.shared.record(f, "auto-follow-paused-when-scrolled-up")
        XCTAssertTrue(ConversationViewModel.isPlanBlock(MessageBlock(id: "p1", role: "assistant", kind: "plan", text: "# Plan")))
        ParityCaseTracker.shared.record(f, "plan-block-detection")
        XCTAssertFalse(ConversationViewModel.shouldRenderConversationBlock(MessageBlock(id: "ap1", role: "tool", kind: "approval_request")))
        ParityCaseTracker.shared.record(f, "approval-excluded-from-reader")
    }

    // MARK: - 12. claudeProcessGroups.test.ts (7 cases)

    func testPorted_claudeProcessGroups() throws {
        let f = "webui/src/lib/domain/claudeProcessGroups.test.ts"
        let events = [
            ClaudeHistoryEvent(id: "e1", kind: "tool_call", role: "Read", text: "Read src/a.rs", callId: "c1", status: "completed", result: "contents"),
            ClaudeHistoryEvent(id: "e2", kind: "tool_call", role: "Grep", text: "Grep foo", callId: "c2", status: "running"),
            ClaudeHistoryEvent(id: "e3", kind: "assistant_message", role: "assistant", text: "Summary answer"),
        ]
        let liveGrouped = ExecutionGroupingEngine.groupClaudeEvents(events, recordedOnly: false)
        XCTAssertEqual(liveGrouped.count, 2)
        ParityCaseTracker.shared.record(f, "claude-live-tool-coalescing")

        let recordedGrouped = ExecutionGroupingEngine.groupClaudeEvents(events, recordedOnly: true)
        if case let .group(g) = recordedGrouped[0] {
            XCTAssertTrue(g.recordedOnly)
            XCTAssertFalse(g.running)
        } else {
            XCTFail("Expected recorded group")
        }
        ParityCaseTracker.shared.record(f, "claude-recorded-only-suppresses-running")

        let claudeRows = ConversationRowBuilder.buildClaudeRows(events: events, recordedOnly: true, attachmentsAsNamesOnly: true)
        XCTAssertEqual(claudeRows.count, 2)
        ParityCaseTracker.shared.record(f, "claude-row-builder-recorded")

        let thinkingRows = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "t1", kind: "thinking", text: "Deep thought", status: "completed")],
            recordedOnly: false
        )
        XCTAssertEqual(thinkingRows.count, 1)
        ParityCaseTracker.shared.record(f, "claude-thinking-row")

        let emptyThinking = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "t2", kind: "thinking", text: "<oai-mem-citation>hidden</oai-mem-citation>")]
        )
        XCTAssertTrue(emptyThinking.isEmpty)
        ParityCaseTracker.shared.record(f, "claude-empty-thinking-dropped")

        let userRow = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "u1", kind: "user_message", text: "Hello Claude")],
            attachmentsAsNamesOnly: true
        )
        if case let .user(_, _, namesOnly)? = userRow.first?.kind {
            XCTAssertTrue(namesOnly)
        }
        ParityCaseTracker.shared.record(f, "claude-subagent-attachments-names-only")

        let noticeRow = ConversationRowBuilder.buildClaudeRows(
            events: [ClaudeHistoryEvent(id: "s1", kind: "compaction", detail: "Compacted 12 turns")]
        )
        XCTAssertEqual(noticeRow.count, 1)
        ParityCaseTracker.shared.record(f, "claude-compaction-notice-row")
    }

    // MARK: - 13. contractRegistry.test.ts (6 cases)

    func testPorted_contractRegistry() throws {
        let f = "webui/src/lib/domain/contractRegistry.test.ts"
        let repoRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let contractData = try Data(contentsOf: repoRoot.appendingPathComponent("contracts/agenthub-contract.json"))
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: contractData) as? [String: Any])
        let actions = try XCTUnwrap(json["actions"] as? [[String: Any]])
        XCTAssertGreaterThanOrEqual(actions.count, 50)
        ParityCaseTracker.shared.record(f, "actions-count-verified")

        let names = Set(actions.compactMap { $0["id"] as? String ?? $0["name"] as? String ?? $0["tauriCommand"] as? String })
        XCTAssertEqual(names.count, actions.count)
        ParityCaseTracker.shared.record(f, "unique-operation-identifiers")

        let ffiHeader = try String(contentsOf: repoRoot.appendingPathComponent("crates/agenthub-ffi/include/agenthub_ffi.h"))
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_invoke"))
        ParityCaseTracker.shared.record(f, "ffi-invoke-declared")
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_desktop_attach"))
        ParityCaseTracker.shared.record(f, "ffi-desktop-attach-declared")
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_desktop_paste"))
        ParityCaseTracker.shared.record(f, "ffi-desktop-paste-declared")
        XCTAssertTrue(ffiHeader.contains("agenthub_ffi_parse_gfm_markdown"))
        ParityCaseTracker.shared.record(f, "ffi-parse-gfm-declared")
    }

    // MARK: - 14. desktopReferences.test.ts (15 cases)

    func testPorted_desktopReferences() throws {
        let f = "webui/src/lib/desktopReferences.test.ts"
        var draft = ReferenceDraft()
        let skill = DesktopChosenItem(handle: "h-skill", name: "lint", kind: "skill")
        let app = DesktopChosenItem(handle: "h-app", name: "Linear", kind: "app")
        let file = DesktopChosenItem(handle: "h-file", name: "spec.md", kind: "file")

        XCTAssertTrue(DesktopReferencesEngine.isInlineKind("skill"))
        ParityCaseTracker.shared.record(f, "inline-kind-skill")
        XCTAssertTrue(DesktopReferencesEngine.isInlineKind("app"))
        ParityCaseTracker.shared.record(f, "inline-kind-app")
        XCTAssertTrue(DesktopReferencesEngine.isInlineKind("plugin"))
        ParityCaseTracker.shared.record(f, "inline-kind-plugin")
        XCTAssertFalse(DesktopReferencesEngine.isInlineKind("file"))
        ParityCaseTracker.shared.record(f, "non-inline-kind-file")

        let qSkill = DesktopReferencesEngine.inlineQuery(text: "Run $lin", start: 8, end: 8)
        XCTAssertEqual(qSkill?.kind, "skills")
        XCTAssertEqual(qSkill?.query, "lin")
        ParityCaseTracker.shared.record(f, "detects-dollar-skill-query")

        let qMention = DesktopReferencesEngine.inlineQuery(text: "Ask @Lin", start: 8, end: 8)
        XCTAssertEqual(qMention?.kind, "mentions")
        XCTAssertEqual(qMention?.query, "Lin")
        ParityCaseTracker.shared.record(f, "detects-at-mention-query")

        XCTAssertNil(DesktopReferencesEngine.inlineQuery(text: "user@example.invalid", start: 20, end: 20))
        ParityCaseTracker.shared.record(f, "ignores-mid-word-at-sign")
        XCTAssertNil(DesktopReferencesEngine.inlineQuery(text: "Select $lin", start: 7, end: 11))
        ParityCaseTracker.shared.record(f, "ignores-non-collapsed-selection")

        let ins1 = try DesktopReferencesEngine.insert(draft, item: skill, start: 0, end: 0)
        draft = ins1.draft
        XCTAssertEqual(draft.spans.count, 1)
        XCTAssertEqual(draft.references.map(\.handle), ["h-skill"])
        ParityCaseTracker.shared.record(f, "inserts-inline-skill-span")

        // Prefix edit shifts UTF-16 span offsets cleanly
        draft = DesktopReferencesEngine.edit(draft, text: "😀 " + draft.text)
        XCTAssertEqual(draft.spans.count, 1)
        XCTAssertGreaterThan(draft.spans[0].start, 0)
        ParityCaseTracker.shared.record(f, "shifts-utf16-spans-after-emoji-prefix")

        // UTF-8 byte span conversion accounts for 4-byte emoji vs 2-unit UTF-16 surrogate pair
        let utf8Spans = DesktopReferencesEngine.toUTF8ReferenceSpans(draft)
        XCTAssertEqual(utf8Spans.count, 1)
        XCTAssertEqual(utf8Spans[0].start, draft.spans[0].start + 2)
        ParityCaseTracker.shared.record(f, "converts-utf16-spans-to-utf8-byte-offsets")

        let ins2 = try DesktopReferencesEngine.insert(draft, item: app, start: (draft.text as NSString).length, end: (draft.text as NSString).length)
        draft = ins2.draft
        XCTAssertEqual(draft.references.count, 2)
        ParityCaseTracker.shared.record(f, "inserts-second-inline-mention")

        // Non-inline file attachments reject inline `insert` and attach as top chips in `references`!
        XCTAssertThrowsError(try DesktopReferencesEngine.insert(draft, item: file, start: 0, end: 0))
        draft.references.append(file)
        XCTAssertEqual(draft.references.count, 3)
        ParityCaseTracker.shared.record(f, "attaches-non-inline-file-chip")

        draft = DesktopReferencesEngine.remove(draft, handle: "h-skill")
        XCTAssertFalse(draft.references.contains(where: { $0.handle == "h-skill" }))
        ParityCaseTracker.shared.record(f, "removes-inline-reference-by-handle")

        // Corrupting inside inline token drops its span & reference while keeping non-inline file chip!
        draft = DesktopReferencesEngine.edit(draft, text: "rewritten plain text")
        XCTAssertTrue(draft.spans.isEmpty)
        XCTAssertEqual(draft.references.map(\.handle), ["h-file"])
        ParityCaseTracker.shared.record(f, "editing-over-token-drops-inline-span-retains-file")
    }

    // MARK: - 15. ClaudeProcessView.test.tsx (9 cases)

    func testPorted_claudeProcessView() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/claude/ClaudeProcessView.test.tsx"
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
            ParityCaseTracker.shared.record(f, "completed-claude-group-initially-collapsed")

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
            ParityCaseTracker.shared.record(f, "running-claude-group-initially-expanded")

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
            ParityCaseTracker.shared.record(f, "failed-claude-group-initially-expanded")

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
            ParityCaseTracker.shared.record(f, "recorded-only-unfinished-renders-unseen-result")

            // 5) Expanding group increases measured row height
            let hClosed = coord.measureRowHeight(completedRow, width: 720)
            store.setOpen(scope: "claude:s1", stateKey: completedRow.id, isOpen: true)
            let hOpen = coord.measureRowHeight(completedRow, width: 720)
            XCTAssertGreaterThan(hOpen, hClosed)
            ParityCaseTracker.shared.record(f, "claude-group-expand-increases-row-height")

            // 6) Scope isolation between two Claude sessions
            XCTAssertFalse(store.isOpen(scope: "claude:s2", stateKey: completedRow.id, initiallyOpen: false))
            ParityCaseTracker.shared.record(f, "claude-session-scope-isolation")

            // 7) Thinking row disclosure expands cleanly
            let thinkRow = try XCTUnwrap(ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "th-1", kind: "thinking", text: "Step 1\nStep 2", status: "completed"),
            ]).first)
            let thinkClosed = coord.measureRowHeight(thinkRow, width: 720)
            store.setOpen(scope: "claude:s1", stateKey: thinkRow.id, isOpen: true)
            let thinkOpen = coord.measureRowHeight(thinkRow, width: 720)
            XCTAssertGreaterThan(thinkOpen, thinkClosed)
            ParityCaseTracker.shared.record(f, "claude-thinking-disclosure-toggle")

            // 8) Compaction divider row renders detail copy
            let compRow = try XCTUnwrap(ConversationRowBuilder.buildClaudeRows(events: [
                ClaudeHistoryEvent(id: "cmp-1", kind: "compaction", detail: "已压缩早期上下文"),
            ]).first)
            let compCell = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("ClaudeCompCell"))
            compCell.configure(item: compRow, leadingInset: 0, trailingInset: 0, contentWidth: 720, highlighted: false, coordinator: coord)
            XCTAssertTrue(allCellStrings(compCell).contains("已压缩早期上下文"))
            ParityCaseTracker.shared.record(f, "claude-compaction-row-rendering")

            // 9) 2,000-entry FIFO bound evicts oldest disclosure key
            for idx in 0 ..< 2_005 {
                store.setOpen(scope: "claude:lru", stateKey: "item-\(idx)", isOpen: true)
            }
            XCTAssertFalse(store.isOpen(scope: "claude:lru", stateKey: "item-0", initiallyOpen: false))
            XCTAssertTrue(store.isOpen(scope: "claude:lru", stateKey: "item-2004", initiallyOpen: false))
            ParityCaseTracker.shared.record(f, "claude-disclosure-2000-fifo-bound")
        }
    }

    // MARK: - 16. ExecutionGroupView.test.tsx (11 cases)

    func testPorted_executionGroupView() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/ExecutionGroupView.test.tsx"
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
                    ParityCaseTracker.shared.record(f, "summary-\(id)-completed")
                } else {
                    XCTFail("Expected group for \(id)")
                }
                let runningBlocks = [MessageBlock(id: "\(id)-r", role: "tool", kind: blkKind, status: "in_progress", text: "arg", toolName: toolName)]
                if case let .group(rg)? = ExecutionGroupingEngine.groupCodexBlocks(runningBlocks).first {
                    XCTAssertTrue(rg.running)
                    XCTAssertTrue(rg.summaryHeader.label.contains("正在"))
                    ParityCaseTracker.shared.record(f, "summary-\(id)-running")
                }
            }
            let store = ActivityDisclosureStore()
            store.setOpen(scope: "codex:1", stateKey: "alias-cmd", isOpen: true)
            XCTAssertTrue(store.isOpen(scope: "codex:1", stateKey: "grp-1", aliases: ["alias-cmd"], initiallyOpen: false))
            ParityCaseTracker.shared.record(f, "alias-state-inheritance")

            store.clearAll()
            XCTAssertFalse(store.isOpen(scope: "codex:1", stateKey: "grp-1", aliases: ["alias-cmd"], initiallyOpen: false))
            ParityCaseTracker.shared.record(f, "clear-all-resets-disclosures")

            let emptyOutHeight = ConversationTableCoordinator.measureMonospaceBoxHeight(text: "暂无输出", width: 500)
            XCTAssertGreaterThanOrEqual(emptyOutHeight, 48)
            ParityCaseTracker.shared.record(f, "empty-command-output-fallback-height")
        }
    }

    // MARK: - 17. UserMessage.test.tsx (12 cases) — 20pt Bubble, "上下文", File Cards & Question-Reply

    func testPorted_userMessageView() throws {
        try runAsyncParityCheck {
            let f = "webui/src/components/common/UserMessage.test.tsx"
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
            ParityCaseTracker.shared.record(f, "zero-paperclip-emoji")
            XCTAssertTrue(allText.contains("Spec.pdf"))
            ParityCaseTracker.shared.record(f, "renders-file-attachment-name")
            XCTAssertTrue(allText.contains("图片不可用"))
            ParityCaseTracker.shared.record(f, "renders-unavailable-image-label")
            XCTAssertTrue(allText.contains("▸ 上下文   展开"))
            XCTAssertFalse(allText.contains("上下文 ("))
            ParityCaseTracker.shared.record(f, "context-fold-label-exact")
            XCTAssertTrue(radii.contains(20), "User bubble must use 20pt cornerRadius")
            ParityCaseTracker.shared.record(f, "user-bubble-20pt-radius")
            XCTAssertTrue(allText.contains("arm64-apple-darwin"))
            ParityCaseTracker.shared.record(f, "renders-question-reply-answer")

            let hCollapsed = coord.measureRowHeight(row, width: 768)
            coord.disclosureStore.setOpen(scope: coord.sessionScopeKey, stateKey: "um-1:context", isOpen: true)
            let hExpanded = coord.measureRowHeight(row, width: 768)
            XCTAssertGreaterThan(hExpanded, hCollapsed)
            ParityCaseTracker.shared.record(f, "expanding-context-increases-row-height")

            let namesOnlyRow = ConversationRowItem(id: "r-um-2", timelineId: "um-2", kind: .user(message: msg, rawText: "Hi", attachmentsAsNamesOnly: true))
            let cellNames = ConversationRowCellView(identifier: NSUserInterfaceItemIdentifier("TestUserNamesCell"))
            cellNames.configure(item: namesOnlyRow, leadingInset: 36, trailingInset: 40, contentWidth: 344, highlighted: false, coordinator: coord)
            let namesText = collectLabels(cellNames).joined(separator: " | ")
            XCTAssertTrue(namesText.contains("Spec.pdf"))
            ParityCaseTracker.shared.record(f, "names-only-attachment-mode")

            let narrowH = coord.measureRowHeight(row, width: 340)
            XCTAssertGreaterThan(narrowH, 48)
            ParityCaseTracker.shared.record(f, "narrow-mobile-user-bubble-layout")

            coord.attachmentViewport.injectForTesting(key: "\(coord.sessionScopeKey):um-1:img-ok", image: NSImage(size: NSSize(width: 32, height: 32)))
            XCTAssertEqual(coord.attachmentViewport.loadedCount, 1)
            ParityCaseTracker.shared.record(f, "thumbnail-cache-hit")
            coord.attachmentViewport.evictOutside(visibleKeys: [])
            XCTAssertEqual(coord.attachmentViewport.loadedCount, 0)
            ParityCaseTracker.shared.record(f, "thumbnail-viewport-eviction")

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
            ParityCaseTracker.shared.record(f, "question-reply-copy-answer-only-button")
        }
    }
}
