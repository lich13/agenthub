import AppKit
import CAgentHubFFI
import Foundation
@preconcurrency import UserNotifications

// MARK: - Legacy WebKit localStorage (UTF-16LE SQLite) One-Shot Migration

public struct LegacyPreferencesSnapshot: Codable, Equatable, Sendable {
    public var theme: String?
    public var navCollapsed: Bool?
}

public enum LegacyPrefsMigrator {
    public static let migrationFlagKey = "agenthub.native.migratedLegacyWebviewPrefs"
    public static let themeDefaultsKey = "agenthub.theme"
    public static let navCollapsedDefaultsKey = "agenthub.nav-collapsed"

    public static func readFromDisk(homeDirectory: String? = nil) -> LegacyPreferencesSnapshot {
        let ptr: UnsafeMutablePointer<CChar>? = if let homeDirectory {
            homeDirectory.withCString { agenthub_ffi_read_legacy_webview_prefs($0) }
        } else {
            agenthub_ffi_read_legacy_webview_prefs(nil)
        }
        guard let ptr else { return LegacyPreferencesSnapshot() }
        defer { agenthub_ffi_string_free(ptr) }
        let json = Data(String(cString: ptr).utf8)
        return (try? JSONDecoder().decode(LegacyPreferencesSnapshot.self, from: json))
            ?? LegacyPreferencesSnapshot()
    }

    @discardableResult
    public static func migrateIfNeeded(
        defaults: UserDefaults = .standard,
        homeDirectory: String? = nil
    ) -> LegacyPreferencesSnapshot? {
        guard !defaults.bool(forKey: migrationFlagKey) else {
            return nil
        }
        let legacy = readFromDisk(homeDirectory: homeDirectory)
        if defaults.string(forKey: themeDefaultsKey) == nil, let theme = legacy.theme {
            defaults.set(ThemeMode.normalize(theme).rawValue, forKey: themeDefaultsKey)
        }
        if defaults.object(forKey: navCollapsedDefaultsKey) == nil, let collapsed = legacy.navCollapsed {
            defaults.set(collapsed ? "1" : "0", forKey: navCollapsedDefaultsKey)
        }
        defaults.set(true, forKey: migrationFlagKey)
        return legacy
    }
}

// MARK: - Native macOS UNUserNotificationCenter CI Delivery & Click Routing

public struct CiNotificationItem: Codable, Equatable, Sendable {
    public var identifier: String
    public var title: String
    public var body: String
    public var htmlUrl: String
}

@MainActor
public final class CiNotificationCenterBridge: NSObject, UNUserNotificationCenterDelegate {
    public static let shared = CiNotificationCenterBridge()
    public var openUrlHandler: @MainActor (URL) -> Void = { url in
        NSWorkspace.shared.open(url)
    }

    private var installed = false

    public func installIfAvailable() {
        guard !installed else { return }
        // UNUserNotificationCenter.current() requires a valid bundled app context on macOS.
        guard Bundle.main.bundleIdentifier != nil,
              Bundle.main.bundlePath.hasSuffix(".app")
        else { return }
        installed = true
        UNUserNotificationCenter.current().delegate = self
    }

    nonisolated public static func extractClickTargetURL(from identifier: String) -> URL? {
        guard let range = identifier.range(of: "#agenthub-ci:", options: .backwards) else {
            return nil
        }
        let rawUrl = String(identifier[..<range.lowerBound])
        guard let url = URL(string: rawUrl),
              let scheme = url.scheme?.lowercased(),
              scheme == "https" || scheme == "http"
        else {
            return nil
        }
        return url
    }

    public func deliverAuthorized(eventsJSON: Data) {
        guard let items = try? JSONDecoder().decode([CiNotificationItem].self, from: eventsJSON),
              !items.isEmpty,
              Bundle.main.bundleIdentifier != nil,
              Bundle.main.bundlePath.hasSuffix(".app")
        else { return }

        installIfAvailable()
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound]) { granted, error in
            guard error == nil, granted else { return }
            for item in items {
                let content = UNMutableNotificationContent()
                content.title = item.title
                content.body = item.body
                content.sound = .default
                let request = UNNotificationRequest(
                    identifier: item.identifier,
                    content: content,
                    trigger: nil
                )
                center.add(request) { _ in }
            }
        }
    }

    nonisolated public func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated public func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        if response.actionIdentifier == UNNotificationDefaultActionIdentifier {
            let identifier = response.notification.request.identifier
            if let url = Self.extractClickTargetURL(from: identifier) {
                Task { @MainActor in
                    CiNotificationCenterBridge.shared.openUrlHandler(url)
                }
            }
        }
        completionHandler()
    }
}
