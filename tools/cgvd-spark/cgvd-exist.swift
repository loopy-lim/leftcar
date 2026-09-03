import Foundation
import ObjectiveC

// 세션과 무관한 실측: private 클래스 존재 + 메서드 표면 (macOS 26.6.2)
func dump(_ name: String) -> Bool {
    guard let cls = NSClassFromString(name) else { print("MISSING: \(name)"); return false }
    var count: UInt32 = 0
    let methods = class_copyMethodList(cls, &count)
    print("EXISTS: \(name) — instance methods: \(count)")
    if methods != nil { free(methods) }
    return true
}

print("== CGVD runtime existence (session-independent) ==")
print("OS: \(ProcessInfo.processInfo.operatingSystemVersionString)")
var ok = true
for n in ["CGVirtualDisplay", "CGVirtualDisplayDescriptor", "CGVirtualDisplaySettings", "CGVirtualDisplayMode"] {
    ok = dump(n) && ok
}
print(ok ? "== RESULT: private API 4종 모두 macOS 26.6.2에 존재 ==" : "== RESULT: 일부 클래스 부재 ==")
