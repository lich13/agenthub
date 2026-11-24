@testable import AgentHubCore
import Foundation
import XCTest

final class GithubCiAndPathActionTests: XCTestCase {
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
