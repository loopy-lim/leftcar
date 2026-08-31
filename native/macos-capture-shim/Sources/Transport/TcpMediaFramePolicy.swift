/// A single 4K recovery access unit can temporarily exceed the old 2 MiB
/// framing ceiling on a highly complex desktop. The decoder already accepts
/// up to 16 MiB, so the reliable transport uses the same fail-closed limit.
let tcpMediaFrameLimitBytes = 16 * 1024 * 1024

func isValidTcpMediaFrameLength(_ length: Int) -> Bool {
    length > 0 && length <= tcpMediaFrameLimitBytes
}
