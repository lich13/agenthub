@testable import AgentHubCore
import Foundation
import XCTest

final class RuntimeLeaseTests: XCTestCase {
    func testShutdownWaitsForBorrowedHandleAndRejectsLaterCalls() throws {
        let context = HostBridgeContext()
        let token = UnsafeMutableRawPointer.allocate(byteCount: 1, alignment: 1)
        defer { token.deallocate() }
        context.bindRuntime(OpaquePointer(token))
        let borrowed = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
        let closed = DispatchSemaphore(value: 0)
        let address = UInt(bitPattern: token)
        DispatchQueue.global().async {
            do {
                try context.withRuntime { ptr, _ in
                    XCTAssertEqual(UInt(bitPattern: ptr), address)
                    borrowed.signal()
                    _ = release.wait(timeout: .now() + 5)
                }
            } catch { XCTFail(error.localizedDescription) }
        }
        XCTAssertEqual(borrowed.wait(timeout: .now() + 5), .success)
        DispatchQueue.global().async {
            XCTAssertEqual(context.takeRuntime().map(UInt.init(bitPattern:)), address)
            closed.signal()
        }
        let deadline = Date().addingTimeInterval(3)
        while (try? context.withRuntime { _, _ in true }) == true, Date() < deadline {
            Thread.sleep(forTimeInterval: 0.001)
        }
        XCTAssertThrowsError(try context.withRuntime { _, _ in })
        XCTAssertEqual(closed.wait(timeout: .now()), .timedOut)
        release.signal()
        XCTAssertEqual(closed.wait(timeout: .now() + 5), .success)
        XCTAssertNil(context.takeRuntime())
    }
}
