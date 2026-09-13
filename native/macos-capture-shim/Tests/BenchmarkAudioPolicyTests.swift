import Foundation
@main struct BenchmarkAudioPolicyTests {
    static func main() {
        precondition(benchmarkSystemAudioAllowed(environment: [:]))
        precondition(!benchmarkSystemAudioAllowed(environment: ["LEFTCAR_BENCHMARK_SYSTEM_AUDIO": "off"]))
        // This decision precedes SCStream construction and is reapplied on
        // every ownership refresh. It is a restriction, never a source grant.
        for owner in [true, false] {
            precondition(!(benchmarkSystemAudioAllowed(environment: ["LEFTCAR_BENCHMARK_SYSTEM_AUDIO": "off"]) && owner))
        }
        print("BenchmarkAudioPolicyTests passed")
    }
}
