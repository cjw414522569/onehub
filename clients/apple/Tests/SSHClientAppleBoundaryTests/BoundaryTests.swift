import XCTest
@testable import SSHClientAppleBoundary

final class BoundaryTests: XCTestCase {
    func testApprovedBridgeAndBatchSemantics() {
        XCTAssertEqual(SSHClientAppleBoundary.approvedBridge, "bindings-swift")
        XCTAssertEqual(SSHClientAppleBoundary.status, "interface-only")
        XCTAssertEqual(SSHClientAppleBoundary.messageSemantics, "versioned-batch")
    }
}
