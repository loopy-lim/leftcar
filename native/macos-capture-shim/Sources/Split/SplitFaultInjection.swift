import Foundation

struct SplitFaultInjection {
    private(set) var remainingPairs: UInt64?
    private(set) var injectedDrops: Int64 = 0

    init(environment: [String: String] = ProcessInfo.processInfo.environment) {
        remainingPairs = environment["LEFTCAR_SPLIT_TEST_DROP_RIGHT_AU_AFTER"]
            .flatMap(UInt64.init)
            .flatMap { $0 > 0 ? $0 : nil }
    }

    mutating func shouldDropRightAu() -> Bool {
        guard let remainingPairs else { return false }
        if remainingPairs > 1 {
            self.remainingPairs = remainingPairs - 1
            return false
        }
        self.remainingPairs = nil
        injectedDrops &+= 1
        return true
    }
}
