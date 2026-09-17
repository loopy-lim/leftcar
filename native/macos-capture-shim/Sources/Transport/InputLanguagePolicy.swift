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
