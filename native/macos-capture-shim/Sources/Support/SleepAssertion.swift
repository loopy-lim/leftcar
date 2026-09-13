import Foundation
import IOKit
import IOKit.pwr_mgt

/// 세션 생존 중 시스템·디스플레이 절전을 막는 전원 어설션 묶음.
/// 스트리밍 중 노트북이 잠들면 캡처·오디오가 끊긴다 — 캡처 세션 수만큼
/// 들고 있다가 deinit에서 놓는다(여러 세션이 각자 어설션을 잡는다).
struct SleepAssertion {
    private let ids: [IOPMAssertionID]

    /// 시스템+디스플레이 두 종류를 함께 잡는다 — 디스플레이만 막으면 다른
    /// 유휴 사유로 시스템이 잠들 수 있고, 반대도 마찬가지다.
    static func streamingSession() -> SleepAssertion? {
        let types = [
            kIOPMAssertionTypePreventUserIdleSystemSleep,
            kIOPMAssertionTypePreventUserIdleDisplaySleep,
        ]
        var ids: [IOPMAssertionID] = []
        for type in types {
            var id: IOPMAssertionID = 0
            let result = IOPMAssertionCreateWithName(
                type as CFString,
                IOPMAssertionLevel(kIOPMAssertionLevelOn),
                "leftcar streaming session" as CFString,
                &id
            )
            if result == kIOReturnSuccess {
                ids.append(id)
            }
        }
        return ids.isEmpty ? nil : SleepAssertion(ids: ids)
    }

    func release() {
        for id in ids {
            IOPMAssertionRelease(id)
        }
    }
}
