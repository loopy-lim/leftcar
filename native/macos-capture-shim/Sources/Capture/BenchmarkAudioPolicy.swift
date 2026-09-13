import Foundation

/// Restrictive internal benchmark control. The Host profile validator rejects
/// use outside an explicit isolated profile. At the capture boundary, an off
/// value always fails closed, including before the first SCStream exists.
func benchmarkSystemAudioAllowed(environment: [String: String] = ProcessInfo.processInfo.environment) -> Bool {
    environment["LEFTCAR_BENCHMARK_SYSTEM_AUDIO"] != "off"
}
