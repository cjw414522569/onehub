// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SSHClientAppleBoundary",
    platforms: [
        .macOS(.v15),
        .iOS(.18),
    ],
    products: [
        .library(name: "SSHClientAppleBoundary", targets: ["SSHClientAppleBoundary"]),
    ],
    targets: [
        .target(name: "SSHClientAppleBoundary"),
        .testTarget(name: "SSHClientAppleBoundaryTests", dependencies: ["SSHClientAppleBoundary"]),
    ]
)
