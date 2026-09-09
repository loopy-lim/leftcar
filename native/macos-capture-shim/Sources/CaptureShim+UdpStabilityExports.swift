import Foundation

/// v8 is the sealed-media ABI: the retired v7 UDP-policy parameter set plus
/// the trailing viewer-generated 32-byte media key. The key arrives over the
/// encrypted control plane and AEAD-seals every media datagram in both
/// directions. There is deliberately no plaintext fallback — a nil or
/// wrong-length key fails the start, and the unsealed v2..v7 exports no
/// longer exist.
@_cdecl("leftcar_capture_start_v8")
public func leftcarCaptureStartV8(
    ip: UnsafePointer<CChar>,
    port: UInt16,
    displayIndex: UInt32,
    width: UInt32,
    height: UInt32,
    fps: UInt32,
    backendName: UnsafePointer<CChar>?,
    transportName: UnsafePointer<CChar>?,
    contentModeName: UnsafePointer<CChar>?,
    encoderExperimentName: UnsafePointer<CChar>?,
    udpProfileName: UnsafePointer<CChar>?,
    udpBurstDatagrams: UInt8,
    udpFecParityShards: UInt8,
    udpAdaptivePacing: Int32,
    mediaKey: UnsafePointer<UInt8>?,
    mediaKeyLen: UInt32
) -> UInt32 {
    let rawBackend = backendName.flatMap { String(validatingUTF8: $0) }
    guard let backend = CaptureBackendKind.parse(rawBackend) else {
        setLastError("unknown capture backend: \(rawBackend ?? "null")")
        return 0
    }
    let rawTransport = transportName.flatMap { String(validatingUTF8: $0) }
    guard let mediaTransport = MediaTransportKind.parse(rawTransport) else {
        setLastError("unknown media transport: \(rawTransport ?? "null")")
        return 0
    }
    let rawContentMode = contentModeName.flatMap { String(validatingUTF8: $0) }
    guard let contentMode = StreamContentMode.parse(rawContentMode) else {
        setLastError("unknown content mode: \(rawContentMode ?? "null")")
        return 0
    }
    let experiment = parseEncoderExperimentCString(encoderExperimentName)
    guard case let .success(encoderExperiment) = experiment else {
        if case let .failure(error) = experiment { setLastError(error) }
        return 0
    }
    let rawUdpProfile = udpProfileName.flatMap { String(validatingUTF8: $0) }
    guard mediaTransport == .udp,
          let udpStability = AppliedUdpStability.validated(
              profileName: rawUdpProfile,
              burstDatagrams: Int(udpBurstDatagrams),
              fecParityShards: Int(udpFecParityShards),
              adaptivePacing: udpAdaptivePacing != 0
          ) else {
        setLastError(
            "invalid UDP stability configuration: \(rawUdpProfile ?? "null")/\(udpBurstDatagrams)/\(udpFecParityShards)/\(udpAdaptivePacing)"
        )
        return 0
    }
    guard let mediaKey, mediaKeyLen == 32 else {
        setLastError("missing or malformed media encryption key (\(mediaKeyLen) bytes)")
        return 0
    }
    let keyData = Data(bytes: mediaKey, count: Int(mediaKeyLen))
    return startCaptureSession(
        ip: ip,
        port: port,
        displayIndex: displayIndex,
        width: width,
        height: height,
        fps: fps,
        backend: backend,
        mediaTransport: mediaTransport,
        contentMode: contentMode,
        encoderExperiment: encoderExperiment,
        udpStability: udpStability,
        mediaKey: keyData
    )
}
