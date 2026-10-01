// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "Clarity",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/swiftlang/swift-markdown.git", from: "0.5.0"),
    ],
    targets: [
        .executableTarget(
            name: "Clarity",
            dependencies: [.product(name: "Markdown", package: "swift-markdown")],
            path: "Sources/Clarity"
        ),
    ]
)
