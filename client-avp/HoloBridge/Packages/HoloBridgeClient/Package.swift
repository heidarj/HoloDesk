// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "HoloBridgeClient",
    platforms: [
        .macOS(.v12),
        .visionOS(.v1),
    ],
    products: [
        .library(
            name: "HoloBridgeClientCore",
            targets: ["HoloBridgeClientCore"]
        ),
        .library(
            name: "HoloBridgeClientTestAuth",
            targets: ["HoloBridgeClientTestAuth"]
        ),
        .executable(
            name: "holobridge-client-smoke",
            targets: ["holobridge-client-smoke"]
        ),
    ],
    targets: [
        .target(
            name: "HoloBridgeClientCore"
        ),
        .target(
            name: "HoloBridgeClientTestAuth",
            dependencies: ["HoloBridgeClientCore"]
        ),
        .executableTarget(
            name: "holobridge-client-smoke",
            dependencies: [
                "HoloBridgeClientCore",
                "HoloBridgeClientTestAuth",
            ]
        ),
        .testTarget(
            name: "HoloBridgeClientCoreTests",
            dependencies: ["HoloBridgeClientCore"]
        ),
    ]
)
