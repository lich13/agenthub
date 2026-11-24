import AppKit
import Foundation
@preconcurrency import UserNotifications

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
    public private(set) var lastDecodeError: String?

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
        let items: [CiNotificationItem]
        do {
            items = try JSONDecoder().decode([CiNotificationItem].self, from: eventsJSON)
            lastDecodeError = nil
        } catch {
            lastDecodeError = "CI 通知事件解码失败"
            return
        }
        guard !items.isEmpty,
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
