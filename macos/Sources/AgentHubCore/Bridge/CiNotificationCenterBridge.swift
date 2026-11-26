import AppKit
import Foundation
@preconcurrency import UserNotifications

// MARK: - Native macOS UNUserNotificationCenter CI Delivery & Click Routing

struct CiNotificationItem: Codable, Equatable, Sendable {
    var identifier: String
    var title: String
    var body: String
    var htmlUrl: String
}

protocol UserNotificationCenterScheduling: AnyObject, Sendable {
    func requestAuthorization(
        options: UNAuthorizationOptions,
        completionHandler: @escaping @Sendable (Bool, (any Error)?) -> Void
    )
    func add(
        _ request: UNNotificationRequest,
        withCompletionHandler completionHandler: (@Sendable ((any Error)?) -> Void)?
    )
}

private final class SystemUserNotificationCenter: UserNotificationCenterScheduling {
    func requestAuthorization(
        options: UNAuthorizationOptions,
        completionHandler: @escaping @Sendable (Bool, (any Error)?) -> Void
    ) {
        UNUserNotificationCenter.current().requestAuthorization(
            options: options,
            completionHandler: completionHandler
        )
    }

    func add(
        _ request: UNNotificationRequest,
        withCompletionHandler completionHandler: (@Sendable ((any Error)?) -> Void)?
    ) {
        UNUserNotificationCenter.current().add(
            request,
            withCompletionHandler: completionHandler
        )
    }
}

@MainActor
final class CiNotificationCenterBridge: NSObject, UNUserNotificationCenterDelegate {
    static let shared = CiNotificationCenterBridge()
    var openUrlHandler: @MainActor (URL) -> Void = { url in
        NSWorkspace.shared.open(url)
    }
    var notificationCenterOverride: (any UserNotificationCenterScheduling)?

    private var installed = false
    private(set) var lastDecodeError: String?

    func installIfAvailable() {
        guard !installed else { return }
        // UNUserNotificationCenter.current() requires a valid bundled app context on macOS.
        guard Bundle.main.bundleIdentifier != nil,
              Bundle.main.bundlePath.hasSuffix(".app")
        else { return }
        installed = true
        UNUserNotificationCenter.current().delegate = self
    }

    nonisolated static func extractClickTargetURL(from identifier: String) -> URL? {
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

    func deliverAuthorized(eventsJSON: Data) {
        let items: [CiNotificationItem]
        do {
            items = try JSONDecoder().decode([CiNotificationItem].self, from: eventsJSON)
            lastDecodeError = nil
        } catch {
            lastDecodeError = "CI 通知事件解码失败"
            return
        }
        guard !items.isEmpty else { return }

        let scheduler: any UserNotificationCenterScheduling
        if let notificationCenterOverride {
            scheduler = notificationCenterOverride
        } else {
            guard Bundle.main.bundleIdentifier != nil,
                  Bundle.main.bundlePath.hasSuffix(".app")
            else { return }
            installIfAvailable()
            scheduler = SystemUserNotificationCenter()
        }
        Self.scheduleAuthorizedNotifications(items, center: scheduler)
    }

    // Constructed in a `nonisolated` scope with explicit `@Sendable` callbacks so Swift 6 SE-0423
    // never injects a `@MainActor` runtime assertion when `UNUserNotificationCenter` invokes blocks
    // on `com.apple.usernotifications.UNUserNotificationServiceConnection.call-out`.
    nonisolated private static func scheduleAuthorizedNotifications(
        _ items: [CiNotificationItem],
        center: any UserNotificationCenterScheduling
    ) {
        let onAdded: @Sendable ((any Error)?) -> Void = { _ in }
        let onAuthorized: @Sendable (Bool, (any Error)?) -> Void = { granted, error in
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
                center.add(request, withCompletionHandler: onAdded)
            }
        }
        center.requestAuthorization(options: [.alert, .sound], completionHandler: onAuthorized)
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(
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
