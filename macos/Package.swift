// swift-tools-version: 6.0
import Foundation
import PackageDescription

let env = ProcessInfo.processInfo.environment
let ffiLinkerFlags: [String] = {
    if let explicit = env["AGENTHUB_FFI_LIB_DIR"], !explicit.isEmpty {
        return ["-L\(explicit)", "-lagenthub_ffi"]
    }
    let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
    let repoRoot = URL(fileURLWithPath: packageDir).deletingLastPathComponent().path
    return [
        "-L\(repoRoot)/target/aarch64-apple-darwin/release",
        "-L\(repoRoot)/target/release",
        "-L\(repoRoot)/target/debug",
        "-lagenthub_ffi",
    ]
}()

let package = Package(
    name: "AgentHubMacOS",
    platforms: [
        .macOS(.v12),
    ],
    products: [
        .library(name: "AgentHubCore", targets: ["AgentHubCore"]),
        .executable(name: "agenthub", targets: ["agenthub"]),
    ],
    targets: [
        .systemLibrary(
            name: "CAgentHubFFI",
            path: "Sources/CAgentHubFFI"
        ),
        .target(
            name: "AgentHubCore",
            dependencies: ["CAgentHubFFI"],
            path: "Sources/AgentHubCore",
            linkerSettings: [
                .unsafeFlags(ffiLinkerFlags),
                .linkedFramework("AppKit"),
                .linkedFramework("Security"),
                .linkedFramework("UserNotifications"),
                .linkedFramework("QuartzCore"),
            ]
        ),
        .executableTarget(
            name: "agenthub",
            dependencies: ["AgentHubCore"],
            path: "Sources/AgentHubApp"
        ),
        .testTarget(
            name: "AgentHubTests",
            dependencies: ["AgentHubCore", "CAgentHubFFI"],
            path: "Tests/AgentHubTests",
            resources: [
                .copy("SnapshotBaselines.json"),
            ]
        ),
    ],
    swiftLanguageModes: [.v6]
)
