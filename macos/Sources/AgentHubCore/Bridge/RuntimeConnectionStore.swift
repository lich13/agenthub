import AppKit
import Combine
import Foundation

@MainActor
public final class RuntimeConnectionStore: ObservableObject {
    public static let readCommands: Set<String> = [
        "cleanup.unifiedDryRun", "github.ci.status", "cleanup.hiddenDryRun",
        "system.capabilities", "threads.list", "threads.detail",
        "threads.subagentDetail", "threads.blocks", "threads.accountScopeStatus",
        "jobs.list", "jobs.detail", "probe.status", "probe.settings.get",
        "probe.events", "updates.status", "grokbot.list", "grokbot.detail",
        "grok.list", "grok.detail", "grok.deletePreview", "claude.list",
        "claude.detail", "claude.ownershipDiagnostics", "claude.deletePreview", "antigravity.list",
        "antigravity.detail", "dsh.list", "dsh.detail",
        "sessions.attachmentRead", "sessions.search", "sessions.bulkPreview",
        "codex.desktop.attach", "codex.desktop.catalog", "codex.desktop.diagnostics",
    ]

    public let transport: any RuntimeTransport
    private let defaults: UserDefaults

    @Published public private(set) var target: MachineTarget = .local
    @Published public private(set) var revision: UInt64 = 0
    @Published public private(set) var sshHost: String?
    @Published public private(set) var configured: Bool = false
    @Published public private(set) var activeMachineId: String?
    @Published public private(set) var activeMachineName: String?
    @Published public private(set) var machines: [RemoteMachineSummary] = []
    @Published public private(set) var ready: Bool = false
    @Published public private(set) var writes: Int = 0
    @Published public private(set) var changing: Bool = false
    @Published public var connectionError: String?
    @Published public var connectionNotice: String?

    @Published public private(set) var capabilities: RuntimeCapabilityMatrix = .bootstrap
    @Published public var capabilitiesError: String?

    @Published public var activeWorkspace: WorkspaceId = .codex
    @Published public var settingsSection: SettingsTabSection = .maintenance
    @Published public var navCollapsed: Bool
    @Published public var themeMode: ThemeMode
    @Published public private(set) var isWindowVisible: Bool = true
    @Published public private(set) var isWindowFocused: Bool = true
    @Published public private(set) var isInlineModalPresented: Bool = false
    private var explicitModalKeys: Set<String> = []

    public typealias BeforeMachineChangeCleanup = @Sendable @MainActor () async -> Void

    @Published public private(set) var trackedJobs: [String: JobRecord] = [:]
    private var trackedJobOrder: [String] = []
    private var jobPollingTask: Task<Void, Never>?
    private var readCleanups: [UUID: BeforeMachineChangeCleanup] = [:]

    public enum SettingsTabSection: String, Sendable {
        case maintenance
        case remote
    }

    public init(transport: any RuntimeTransport, defaults: UserDefaults = .standard) {
        self.transport = transport
        self.defaults = defaults
        LegacyPrefsMigrator.migrateIfNeeded(defaults: defaults)
        self.themeMode = ThemeMode.normalize(defaults.string(forKey: LegacyPrefsMigrator.themeDefaultsKey))
        self.navCollapsed = defaults.string(forKey: LegacyPrefsMigrator.navCollapsedDefaultsKey) == "1"
        applyWindowAppearance()
    }

    public var isBusyForSwitch: Bool {
        writes > 0 || changing
    }

    public var machineScope: String {
        guard target == .remote else { return "local:" }
        if let id = activeMachineId {
            return "remote:\(id):\(sshHost ?? "")"
        }
        return "remote:\(sshHost ?? "")"
    }

    public var machineDisplayLabel: String {
        target == .local ? "本机" : (activeMachineName ?? "远程机器")
    }

    public func toggleNavCollapsed() {
        navCollapsed.toggle()
        defaults.set(navCollapsed ? "1" : "0", forKey: LegacyPrefsMigrator.navCollapsedDefaultsKey)
    }

    public func setTheme(_ next: ThemeMode) {
        themeMode = next
        defaults.set(next.rawValue, forKey: LegacyPrefsMigrator.themeDefaultsKey)
        applyWindowAppearance()
    }

    public func applyWindowAppearance() {
        guard NSApp != nil else { return }
        switch themeMode {
        case .system:
            NSApp.appearance = nil
        case .dark:
            NSApp.appearance = NSAppearance(named: .darkAqua)
        case .light:
            NSApp.appearance = NSAppearance(named: .aqua)
        }
    }

    public func updateWindowState(visible: Bool, focused: Bool) {
        isWindowVisible = visible
        isWindowFocused = focused
        transport.setMainWindowState(visible: visible, focused: focused)
    }

    public func setInlineModalPresented(_ key: String, presented: Bool) {
        if presented {
            explicitModalKeys.insert(key)
        } else {
            explicitModalKeys.remove(key)
        }
        isInlineModalPresented = !explicitModalKeys.isEmpty
    }

    public var hasAnyActiveModal: Bool {
        isInlineModalPresented || ModalPresentationRegistry.shared.hasActiveModal
    }

    public func registerBeforeMachineChange(_ cleanup: @escaping BeforeMachineChangeCleanup) -> UUID {
        let id = UUID()
        readCleanups[id] = cleanup
        return id
    }

    public func unregisterBeforeMachineChange(_ id: UUID) {
        readCleanups.removeValue(forKey: id)
    }

    public func bootstrap() async {
        do {
            let view: RemoteConnectionView = try await invokeLocal("remote.get")
            applyConnectionView(view)
            ready = true
            connectionError = nil
        } catch {
            ready = true
            connectionError = error.localizedDescription
        }
        await refreshSystemCapabilities()
    }

    public func refreshSystemCapabilities() async {
        do {
            let response: SystemCapabilitiesResponse = try await rpc("system.capabilities")
            let next = RuntimeCapabilityMatrix.from(response)
            capabilities = next
            capabilitiesError = nil
            if activeWorkspace == .grokbot && !next.grokbotSessions {
                activeWorkspace = .codex
            }
        } catch {
            capabilitiesError = error.localizedDescription
        }
    }

    private func applyConnectionView(_ view: RemoteConnectionView) {
        target = view.target
        revision = view.revision
        sshHost = view.sshHost
        configured = view.configured
        activeMachineId = view.activeMachineId
        activeMachineName = view.activeMachineName
        machines = view.machines ?? []
    }

    // MARK: - Typed RPC & Machine Switching

    public func invokeLocal<Res: Decodable>(_ command: String, argsJSON: String = "{}") async throws -> Res {
        let data = try await transport.invokeRawJSON(command: command, argsJSON: argsJSON)
        return try JSONDecoder().decode(Res.self, from: data)
    }

    public func invokeLocal<Req: Encodable, Res: Decodable>(_ command: String, args: Req) async throws -> Res {
        let encoded = try JSONEncoder().encode(args)
        let argsJSON = String(data: encoded, encoding: .utf8) ?? "{}"
        return try await invokeLocal(command, argsJSON: argsJSON)
    }

    public func rpcRaw(_ command: String, argsObject: [String: JSONValue] = [:], localOnly: Bool = false) async throws -> Data {
        if changing {
            throw AgentHubFFIError("连接切换中，请稍后重试")
        }
        let capturedRevision = revision
        let capturedTarget = target
        let isReadHookStatus: Bool = {
            guard command == "claude.hooks",
                  case let .object(req)? = argsObject["request"],
                  case let .string(act)? = req["action"]
            else { return false }
            return act == "status"
        }()
        let isWrite = !Self.readCommands.contains(command) && !isReadHookStatus
        if isWrite { writes += 1 }
        defer { if isWrite { writes = max(0, writes - 1) } }

        let data: Data
        if capturedTarget == .remote && !localOnly {
            let invokePayload: [String: JSONValue] = [
                "request": .object([
                    "revision": .number(Double(capturedRevision)),
                    "command": .string(command),
                    "args": .object(argsObject),
                ]),
            ]
            let encoded = try JSONEncoder().encode(invokePayload)
            let jsonStr = String(data: encoded, encoding: .utf8) ?? "{}"
            data = try await transport.invokeRawJSON(command: "remote.invoke", argsJSON: jsonStr)
        } else {
            let encoded = try JSONEncoder().encode(argsObject)
            let jsonStr = String(data: encoded, encoding: .utf8) ?? "{}"
            data = try await transport.invokeRawJSON(command: command, argsJSON: jsonStr)
        }

        if capturedRevision != revision {
            throw AgentHubFFIError("连接已变化，已丢弃旧响应")
        }
        return data
    }

    public func rpc<Res: Decodable>(
        _ command: String,
        args: [String: JSONValue] = [:],
        localOnly: Bool = false
    ) async throws -> Res {
        let data = try await rpcRaw(command, argsObject: args, localOnly: localOnly)
        return try JSONDecoder().decode(Res.self, from: data)
    }

    public func rpc<Req: Encodable, Res: Decodable>(
        _ command: String,
        args: Req,
        localOnly: Bool = false
    ) async throws -> Res {
        let encoded = try JSONEncoder().encode(args)
        let argsObj = (try? JSONDecoder().decode([String: JSONValue].self, from: encoded)) ?? [:]
        let data = try await rpcRaw(command, argsObject: argsObj, localOnly: localOnly)
        return try JSONDecoder().decode(Res.self, from: data)
    }

    private func runBeforeMachineChangeCleanups(_ cleanups: [BeforeMachineChangeCleanup]) async {
        for fn in cleanups {
            await fn()
        }
    }

    private func changeConnection<Req: Encodable, Res: Decodable>(
        _ command: String,
        request: Req,
        updatesState: Bool = true
    ) async throws -> Res {
        if isBusyForSwitch {
            throw AgentHubFFIError("操作进行中，请稍后切换")
        }
        if updatesState && !readCleanups.isEmpty {
            let cleanups = Array(readCleanups.values)
            await withTaskGroup(of: Void.self) { group in
                group.addTask { [self] in
                    await self.runBeforeMachineChangeCleanups(cleanups)
                }
                group.addTask {
                    try? await Task.sleep(nanoseconds: 1_000_000_000)
                }
                _ = await group.next()
                group.cancelAll()
            }
            if isBusyForSwitch {
                throw AgentHubFFIError("操作进行中，请稍后切换")
            }
        }
        changing = true
        defer { changing = false }

        let res: Res = try await invokeLocal(command, args: RemoteRequestEnvelope(request: request))
        if updatesState, let view = res as? RemoteConnectionView {
            applyConnectionView(view)
            connectionError = nil
            connectionNotice = (command == "remote.save") ? "已保存" : nil
            trackedJobs.removeAll()
            trackedJobOrder.removeAll()
            await refreshSystemCapabilities()
        }
        return res
    }

    public func selectMachine(target nextTarget: MachineTarget, machineId: String? = nil) async throws {
        if nextTarget == .local && target == .local { return }
        struct SelectionReq: Encodable {
            let revision: UInt64
            let target: MachineTarget
            let machineId: String?
        }
        let _: RemoteConnectionView = try await changeConnection(
            "remote.select",
            request: SelectionReq(revision: revision, target: nextTarget, machineId: machineId)
        )
    }

    public func saveRemoteMachine(_ creds: RemoteConnectionCredentials) async throws -> RemoteConnectionView {
        try await changeConnection("remote.save", request: creds)
    }

    public func installRemoteMachine(_ creds: RemoteConnectionCredentials) async throws -> RemoteConnectionView {
        try await changeConnection("remote.install", request: creds)
    }

    public func removeRemoteMachine(machineId: String?) async throws -> RemoteConnectionView {
        struct RemoveReq: Encodable {
            let revision: UInt64
            let machineId: String?
        }
        return try await changeConnection("remote.remove", request: RemoveReq(revision: revision, machineId: machineId))
    }

    public func deleteSshHost(alias: String) async throws -> RemoteConnectionView {
        struct DeleteReq: Encodable {
            let revision: UInt64
            let alias: String
        }
        return try await changeConnection("remote.sshDelete", request: DeleteReq(revision: revision, alias: alias))
    }

    public func listSshHosts() async throws -> [String] {
        let rows: [SshHostItem] = try await invokeLocal("remote.sshHosts")
        return rows.map(\.alias)
    }

    // MARK: - Background Started-Job Tracking (continues at 2s even when window is hidden)

    public func trackStartedJob(_ jobId: String?) {
        guard let jobId, !jobId.isEmpty else { return }
        if !trackedJobOrder.contains(jobId) {
            trackedJobOrder.append(jobId)
        }
        startJobPollingIfNeeded()
    }

    private func startJobPollingIfNeeded() {
        guard jobPollingTask == nil else { return }
        jobPollingTask = Task { @MainActor [weak self] in
            while let self, !Task.isCancelled {
                let activeIds = self.trackedJobOrder.filter { id in
                    guard let existing = self.trackedJobs[id] else { return true }
                    return !existing.isTerminal
                }
                if activeIds.isEmpty {
                    self.jobPollingTask = nil
                    return
                }
                for id in activeIds {
                    if let envelope: OptionalResultEnvelope<JobRecord> = try? await self.rpc(
                        "jobs.detail",
                        args: ["id": .string(id), "includeOutput": .bool(false)]
                    ), let job = envelope.data {
                        self.trackedJobs[id] = job
                    }
                }
                try? await Task.sleep(nanoseconds: 2_000_000_000)
            }
        }
    }
}

private struct RemoteRequestEnvelope<T: Encodable>: Encodable {
    let request: T
}
