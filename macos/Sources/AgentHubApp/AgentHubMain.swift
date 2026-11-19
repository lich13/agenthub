import AgentHubCore
import AppKit

@main
@MainActor
struct AgentHubNativeMain {
    static func main() {
        let app = NSApplication.shared
        let delegate = AgentHubAppDelegate()
        app.delegate = delegate
        withExtendedLifetime(delegate) {
            app.run()
        }
    }
}
