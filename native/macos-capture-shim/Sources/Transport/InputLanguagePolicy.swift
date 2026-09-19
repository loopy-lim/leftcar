import Foundation

struct InputLanguageSource {
    let id: String
    let languages: [String]
    let selectable: Bool
}

func inputLanguageSource(_ language: UInt8, current: String?, sources: [InputLanguageSource]) -> String? {
    let tag: String
    switch language {
    case 1: tag = "en"
    case 2: tag = "ko"
    default: return nil
    }
    let matches = sources.filter { source in
        source.selectable && source.languages.contains {
            $0.lowercased().split(whereSeparator: { $0 == "-" || $0 == "_" }).first == Substring(tag)
        }
    }
    // Preserve the user's current layout when it already has the right language.
    if let current, matches.contains(where: { $0.id == current }) { return current }
    return matches.first?.id
}

/// 입력 소스 언어 목록의 대표 서브태그("ko-KR" → "ko"). 판정만 하고 선택은 하지 않는다.
func primaryLanguageTag(_ languages: [String]) -> String? {
    guard let first = languages.first else { return nil }
    return first.lowercased().split(whereSeparator: { $0 == "-" || $0 == "_" }).first.map(String.init)
}

/// 한/영 토글의 목표 언어: 현재 한국어면 영어(1), 그 외(영어·미확인 포함)면
/// 한국어(2). 이 호스트의 기본 사용자는 한국어 입력 소스를 갖고 있다는 전제다.
func nextInputLanguage(currentTag: String?) -> UInt8 {
    currentTag == "ko" ? 1 : 2
}

/// Protected by CaptureSession.inputLock. A generation invalidates work already
/// dispatched to the main queue when focus is released or the session stops.
struct InputLanguageTransition {
    private(set) var sequence: UInt32?
    private(set) var generation: UInt64 = 0
    mutating func begin(sequence: UInt32) -> Bool {
        guard self.sequence == nil else { return false }
        generation &+= 1
        self.sequence = sequence
        return true
    }
    mutating func cancel() {
        generation &+= 1
        sequence = nil
    }
    mutating func complete(sequence: UInt32) -> Bool {
        guard self.sequence == sequence else { return false }
        self.sequence = nil
        return true
    }
}
