import AppKit
import SwiftUI

// MARK: - Session Search Floating Trigger & ⌘F Modal (SessionSearch.tsx)

public enum SessionSearchMatchKind {
    public static func label(for kind: String) -> String {
        switch kind {
        case "session", "thread_info": "线程信息"
        case "user", "user_message": "用户消息"
        case "assistant", "assistant_message": "助手回复"
        case "thinking", "reasoning": "思考"
        case "plan": "计划"
        case "tool", "tool_call": "工具"
        case "tool_result": "工具结果"
        case "command": "命令"
        default: kind
        }
    }
}

public struct FloatingSearchPillButton: View {
    @Environment(\.agentHubPalette) private var palette
    public let action: () -> Void

    public init(action: @escaping () -> Void) {
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                LucideIcon(.search, size: 14)
                Text("搜索 ⌘F")
                    .font(DesignFonts.body(12, weight: .medium))
            }
            .foregroundStyle(palette.textSecondary.color)
            .padding(.horizontal, 12)
            .frame(height: 32)
            .background(
                Capsule()
                    .fill(palette.elevated.color)
            )
            .overlay(
                Capsule()
                    .strokeBorder(palette.borderStrong.color, lineWidth: 1)
            )
            .shadow(color: Color.black.opacity(0.12), radius: 6, x: 0, y: 2)
            .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
        .accessibilityLabel("搜索会话")
    }
}

public struct SessionSearchModalView: View {
    @Environment(\.agentHubPalette) private var palette
    @ObservedObject public var store: RuntimeConnectionStore
    public let provider: SessionProvider
    public let currentSessionKey: String?
    public let onSelectResult: (SessionSearchResult) -> Void
    public let onClose: () -> Void

    @State private var query: String = ""
    @State private var scope: SearchScope
    @State private var results: [SessionSearchResult] = []
    @State private var nextCursor: String?
    @State private var truncated: Bool = false
    @State private var warnings: [String] = []
    @State private var loading: Bool = false
    @State private var errorText: String?
    @State private var selectedIndex: Int = 0
    @State private var searchTask: Task<Void, Never>?

    public init(
        store: RuntimeConnectionStore,
        provider: SessionProvider,
        currentSessionKey: String?,
        onSelectResult: @escaping (SessionSearchResult) -> Void,
        onClose: @escaping () -> Void
    ) {
        self.store = store
        self.provider = provider
        self.currentSessionKey = currentSessionKey
        self.onSelectResult = onSelectResult
        self.onClose = onClose
        self._scope = State(initialValue: currentSessionKey != nil ? .thread : .provider)
    }

    public var body: some View {
        GeometryReader { geo in
            let modalWidth = min(680, max(300, geo.size.width - 32))
            let modalHeight = min(720, max(280, geo.size.height - 80))

            ZStack(alignment: .top) {
                Color.black.opacity(0.46)
                    .ignoresSafeArea()
                    .onTapGesture(perform: onClose)

                VStack(alignment: .leading, spacing: 12) {
                    // 38pt Capsule Search Input + Close
                    HStack(spacing: 10) {
                        HStack(spacing: 8) {
                            LucideIcon(.search, size: 16)
                                .foregroundStyle(palette.textMuted.color)
                            IMEGuardedInlineTextField(
                                text: $query,
                                maxLength: 500,
                                placeholder: scope == .thread ? "搜索当前线程内容…" : "搜索当前 Provider 全部会话…",
                                onSubmit: { confirmCurrentSelection() },
                                onCancel: { onClose() }
                            )
                            if !query.isEmpty {
                                Button {
                                    query = ""
                                } label: {
                                    LucideIcon(.x, size: 14)
                                }
                                .buttonStyle(IconButtonStyle(dimension: 24))
                            }
                        }
                        .padding(.horizontal, 14)
                        .frame(height: 38)
                        .background(Capsule().fill(palette.surface.color))
                        .overlay(Capsule().strokeBorder(palette.borderStrong.color, lineWidth: 1))

                        Button(action: onClose) {
                            LucideIcon(.x, size: 17)
                        }
                        .buttonStyle(IconButtonStyle(dimension: 34))
                        .accessibilityLabel("关闭搜索")
                    }

                    // Scope Switcher
                    HStack(spacing: 8) {
                        scopeButton(.thread, label: "当前线程", disabled: currentSessionKey == nil)
                        scopeButton(.provider, label: "当前 Provider", disabled: false)
                        Spacer()
                        if loading {
                            Text("正在搜索…")
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.textMuted.color)
                        } else if !results.isEmpty {
                            Text("\(results.count) 条结果\(truncated ? "（已截断）" : "")")
                                .font(DesignFonts.body(12))
                                .foregroundStyle(palette.textMuted.color)
                        }
                    }

                    if let errorText {
                        Text(errorText)
                            .font(DesignFonts.body(12))
                            .foregroundStyle(palette.errFg.color)
                    }

                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 6) {
                            if !loading && !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && results.isEmpty && errorText == nil {
                                Text("未找到匹配内容")
                                    .font(DesignFonts.body(13))
                                    .foregroundStyle(palette.textMuted.color)
                                    .padding(.vertical, 20)
                            }
                            ForEach(Array(results.enumerated()), id: \.element.resultId) { index, item in
                                let active = (index == selectedIndex)
                                Button {
                                    selectedIndex = index
                                    onSelectResult(item)
                                    onClose()
                                } label: {
                                    VStack(alignment: .leading, spacing: 4) {
                                        HStack(spacing: 6) {
                                            StatusChip(SessionSearchMatchKind.label(for: item.matchKind), tone: .info)
                                            Text(item.title)
                                                .font(DesignFonts.body(13, weight: .semibold))
                                                .foregroundStyle(palette.textPrimary.color)
                                                .lineLimit(1)
                                            Spacer()
                                        }
                                        Text(item.snippet)
                                            .font(DesignFonts.body(12))
                                            .foregroundStyle(palette.textSecondary.color)
                                            .lineLimit(2)
                                    }
                                    .padding(10)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(
                                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                                            .fill((active ? palette.selected : palette.surface).color)
                                    )
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(PlainHitButtonStyle())
                            }

                            if nextCursor != nil && results.count < 200 {
                                Button("加载更多") {
                                    Task { await loadMore() }
                                }
                                .buttonStyle(SecondaryButtonStyle())
                                .disabled(loading)
                                .padding(.top, 4)
                            }
                        }
                    }

                    // Footer Navigation Controls
                    HStack(spacing: 8) {
                        Button {
                            if !results.isEmpty {
                                selectedIndex = max(0, selectedIndex - 1)
                            }
                        } label: {
                            LucideIcon(.arrowUp, size: 15)
                        }
                        .buttonStyle(IconButtonStyle(dimension: 30))
                        .disabled(results.isEmpty)

                        Button {
                            if !results.isEmpty {
                                selectedIndex = min(results.count - 1, selectedIndex + 1)
                            }
                        } label: {
                            LucideIcon(.arrowDown, size: 15)
                        }
                        .buttonStyle(IconButtonStyle(dimension: 30))
                        .disabled(results.isEmpty)

                        Text("↑↓ 切换 · Enter 跳转 · Esc 关闭")
                            .font(DesignFonts.body(11))
                            .foregroundStyle(palette.textMuted.color)
                        Spacer()
                    }
                }
                .padding(18)
                .frame(width: modalWidth, height: modalHeight)
                .background(
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .fill(palette.elevated.color)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .strokeBorder(palette.borderStrong.color, lineWidth: 1)
                )
                .shadow(color: Color.black.opacity(0.24), radius: 16, x: 0, y: 6)
                .padding(.top, max(24, geo.size.height * 0.12))
            }
        }
        .onChange(of: query) { _ in scheduleDebouncedSearch() }
        .onChange(of: scope) { _ in scheduleDebouncedSearch() }
    }

    private func scopeButton(_ target: SearchScope, label: String, disabled: Bool) -> some View {
        let active = (scope == target)
        return Button {
            scope = target
        } label: {
            Text(label)
                .font(DesignFonts.body(12, weight: active ? .semibold : .regular))
                .foregroundStyle((active ? palette.accent : palette.textSecondary).color)
                .padding(.horizontal, 10)
                .frame(height: 28)
                .background(
                    Capsule().fill((active ? palette.accentSoft : palette.surface).color)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(PlainHitButtonStyle())
        .disabled(disabled)
    }

    private func confirmCurrentSelection() {
        guard results.indices.contains(selectedIndex) else { return }
        onSelectResult(results[selectedIndex])
        onClose()
    }

    private func scheduleDebouncedSearch() {
        searchTask?.cancel()
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            results = []
            nextCursor = nil
            truncated = false
            errorText = nil
            loading = false
            return
        }
        searchTask = Task { @MainActor in
            try? await Task.sleep(nanoseconds: 200_000_000)
            guard !Task.isCancelled else { return }
            await runSearch(trimmed: trimmed, cursor: nil, append: false)
        }
    }

    private func loadMore() async {
        guard let cursor = nextCursor, results.count < 200 else { return }
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        await runSearch(trimmed: trimmed, cursor: cursor, append: true)
    }

    private func runSearch(trimmed: String, cursor: String?, append: Bool) async {
        loading = true
        errorText = nil
        defer { loading = false }
        struct ReqWrap: Encodable { let request: SessionSearchRequest }
        let req = SessionSearchRequest(
            provider: provider,
            scope: scope,
            sessionKey: scope == .thread ? currentSessionKey : nil,
            query: trimmed,
            cursor: cursor,
            limit: min(50, max(1, 200 - (append ? results.count : 0)))
        )
        do {
            let resp: SessionSearchResponse = try await store.rpc("sessions.search", args: ReqWrap(request: req))
            if append {
                let existingIds = Set(results.map(\.resultId))
                let added = resp.results.filter { !existingIds.contains($0.resultId) }
                results = Array((results + added).prefix(200))
            } else {
                results = Array(resp.results.prefix(200))
                selectedIndex = 0
            }
            nextCursor = results.count >= 200 ? nil : resp.nextCursor
            truncated = resp.truncated || results.count >= 200
            warnings = resp.warnings ?? []
        } catch {
            errorText = error.localizedDescription
        }
    }
}
