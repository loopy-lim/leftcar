import Foundation
@main struct DisplaySourceSelectionTests {
    static func main() {
        let displays: [(id: String?, value: Int)] = [("macos:display:A", 101), ("macos:display:B", 202)]
        func pick(_ list: [(id: String?, value: Int)], _ id: String?, _ profile: String? = nil, _ required: String? = nil) -> Int? {
            selectDisplaySource(candidates: list, index: 0, sourceID: id, benchmarkProfile: profile, benchmarkSource: required)
        }
        precondition(pick(displays.reversed(), "macos:display:A") == 101, "stable source survives reorder")
        precondition(pick(displays, "macos:display:missing") == nil)
        precondition(pick(displays + [displays[0]], "macos:display:A") == nil)
        precondition(pick([(nil, 1)], "") == nil)
        precondition(pick(displays, nil) == 101, "legacy ordinary index behavior")
        precondition(pick(displays, nil, "baseline068", "macos:display:B") == nil, "actual baseline selector rejects changed index")
        precondition(pick(displays.reversed(), nil, "baseline068", "macos:display:B") == 202)
        precondition(pick(displays, "macos:display:A", "candidate", nil) == nil)
        precondition(pick(displays, "macos:display:A", nil, "macos:display:A") == nil)
        precondition(pick(displays, "macos:display:A", "candidate", "macos:display:A") == 101)
        print("DisplaySourceSelectionTests passed")
    }
}
