@testable import AgentHubCore
import Foundation
import XCTest

final class LegacyMigrationAndNotificationTests: XCTestCase {
    func testLegacyWebKitUTF16LEMigrationReadsWithoutMutatingFiles() throws {
        let tempHome = FileManager.default.temporaryDirectory
            .appendingPathComponent("agenthub-webkit-test-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: tempHome) }

        let sqliteDir = tempHome
            .appendingPathComponent("Library/WebKit/com.lich13.agenthub/WebsiteData/Default/origin1/origin2/LocalStorage")
        try FileManager.default.createDirectory(at: sqliteDir, withIntermediateDirectories: true)
        let dbURL = sqliteDir.appendingPathComponent("localstorage.sqlite3")

        // Build UTF-16LE hex literals for "dark" and "1" via python3 sqlite3 module (stdlib).
        let script = """
        import sqlite3, sys
        conn = sqlite3.connect(sys.argv[1])
        conn.execute('CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL)')
        conn.execute('INSERT INTO ItemTable VALUES (?, ?)', ('agenthub.theme', 'dark'.encode('utf-16-le')))
        conn.execute('INSERT INTO ItemTable VALUES (?, ?)', ('agenthub.nav-collapsed', '1'.encode('utf-16-le')))
        conn.commit()
        conn.close()
        """
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        proc.arguments = ["-c", script, dbURL.path]
        try proc.run()
        proc.waitUntilExit()
        XCTAssertEqual(proc.terminationStatus, 0)

        let bytesBefore = try Data(contentsOf: dbURL)

        let suiteName = "com.lich13.agenthub.tests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let migrated = LegacyPrefsMigrator.migrateIfNeeded(defaults: defaults, homeDirectory: tempHome.path)
        XCTAssertEqual(migrated?.theme, "dark")
        XCTAssertEqual(migrated?.navCollapsed, true)
        XCTAssertEqual(defaults.string(forKey: LegacyPrefsMigrator.themeDefaultsKey), "dark")
        XCTAssertEqual(defaults.string(forKey: LegacyPrefsMigrator.navCollapsedDefaultsKey), "1")

        // Second call is a no-op once migration flag is recorded.
        XCTAssertNil(LegacyPrefsMigrator.migrateIfNeeded(defaults: defaults, homeDirectory: tempHome.path))
        XCTAssertEqual(try Data(contentsOf: dbURL), bytesBefore)
    }

    @MainActor
    func testCiNotificationClickIdentifierExtractsHTTPSRunURL() {
        let id = "https://github.com/lich13/agenthub/actions/runs/12345#agenthub-ci:repo:12345:1"
        XCTAssertEqual(
            CiNotificationCenterBridge.extractClickTargetURL(from: id)?.absoluteString,
            "https://github.com/lich13/agenthub/actions/runs/12345"
        )
        XCTAssertNil(CiNotificationCenterBridge.extractClickTargetURL(from: "file:///etc/passwd#agenthub-ci:1"))
        XCTAssertNil(CiNotificationCenterBridge.extractClickTargetURL(from: "https://github.com/no-marker"))
    }

    func testFinderRevealPathValidationRejectsRelativeAndControlChars() {
        XCTAssertNoThrow(try FileOpenerService.validateRevealPath("/tmp/fixture/session.jsonl"))
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("relative/path.jsonl"))
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath("/tmp/bad\npath"))
        XCTAssertThrowsError(try FileOpenerService.validateRevealPath(""))
    }
}
