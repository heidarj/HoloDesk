// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "HoloBridgeClient",
    platforms: [
        .macOS(.v26),
        .visionOS(.v26),
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
        .executable(
            name: "holobridge-quic-interop-smoke",
            targets: ["holobridge-quic-interop-smoke"]
        ),
    ],
    targets: [
        .target(
            name: "HoloBridgeClientQuicBridge",
            path: "Sources/HoloBridgeClientQuicBridge",
            publicHeadersPath: "include",
            linkerSettings: [
                .linkedFramework("Foundation"),
                .linkedFramework("Network"),
                .linkedFramework("Security"),
            ]
        ),
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
        .executableTarget(
            name: "holobridge-quic-interop-smoke",
            dependencies: ["HoloBridgeClientQuicBridge"]
        ),
        .testTarget(
            name: "HoloBridgeClientCoreTests",
            dependencies: ["HoloBridgeClientCore"]
        ),
    ]
)
