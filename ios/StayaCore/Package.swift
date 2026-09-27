// swift-tools-version: 6.0
// Rust-ядро Staya для iOS. XCFramework и привязки генерирует scripts/build-ios-core.sh.
import PackageDescription

let package = Package(
    name: "StayaCore",
    platforms: [.iOS(.v18)],
    products: [
        .library(name: "StayaCore", targets: ["StayaCore"]),
    ],
    targets: [
        .binaryTarget(name: "StayaCoreFFI", path: "StayaCoreFFI.xcframework"),
        .target(name: "StayaCore", dependencies: ["StayaCoreFFI"]),
    ]
)
