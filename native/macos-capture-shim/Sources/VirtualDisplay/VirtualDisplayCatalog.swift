// 가상 디스플레이 카탈로그 통합: 같은 dylib에 링크된 ObjC 브리지
// (CGVirtualDisplayBridge.m)가 유지하는 라이브 가상 디스플레이를
// CoreGraphics 열거 결과에 반영한다.
//
// 심볼은 dlsym으로 늦게 찾는다. 단일 파일 테스트 빌드처럼 브리지 오브젝트가
// 링크되지 않은 변형에서는 조용히 빈 목록을 돌려주며, 열거·캡처 나머지
// 경로는 전혀 영향을 받지 않는다(격리 요구사항).

import Foundation

struct VirtualDisplayOverride {
    let sourceId: String
    let name: String
    let logicalWidth: UInt32
    let logicalHeight: UInt32
    let scale: UInt32

    var backingWidth: UInt32 { logicalWidth * scale }
    var backingHeight: UInt32 { logicalHeight * scale }
}

private func vdispBridgeSymbol(_ name: String) -> UnsafeMutableRawPointer? {
    guard let handle = dlopen(nil, RTLD_LAZY) else { return nil }
    return dlsym(handle, name)
}

/// 브리지가 보고한 현재 가상 디스플레이 오버라이드. 심볼 부재·파싱 실패는
/// 빈 목록으로 degrade한다.
func currentVirtualDisplayOverrides() -> [VirtualDisplayOverride] {
    typealias OverridesFn = @convention(c) () -> UnsafeMutablePointer<CChar>?
    guard let symbol = vdispBridgeSymbol("leftcar_vdisp_catalog_overrides_v1"),
          let raw = unsafeBitCast(symbol, to: OverridesFn.self)() else {
        return []
    }
    defer { free(raw) }
    guard let data = String(cString: raw).data(using: .utf8),
          let entries = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
        return []
    }
    return entries.compactMap { entry in
        guard let uuid = entry["uuid"] as? String, !uuid.isEmpty,
              let name = entry["name"] as? String,
              let logicalWidth = (entry["logicalWidth"] as? NSNumber)?.uint32Value,
              let logicalHeight = (entry["logicalHeight"] as? NSNumber)?.uint32Value,
              let scale = (entry["scale"] as? NSNumber)?.uint32Value else {
            return nil
        }
        return VirtualDisplayOverride(
            sourceId: "macos:display:\(uuid)",
            name: name,
            logicalWidth: logicalWidth,
            logicalHeight: logicalHeight,
            scale: scale
        )
    }
}

/// sourceId(macos:display:<uuid>)로 가상 디스플레이 오버라이드를 찾는다.
func virtualDisplayOverride(forSourceId sourceId: String) -> VirtualDisplayOverride? {
    currentVirtualDisplayOverrides().first { $0.sourceId == sourceId }
}
