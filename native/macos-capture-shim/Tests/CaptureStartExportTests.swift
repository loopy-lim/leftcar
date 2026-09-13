import Foundation

// Compile the real v8/v9 exports and pure parsers. Only the final OS capture
// constructor is substituted; there are no screen/network entry points here.
private var calls = 0
private var applied: AppliedUdpStability?
private var error = ""
func setLastError(_ value: String) { error = value }
func startCaptureSession(ip: UnsafePointer<CChar>, port: UInt16, displayIndex: UInt32,
    width: UInt32, height: UInt32, fps: UInt32, backend: CaptureBackendKind,
    mediaTransport: MediaTransportKind = .udp, contentMode: StreamContentMode = .interactive,
    encoderExperiment: EncoderExperiment = .auto, udpStability: AppliedUdpStability = .legacy,
    mediaKey: Data, sourceID: String? = nil, authenticatedOwner: String? = nil,
    authorization: SourceAuthorization? = nil) -> UInt32 {
    calls += 1; applied = udpStability
    precondition(mediaKey.count == 32)
    if authorization != nil { precondition(sourceID == "macos:display:synthetic" && authenticatedOwner == "host-owner") }
    return 7
}
private struct Lease { var allowed = true; var active = 0; var released = 0 }
private func begin(_ pointer: UnsafeMutableRawPointer?) -> Int32 {
    let lease=pointer!.assumingMemoryBound(to: Lease.self)
    if !lease.pointee.allowed { return 0 };lease.pointee.active += 1;return 1
}
private func end(_ pointer: UnsafeMutableRawPointer?) { pointer!.assumingMemoryBound(to: Lease.self).pointee.active -= 1 }
private func release(_ pointer: UnsafeMutableRawPointer?) { pointer!.assumingMemoryBound(to: Lease.self).pointee.released += 1 }
@main struct CaptureStartExportTests {
    static func main() {
        func invoke(_ version: Int, _ transport: String, _ profile: String = "responsive", _ burst: UInt8 = 8,
                    _ parity: UInt8 = 2, _ adaptive: Int32 = 0, keyLength: UInt32 = 32,
                    backend: String = "sck", content: String = "interactive", experiment: String = "auto", nilKey: Bool = false,
                    source: String = "macos:display:synthetic", owner: String = "host-owner", allowed: Bool = true) -> UInt32 {
            let lease=UnsafeMutablePointer<Lease>.allocate(capacity:1);lease.initialize(to:Lease(allowed:allowed));defer{lease.deinitialize(count:1);lease.deallocate()}
            let key=[UInt8](repeating:7,count:32)
            let result=key.withUnsafeBufferPointer { key in
                "127.0.0.1".withCString { ip in transport.withCString { transport in profile.withCString { profile in
                    source.withCString { source in owner.withCString { owner in
                        backend.withCString { backend in content.withCString { content in experiment.withCString { experiment in
                        if version == 8 { return leftcarCaptureStartV8(ip:ip,port:5001,displayIndex:0,width:1920,height:1080,fps:60,backendName:backend,transportName:transport,contentModeName:content,encoderExperimentName:experiment,udpProfileName:profile,udpBurstDatagrams:burst,udpFecParityShards:parity,udpAdaptivePacing:adaptive,mediaKey:nilKey ? nil : key.baseAddress,mediaKeyLen:keyLength) }
                        return leftcarCaptureStartV9(ip:ip,port:5001,displayIndex:0,width:1920,height:1080,fps:60,backendName:backend,transportName:transport,contentModeName:content,encoderExperimentName:experiment,udpProfileName:profile,udpBurstDatagrams:burst,udpFecParityShards:parity,udpAdaptivePacing:adaptive,mediaKey:nilKey ? nil : key.baseAddress,mediaKeyLen:keyLength,sourceID:source,authenticatedOwner:owner,authorizationContext:lease,beginOperation:begin,endOperation:end,releaseContext:release)
                    }}}}}
                }}}
            }
            if version == 9 { precondition(lease.pointee.released==1 && lease.pointee.active==0,"v9 owns exactly one lease reference on every return") }
            return result
        }
        for version in [8,9] {
            precondition(invoke(version,"udp")==7)
            let beforeInvalid = calls
            precondition(invoke(version,"udp",backend:"invalid")==0)
            precondition(invoke(version,"udp",content:"invalid")==0)
            precondition(invoke(version,"udp",experiment:"invalid")==0)
            precondition(invoke(version,"udp",nilKey:true)==0)
            precondition(calls == beforeInvalid, "both actual exports enforce all shared sealed options")
            for transport in ["tcp","usb","adbTcp"] {
                precondition(invoke(version,transport)==7,"actual Host start tuple must reach native constructor: v\(version) \(transport): \(error)")
                precondition(applied == .legacy)
                precondition(invoke(version,transport,"auto",0,0)==7,"actual Host reconfigure placeholder")
                precondition(applied == .legacy)
                precondition(invoke(version,transport,"auto",0,0,1)==0)
                precondition(invoke(version,transport,"unknown")==0)
                precondition(invoke(version,transport,"responsive",1,2)==0)
                precondition(invoke(version,transport,keyLength:0)==0)
            }
            precondition(invoke(version,"udp","auto",0,0)==0,"UDP remains strict")
            precondition(invoke(version,"unknown")==0)
            precondition(invoke(version,"udp",keyLength:31)==0)
        }
        let before=calls
        precondition(invoke(9,"udp",source:"")==0)
        precondition(invoke(9,"usb",owner:"")==0)
        precondition(invoke(9,"tcp",allowed:false)==0)
        precondition(calls==before,"invalid/unauthorized v9 never reaches capture boundary")
        print("CaptureStartExportTests passed: real v8/v9 transport/key/source/owner/lease export routes")
    }
}
