import Foundation
import CoreGraphics

private let kmExportLock = NSLock()
private var kmExportSession: KmNativeSession?

@_cdecl("leftcar_km_preflight_v1")
public func leftcarKmPreflightV1() -> Int32 { KmNativeSession.preflight() }

@_cdecl("leftcar_km_bounds_v1")
public func leftcarKmBoundsV1(_ output: UnsafeMutablePointer<Double>?) -> Int32 {
    guard let output else { return -1 }
    let bounds = KmNativeSession.desktopBounds()
    guard !bounds.isNull, !bounds.isEmpty else { return -1 }
    output[0] = bounds.minX; output[1] = bounds.minY; output[2] = bounds.width; output[3] = bounds.height
    return 0
}

@_cdecl("leftcar_km_start_v1")
public func leftcarKmStartV1(_ context: UnsafeMutableRawPointer?,
    _ callback: (@convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void)?) -> Int32 {
    guard let callback else { return -1 }
    kmExportLock.lock(); defer { kmExportLock.unlock() }
    guard kmExportSession == nil else { return -2 }
    let session = KmNativeSession(sink: callback, context: context)
    kmExportSession = session
    guard session.start() else { return -3 }
    return 0
}

@_cdecl("leftcar_km_owner_v1")
public func leftcarKmOwnerV1(_ owner: Int32) -> Int32 {
    guard let owner = KmNativeOwner(rawValue:owner) else { return -1 }
    kmExportLock.lock(); defer { kmExportLock.unlock() }
    guard let session = kmExportSession else { return -2 }
    return session.setOwner(owner) ? 0 : -3
}

@_cdecl("leftcar_km_inject_v1")
public func leftcarKmInjectV1(_ json: UnsafePointer<CChar>?) -> Int32 {
    guard let json, let data = String(cString:json).data(using:.utf8),
        let payload = (try? JSONSerialization.jsonObject(with:data)) as? [String:Any] else { return -1 }
    kmExportLock.lock(); defer { kmExportLock.unlock() }
    guard let session = kmExportSession else { return -2 }
    return session.inject(payload) ? 0 : -3
}

@_cdecl("leftcar_km_release_v1")
public func leftcarKmReleaseV1() {
    kmExportLock.lock(); defer { kmExportLock.unlock() }
    kmExportSession?.releaseAll()
}

@_cdecl("leftcar_km_stop_v2")
public func leftcarKmStopV2() -> Int32 {
    kmExportLock.lock(); defer { kmExportLock.unlock() }
    guard let session = kmExportSession else { return 0 }
    guard session.stop() else { return 1 }
    kmExportSession = nil
    return 0
}
