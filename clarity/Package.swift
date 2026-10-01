// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "Clarity",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "Clarity", path: "Sources/Clarity"),
    ]
)
