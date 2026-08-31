import Foundation

/// v7 carries only the UDP policy already canonicalized by the Rust control
/// server. The shim validates the discrete preset tuple again at the ABI
/// boundary so a direct caller cannot create an unsupported hot-path state.
@_cdecl("leftcar_capture_start_v7")
public func leftcarCaptureStartV7(
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
    udpAdaptivePacing: Int32
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
        udpStability: udpStability
    )
}
