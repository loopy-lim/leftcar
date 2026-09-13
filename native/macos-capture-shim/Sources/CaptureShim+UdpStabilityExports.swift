import Foundation

private struct SealedCaptureOptions {
    let backend: CaptureBackendKind
    let mediaTransport: MediaTransportKind
    let contentMode: StreamContentMode
    let encoderExperiment: EncoderExperiment
    let udpStability: AppliedUdpStability
    let mediaKey: Data
}

/// Shared sealed-start contract; v9 source admission remains outside this helper.
private func sealedCaptureOptions(
    backendName: UnsafePointer<CChar>?, transportName: UnsafePointer<CChar>?,
    contentModeName: UnsafePointer<CChar>?, encoderExperimentName: UnsafePointer<CChar>?,
    udpProfileName: UnsafePointer<CChar>?, udpBurstDatagrams: UInt8,
    udpFecParityShards: UInt8, udpAdaptivePacing: Int32,
    mediaKey: UnsafePointer<UInt8>?, mediaKeyLen: UInt32
) -> SealedCaptureOptions? {
    let rawBackend = backendName.flatMap { String(validatingUTF8: $0) }
    guard let backend = CaptureBackendKind.parse(rawBackend) else {
        setLastError("unknown capture backend: \(rawBackend ?? "null")")
        return nil
    }
    let rawTransport = transportName.flatMap { String(validatingUTF8: $0) }
    guard let mediaTransport = MediaTransportKind.parse(rawTransport) else {
        setLastError("unknown media transport: \(rawTransport ?? "null")")
        return nil
    }
    let rawContentMode = contentModeName.flatMap { String(validatingUTF8: $0) }
    guard let contentMode = StreamContentMode.parse(rawContentMode) else {
        setLastError("unknown content mode: \(rawContentMode ?? "null")")
        return nil
    }
    let experiment = parseEncoderExperimentCString(encoderExperimentName)
    guard case let .success(encoderExperiment) = experiment else {
        if case let .failure(error) = experiment { setLastError(error) }
        return nil
    }
    let rawUdpProfile = udpProfileName.flatMap { String(validatingUTF8: $0) }
    guard let udpStability = captureTransportPolicy(
              transport: mediaTransport, profileName: rawUdpProfile,
              burstDatagrams: Int(udpBurstDatagrams),
              fecParityShards: Int(udpFecParityShards),
              adaptivePacing: udpAdaptivePacing != 0
          ) else {
        setLastError(
            "invalid UDP stability configuration: \(rawUdpProfile ?? "null")/\(udpBurstDatagrams)/\(udpFecParityShards)/\(udpAdaptivePacing)"
        )
        return nil
    }
    guard let mediaKey, mediaKeyLen == 32 else {
        setLastError("missing or malformed media encryption key (\(mediaKeyLen) bytes)")
        return nil
    }
    let keyData = Data(bytes: mediaKey, count: Int(mediaKeyLen))
    return SealedCaptureOptions(backend: backend, mediaTransport: mediaTransport,
        contentMode: contentMode, encoderExperiment: encoderExperiment,
        udpStability: udpStability, mediaKey: keyData)
}


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
    guard let options = sealedCaptureOptions(
        backendName: backendName, transportName: transportName, contentModeName: contentModeName,
        encoderExperimentName: encoderExperimentName, udpProfileName: udpProfileName,
        udpBurstDatagrams: udpBurstDatagrams, udpFecParityShards: udpFecParityShards,
        udpAdaptivePacing: udpAdaptivePacing, mediaKey: mediaKey, mediaKeyLen: mediaKeyLen
    ) else { return 0 }
    return startCaptureSession(
        ip: ip,
        port: port,
        displayIndex: displayIndex,
        width: width,
        height: height,
        fps: fps,
        backend: options.backend,
        mediaTransport: options.mediaTransport,
        contentMode: options.contentMode,
        encoderExperiment: options.encoderExperiment,
        udpStability: options.udpStability,
        mediaKey: options.mediaKey
    )
}

// v8 retains explicitly legacy index/IP semantics. The current Host requires v9.
@_cdecl("leftcar_capture_start_v9")
public func leftcarCaptureStartV9(
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
    mediaKeyLen: UInt32,
    sourceID: UnsafePointer<CChar>,
    authenticatedOwner: UnsafePointer<CChar>,
    authorizationContext: UnsafeMutableRawPointer,
    beginOperation: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Int32,
    endOperation: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Void,
    releaseContext: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Void
) -> UInt32 {
    let authorization = SourceAuthorization(context: authorizationContext, begin: beginOperation, end: endOperation, release: releaseContext)
    guard authorization.begin() else { setLastError("source authorization revoked"); return 0 }
    defer { authorization.end() }
    guard let source = String(validatingUTF8: sourceID), !source.isEmpty,
          let owner = String(validatingUTF8: authenticatedOwner), !owner.isEmpty else {
        setLastError("missing Host source/owner identity"); return 0
    }
    guard let options = sealedCaptureOptions(
        backendName: backendName, transportName: transportName, contentModeName: contentModeName,
        encoderExperimentName: encoderExperimentName, udpProfileName: udpProfileName,
        udpBurstDatagrams: udpBurstDatagrams, udpFecParityShards: udpFecParityShards,
        udpAdaptivePacing: udpAdaptivePacing, mediaKey: mediaKey, mediaKeyLen: mediaKeyLen
    ) else { return 0 }
    return startCaptureSession(
        ip: ip,
        port: port,
        displayIndex: displayIndex,
        width: width,
        height: height,
        fps: fps,
        backend: options.backend,
        mediaTransport: options.mediaTransport,
        contentMode: options.contentMode,
        encoderExperiment: options.encoderExperiment,
        udpStability: options.udpStability,
        mediaKey: options.mediaKey,
        sourceID: source,
        authenticatedOwner: owner,
        authorization: authorization
    )
}
