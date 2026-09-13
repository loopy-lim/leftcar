import Foundation

/// Pure selector shared by the real native boundary and synthetic tests.
/// Indices remain legacy locators; a supplied stable ID must resolve exactly once.
func selectDisplaySource<T>(
    candidates: [(id: String?, value: T)], index: UInt32, sourceID: String?,
    benchmarkProfile: String? = ProcessInfo.processInfo.environment["LEFTCAR_BENCHMARK_PROFILE"],
    benchmarkSource: String? = ProcessInfo.processInfo.environment["LEFTCAR_BENCHMARK_SOURCE_ID"]
) -> T? {
    let selected: (id: String?, value: T)
    if let sourceID {
        guard !sourceID.isEmpty else { return nil }
        let matches = candidates.filter { $0.id == sourceID }
        guard matches.count == 1 else { return nil }
        selected = matches[0]
    } else {
        guard Int(index) < candidates.count else { return nil }
        selected = candidates[Int(index)]
    }
    if benchmarkProfile != nil || benchmarkSource != nil {
        guard let benchmarkProfile, !benchmarkProfile.isEmpty,
              let benchmarkSource, !benchmarkSource.isEmpty,
              selected.id == benchmarkSource,
              candidates.filter({ $0.id == benchmarkSource }).count == 1 else { return nil }
    }
    return selected.value
}
