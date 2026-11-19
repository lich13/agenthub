@testable import AgentHubCore
import Foundation
import XCTest

final class GoldenVectorsAndDomainParityTests: XCTestCase {
    private var repoRoot: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }

    // MARK: - 1. ParityManifest 73-of-74 WebUI Test File Guard

    func testParityManifestCoversAll73NonPreviewWebUITestFiles() throws {
        let manifestURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent("ParityManifest.json")
        struct Manifest: Decodable {
            let requiredSourceTestFileCount: Int
            let retiredPreviewOnlyFiles: [String]
            let mappedUnitTestFiles: [String: String]
        }
        let manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: manifestURL))
        XCTAssertEqual(manifest.requiredSourceTestFileCount, 73)
        XCTAssertEqual(manifest.mappedUnitTestFiles.count, 73)
        XCTAssertEqual(manifest.retiredPreviewOnlyFiles, ["webui/src/lib/domain/demoCore.test.ts"])

        // If webui/src still exists on disk (Stage A), verify exact 1-to-1 file set!
        let webuiSrc = repoRoot.appendingPathComponent("webui/src")
        if FileManager.default.fileExists(atPath: webuiSrc.path) {
            var discovered: [String] = []
            if let enumerator = FileManager.default.enumerator(at: webuiSrc, includingPropertiesForKeys: nil) {
                for case let fileURL as URL in enumerator {
                    let name = fileURL.lastPathComponent
                    if name.hasSuffix(".test.ts") || name.hasSuffix(".test.tsx") {
                        let rel = String(fileURL.path.dropFirst(repoRoot.path.count + 1))
                        discovered.append(rel)
                    }
                }
            }
            let expectedAll = Set(manifest.mappedUnitTestFiles.keys).union(manifest.retiredPreviewOnlyFiles)
            XCTAssertEqual(Set(discovered), expectedAll, "Unmapped or missing WebUI test file in ParityManifest.json")
        }

        // Verify every mapped XCTest suite class resolves in this test bundle!
        for (sourceFile, suiteName) in manifest.mappedUnitTestFiles {
            XCTAssertNotNil(
                NSClassFromString("AgentHubTests.\(suiteName)"),
                "Mapped XCTest suite \(suiteName) for \(sourceFile) does not exist"
            )
        }
    }

    // MARK: - 2. Shared Golden Vector #1: visible_markdown.json

    func testSharedGoldenVectorsVisibleMarkdown() throws {
        struct VectorFile: Decodable {
            struct CleanCase: Decodable { let name: String; let input: String; let expected: String }
            struct ActivityCase: Decodable { let inputs: [String]; let expected: Bool }
            struct SegmentCase: Decodable { let name: String; let input: String; let expected: [InstructionSegment] }
            let cleanCases: [CleanCase]
            let instructionActivityCases: [ActivityCase]
            let instructionSegmentCases: [SegmentCase]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/visible_markdown.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))

        for c in vectors.cleanCases {
            XCTAssertEqual(VisibleMarkdownEngine.visibleMarkdown(c.input), c.expected, "cleanCase: \(c.name)")
        }
        for (idx, c) in vectors.instructionActivityCases.enumerated() {
            XCTAssertEqual(
                VisibleMarkdownEngine.isInstructionFileActivity(c.inputs.map { Optional($0) }),
                c.expected,
                "activityCase #\(idx)"
            )
        }
        for c in vectors.instructionSegmentCases {
            XCTAssertEqual(VisibleMarkdownEngine.instructionSegments(c.input), c.expected, "segmentCase: \(c.name)")
        }
    }

    // MARK: - 3. Shared Golden Vector #2: user_context.json

    func testSharedGoldenVectorsUserContext() throws {
        struct VectorFile: Decodable {
            struct CaseItem: Decodable {
                let name: String
                let input: String
                let expected: UserMessageProjection
            }
            let cases: [CaseItem]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/user_context.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        for c in vectors.cases {
            let actual = UserContextEngine.projectUserMessage(c.input)
            XCTAssertEqual(actual, c.expected, "userContext case: \(c.name)")
        }
    }

    // MARK: - 4. Shared Golden Vector #3: session_storage.json

    func testSharedGoldenVectorsSessionStorage() throws {
        struct VectorFile: Decodable {
            struct CaseItem: Decodable {
                let name: String
                let input: SessionStorageSize?
                let expected: FormattedSessionStorage
            }
            let cases: [CaseItem]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/session_storage.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        for c in vectors.cases {
            XCTAssertEqual(SessionStorageFormatter.format(c.input), c.expected, "sessionStorage case: \(c.name)")
        }
    }

    // MARK: - 5. Shared Golden Vector #4: session_selection.json

    func testSharedGoldenVectorsSessionSelection() throws {
        struct VectorFile: Decodable {
            struct StepItem: Decodable {
                let name: String
                let initial: SessionSelectionState
                let activeKey: String?
                let key: String
                let click: SessionClick
                let expected: SessionSelectionState
            }
            let visible: [String]
            let steps: [StepItem]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/session_selection.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        for step in vectors.steps {
            let actual = SessionSelectionEngine.click(
                step.initial,
                visible: vectors.visible,
                key: step.key,
                click: step.click,
                activeKey: step.activeKey,
                macPlatform: true
            )
            XCTAssertEqual(actual, step.expected, "sessionSelection step: \(step.name)")
        }
    }

    // MARK: - 6. Shared Golden Vector #5: desktop_references.json

    func testSharedGoldenVectorsDesktopReferences() throws {
        struct VectorFile: Decodable {
            struct InsertSpec: Decodable {
                let item: DesktopChosenItem
                let start: Int
                let end: Int
            }
            struct CaseItem: Decodable {
                let name: String
                let initialText: String
                let insert: InsertSpec
                let expectedText: String
                let expectedCaretUtf16: Int
                let expectedUtf16Spans: [DesktopReferenceSpan]
                let expectedUtf8Spans: [DesktopReferenceSpan]
            }
            let cases: [CaseItem]
        }
        let url = repoRoot.appendingPathComponent("contracts/parity-vectors/desktop_references.json")
        let vectors = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        for c in vectors.cases {
            let initial = DesktopReferencesEngine.edit(ReferenceDraft(), text: c.initialText)
            let inserted = try DesktopReferencesEngine.insert(
                initial,
                item: c.insert.item,
                start: c.insert.start,
                end: c.insert.end
            )
            XCTAssertEqual(inserted.draft.text, c.expectedText, "\(c.name) text")
            XCTAssertEqual(inserted.caret, c.expectedCaretUtf16, "\(c.name) caret")
            XCTAssertEqual(inserted.draft.spans, c.expectedUtf16Spans, "\(c.name) utf16 spans")
            XCTAssertEqual(
                DesktopReferencesEngine.toUTF8ReferenceSpans(inserted.draft),
                c.expectedUtf8Spans,
                "\(c.name) utf8 spans"
            )
        }
    }

    // MARK: - 7. Domain Compaction, 2000-Key FIFO Disclosure, 120s Optimistic Title & Plan Export

    @MainActor
    func testHistoryCompaction60And3Disclosure2000FIFOAndOptimistic120sTitle() {
        // 65 chat messages + 5 plan blocks -> compacts to 60 chat + 3 plan + 2 collapse markers
        var blocks: [MessageBlock] = []
        for i in 0 ..< 5 {
            blocks.append(MessageBlock(id: "plan-\(i)", role: "assistant", kind: "plan", status: "completed", text: "# Plan \(i)"))
        }
        for i in 0 ..< 65 {
            blocks.append(MessageBlock(id: "msg-\(i)", role: i.isMultiple(of: 2) ? "user" : "assistant", kind: "message", text: "Hello \(i)"))
        }
        let compacted = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: false)
        XCTAssertTrue(compacted.contains { $0.id == "action-history-collapsed" })
        XCTAssertTrue(compacted.contains { $0.id == "chat-history-collapsed" })
        let uncompacted = ConversationViewModel.compactConversationBlocks(blocks, showAllHistory: true)
        XCTAssertEqual(uncompacted.count, 70)

        // ActivityDisclosureStore 2000-key FIFO cap
        let disc = ActivityDisclosureStore()
        for i in 0 ..< 2005 {
            disc.setOpen(scope: "s", stateKey: "k-\(i)", isOpen: true)
        }
        XCTAssertEqual(disc.count, 2000)
        XCTAssertFalse(disc.isOpen(scope: "s", stateKey: "k-0", initiallyOpen: false), "Oldest key k-0 should be evicted")
        XCTAssertTrue(disc.isOpen(scope: "s", stateKey: "k-2004", initiallyOpen: false))

        // 120s optimistic Codex title cache
        let store = ThreadMessageStore()
        let t0 = Date(timeIntervalSince1970: 1_700_000_000)
        store.recordOptimisticTitle(threadId: "th-1", title: "新标题", now: t0)
        XCTAssertEqual(store.effectiveTitle(threadId: "th-1", serverTitle: "旧标题", now: t0.addingTimeInterval(60)), "新标题")
        XCTAssertEqual(store.effectiveTitle(threadId: "th-1", serverTitle: "旧标题", now: t0.addingTimeInterval(121)), "旧标题")

        // Plan filename sanitization & Windows reserved stem
        XCTAssertEqual(FileLinksAndPlanExport.planFilename(markdown: "# CON\nBody", fallbackTitle: ""), "_CON.md")
        XCTAssertEqual(
            FileLinksAndPlanExport.fileLinkTarget("src/main.rs:42:10", cwd: "/Users/example/repo"),
            FileLinkTarget(path: "/Users/example/repo/src/main.rs", resolved: true)
        )
    }
}
