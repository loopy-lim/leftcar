// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "cgvd-shim",
    targets: [
        .executableTarget(
            name: "cgvd-shim",
            // module.modulemap이 있는 헤더 경로를 importer에 전달한다 (README 구현 노트).
            cSettings: [.unsafeFlags(["-I", "Sources/cgvd-shim/include"])],
            // private 클래스 심볼은 macOS 버전에 따라 사라질 수 있다. 약한 바인딩으로
            // 묶어야 파손된 macOS에서도 바이너리가 로드돼 probe가 MISSING이라는 정상
            // 답을 낸다 (README 구현 노트).
            linkerSettings: [.unsafeFlags(["-Xlinker", "-weak_framework", "-Xlinker", "CoreGraphics"])]
        )
    ]
)
