import Foundation
import AppKit
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import IOSurface
import Security
import Darwin
import OSLog

/// UDP/TCP 제어 경로가 공유하는 명령 디스패치. BYE는 수신 루프 자체를
/// 끝내고(.stop), 나머지 공통 명령은 소비 완료(.handled)다. LCF1 피드백과
/// 입력 프레임은 전송 경로별 의미가 달라 각 루프의 후미에서 처리한다.
enum ViewerControlDispatch {
    case stop
    case handled
    case unhandled
}

extension CaptureSession {
    func dispatchViewerControlCommand(
        _ message: Data,
        fd: Int32,
        destination: sockaddr_in?,
        sourceSide: TileSide? = nil
    ) -> ViewerControlDispatch {
        if message == Data("BYE".utf8) {
            print("viewer close signal received for \(targetLabel)")
            requestViewerStop()
            return .stop
        }
        if message == Data("IDR".utf8) {
            // Side-aware routing (per-tile gap recovery, R2): the split
            // control listener resolves the requesting tile by source port
            // BEFORE dispatch, so one tile's "IDR" can refresh that tile
            // alone when the peer's feedback is clean. Non-split sessions
            // and TCP/unmapped sources keep today's paired recovery.
            beginSplitAwareNetworkRecovery(requestingSide: sourceSide)
            // A prepared Android listener can consume the initial status
            // before its Surface exists. IDR is the renderer handoff
            // signal, so refresh both codec state and input-lock state.
            sendInputStatus(fd: fd)
            return .handled
        }
        if message.count >= 6, message.prefix(3) == Data("NAK".utf8) {
            // Selective retransmit request (viewer NACK/RTX). Older hosts
            // never match this prefix: the body falls through to the input
            // handler whose LCI1 guard drops it, which is the established
            // unknown-command behavior — and the released viewer never
            // sends NAK at all.
            handleViewerNack(message, fd: fd, destination: destination)
            return .handled
        }
        if message == Data("LCDON".utf8) || message == Data("LCDOFF".utf8) {
            // Token-authenticated above, same frame class as IDR/BYE.
            // Older hosts never match these and drop the datagram, which
            // is exactly the design's backward-compatibility behavior.
            handleCursorStreamCommand(message, fd: fd, destination: destination)
            return .handled
        }
        if message == Data("SNDON".utf8) || message == Data("SNDOFF".utf8) || message == Data("SNDA1O".utf8) || message == Data("SNDA1P".utf8) {
            // Same token-authenticated command class; audio needs no
            // reply address because it rides the existing media socket.
            handleSystemAudioCommand(message)
            return .handled
        }
        if message.count == 16,
           message.prefix(4) == Data("LCP1".utf8) {
            sendLatencyProbeResponse(message, fd: fd, destination: destination)
            return .handled
        }
        return .unhandled
    }
}

func splitGapRecoveryCounts(
    _ message: Data
) -> (keyframe: UInt32, delta: UInt32)? {
    guard message.count >= 60 else { return nil }
    let bytes = Array(message)
    let read = { (offset: Int) -> UInt32 in
        (UInt32(bytes[offset]) << 24)
            | (UInt32(bytes[offset + 1]) << 16)
            | (UInt32(bytes[offset + 2]) << 8)
            | UInt32(bytes[offset + 3])
    }
    return (keyframe: read(52), delta: read(56))
}

private extension TileSide {
    var peer: TileSide { self == .left ? .right : .left }
}

/// Single-session media and control intentionally use different Android UDP
/// sockets so input/feedback cannot wait behind a 4K fragment burst. The
/// authenticated control socket therefore has an ephemeral source port. Split
/// sessions still send each tile's feedback from its bound media port, which
/// is required to identify left and right independently.
func viewerControlSide(
    sourcePort: UInt16,
    targetPort: UInt16,
    split: Bool
) -> TileSide? {
    guard split else { return .left }
    if sourcePort == targetPort { return .left }
    if targetPort < UInt16.max, sourcePort == targetPort + 1 { return .right }
    return nil
}

extension CaptureSession {
    /// Viewer-requested selective retransmit. Wire body (AEAD-sealed like
    /// IDR/BYE): `NAK | au_id u16 LE | count u8 | count × fragment_index u16
    /// LE`, count ≤ 20 per message. Each requested fragment still present in
    /// the retransmit ring is re-sent through the normal media send path with
    /// a fresh AEAD counter (never a replay of old sealed bytes, which the
    /// viewer's replay window would reject). Evicted fragments are counted as
    /// misses and skipped silently; the viewer's bounded grace then expires
    /// into its existing freeze + IDR recovery.
    func handleViewerNack(
        _ message: Data,
        fd: Int32,
        destination: sockaddr_in?
    ) {
        let bytes = Array(message)
        guard bytes.count >= 6 else { return }
        let auID = UInt16(bytes[3]) | (UInt16(bytes[4]) << 8)
        let requested = Int(bytes[5])
        guard requested > 0 else { return }
        // Defensive caps: never read past the datagram, never serve more than
        // the protocol maximum per message.
        let count = min(requested, 20, (bytes.count - 6) / 2)
        // The requesting tile is identified by its source port (split paths
        // send from each tile's media port); the reply goes back over the
        // same side's media route. Single sessions map to .left, whose
        // sendToTile route is byte-identical to sendToViewer.
        let sourceSide: TileSide
        if let destination {
            guard let side = viewerControlSide(
                sourcePort: UInt16(bigEndian: destination.sin_port),
                targetPort: targetPort,
                split: requestedEncoderExperiment == .splitVertical
            ) else {
                return
            }
            sourceSide = side
        } else {
            sourceSide = .left
        }
        var served = 0
        var missed = 0
        for index in 0..<count {
            let offset = 6 + index * 2
            let fragmentIndex = UInt16(bytes[offset])
                | (UInt16(bytes[offset + 1]) << 8)
            if let envelope = retransmitRing.lookup(
                auID: auID,
                fragmentIndex: fragmentIndex,
                side: sourceSide
            ) {
                _ = sendMediaDatagram(envelope, fd: fd, tileSide: sourceSide)
                served += 1
            } else {
                missed += 1
            }
        }
        stateLock.lock()
        nacksServed &+= Int64(served)
        nacksMissed &+= Int64(missed)
        stateLock.unlock()
    }

    /// Fresh-loss window for the side-aware IDR decision. The requesting
    /// tile's cumulative receiver-loss increase is reported by the first-loss
    /// fast LCF1, which the viewer sends BEFORE its IDR on the same socket —
    /// but reordering and the 1Hz fallback both exist, so the window only has
    /// to cover "this gap was reported recently", not any timing guarantee.
    private static let splitPerTileLossFreshWindowNs: UInt64 = 2_000_000_000

    /// Side-aware viewer IDR entry (per-tile gap recovery, R2). The
    /// transport-level paired recovery (deadline abort, overflow, encoder
    /// faults) is untouched — only the viewer-PLI path becomes side-aware.
    ///
    /// Decision rule, evaluated once per arriving IDR (never re-evaluated
    /// mid-recovery: while a paired boundary is pending the call below is
    /// suppressed by the existing recoveryAlreadyPending gate, and a per-tile
    /// decision is a single one-shot force consumed by the next admission):
    /// - Paired recovery pending → paired (suppressed no-op; latched).
    /// - Viewer is per-tile-capable (extended LCF1 body) AND the requesting
    ///   tile shows a fresh loss increase AND the peer's feedback is clean
    ///   → per-tile: force a keyframe on the requesting encoder only. No
    ///   flow-generation bump, no boundary, the peer streams through.
    /// - Everything else — burst loss on both tiles, no fresh loss signal
    ///   for the requester (fast LCF1 lost or not yet parsed), an
    ///   already-healed duplicate, an old paired-only viewer, TCP or an
    ///   unmapped source port — keeps today's `beginSplitTransportRecovery`.
    func beginSplitAwareNetworkRecovery(requestingSide: TileSide?) {
        guard requestedEncoderExperiment == .splitVertical else {
            beginNetworkRecovery()
            return
        }
        guard let requestingSide else {
            beginSplitTransportRecovery(reason: "network recovery requested")
            return
        }
        captureLock.lock()
        let boundaryPending = splitFlowState.recoveryBoundaryPending
        captureLock.unlock()
        if boundaryPending {
            beginSplitTransportRecovery(reason: "network recovery requested")
            return
        }
        stateLock.lock()
        let capable = splitPerTileKeyframeCapable
        let requesterLossFresh = splitTileLossFreshLocked(requestingSide)
        let peerLossFresh = splitTileLossFreshLocked(requestingSide.peer)
        stateLock.unlock()
        if capable, requesterLossFresh, !peerLossFresh {
            requestSplitTileKeyframe(
                side: requestingSide,
                reason: "per-tile recovery requested"
            )
            return
        }
        beginSplitTransportRecovery(reason: "network recovery requested")
    }

    /// Caller holds stateLock. True when the tile's cumulative receiver-loss
    /// counter increased within the fresh window (zero mark = never lost).
    private func splitTileLossFreshLocked(_ side: TileSide) -> Bool {
        let mark = side == .left
            ? splitLeftReceiverLossMarkNs
            : splitRightReceiverLossMarkNs
        guard mark != 0 else { return false }
        let now = DispatchTime.now().uptimeNanoseconds
        return now >= mark
            && now - mark <= Self.splitPerTileLossFreshWindowNs
    }

     func consumeViewerControl(_ fd: Int32) {
        if mediaTransport.usesTCP {
            consumeViewerTCPControl(fd)
            return
        }
        // Sealed viewer datagrams carry +24B AEAD overhead; the previous
        // plaintext budget (512) plus headroom keeps legal IME text and the
        // widest feedback frame intact.
        var bytes = [UInt8](repeating: 0, count: 640)
        while true {
            var source = sockaddr_in()
            var sourceLength = socklen_t(MemoryLayout<sockaddr_in>.size)
            let count = bytes.withUnsafeMutableBytes { raw in
                withUnsafeMutablePointer(to: &source) { pointer in
                    pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                        recvfrom(
                            fd,
                            raw.baseAddress,
                            raw.count,
                            MSG_DONTWAIT,
                            socketAddress,
                            &sourceLength
                        )
                    }
                }
            }
            guard count > 0 else { break }
            // AEAD possession is the authentication: anything that does not
            // open under the session media key is dropped without parsing.
            guard let message = mediaCrypto.open(Data(bytes[0..<count])) else {
                continue
            }
            let sourcePort = UInt16(bigEndian: source.sin_port)
            let sourceSide = viewerControlSide(
                sourcePort: sourcePort,
                targetPort: targetPort,
                split: requestedEncoderExperiment == .splitVertical
            )
            switch dispatchViewerControlCommand(
                message,
                fd: fd,
                destination: source,
                sourceSide: sourceSide
            ) {
            case .stop:
                return
            case .handled:
                continue
            case .unhandled:
                break
            }
            if message.count >= 24,
               message.prefix(4) == Data("LCF1".utf8) {
                guard let sourceSide else { continue }
                recordViewerFeedback(message, side: sourceSide)
                armReceiverHealthCheck()
                continue
            }
            guard sourceSide == .left else { continue }
            handleInputMessage(message, fd: fd, destination: source)
        }
    }

     func consumeViewerTCPControl(_ fd: Int32) {
        var bytes = [UInt8](repeating: 0, count: 4096)
        while true {
            let count = bytes.withUnsafeMutableBytes { raw in
                Darwin.recv(fd, raw.baseAddress, raw.count, MSG_DONTWAIT)
            }
            if count == 0 {
                requestViewerStop()
                return
            }
            if count < 0 {
                if errno == EAGAIN || errno == EWOULDBLOCK { return }
                requestViewerStop()
                return
            }
            tcpControlBuffer.append(contentsOf: bytes[0..<count])
            while true {
                // Data can retain a non-zero startIndex after a partial
                // consume. Copying to Array gives this parser a stable,
                // zero-based view and removes every direct Data subscript
                // from the untrusted TCP framing path.
                let framed = Array(tcpControlBuffer)
                guard framed.count >= 4 else { break }
                let length = (UInt32(framed[0]) << 24)
                    | (UInt32(framed[1]) << 16)
                    | (UInt32(framed[2]) << 8)
                    | UInt32(framed[3])
                guard isValidTcpMediaFrameLength(Int(length)) else {
                    requestViewerStop()
                    return
                }
                let frameLength = 4 + Int(length)
                guard framed.count >= frameLength else { break }
                let payload = Data(framed[4..<frameLength])
                tcpControlBuffer.removeAll(keepingCapacity: true)
                tcpControlBuffer.append(contentsOf: framed[frameLength...])

                // Keep all untrusted TCP parsing on Array. Foundation.Data
                // slices may retain a non-zero index and can trap when a
                // later Collection operation assumes a zero-based buffer.
                // The sealed payload must open under the session media key
                // before any of it is interpreted.
                guard let opened = mediaCrypto.open(Data(payload)) else {
                    continue
                }
                let messageBytes = Array(opened)
                switch dispatchViewerControlCommand(Data(messageBytes), fd: fd, destination: nil) {
                case .stop:
                    return
                case .handled:
                    continue
                case .unhandled:
                    break
                }
                if messageBytes.count >= 24,
                   Array(messageBytes.prefix(4)) == Array("LCF1".utf8) {
                    let message = Data(messageBytes)
                    stateLock.lock()
                    receiverFrameGaps = readUInt32BE(message, at: 4)
                    receiverInputDrops = readUInt32BE(message, at: 8)
                    receiverIncompleteAUs = readUInt32BE(message, at: 12)
                    receiverStaleFrames = readUInt32BE(message, at: 16)
                    receiverRttMs = readUInt16BE(message, at: 20)
                    receiverWireMs = readUInt16BE(message, at: 22)
                    receiverStaleInputDrops = message.count >= 32
                        ? readUInt32BE(message, at: 24)
                        : nil
                    receiverOutputBurstDiscards = message.count >= 32
                        ? readUInt32BE(message, at: 28)
                        : 0
                    receiverRenderedFps = message.count >= 34
                        ? UInt32(readUInt16BE(message, at: 32))
                        : nil
                    receiverFeedbackNs = DispatchTime.now().uptimeNanoseconds
                    stateLock.unlock()
                    armReceiverHealthCheck()
                    continue
                }
                let message = Data(messageBytes)
                handleInputMessage(message, fd: fd, destination: nil)
            }
        }
    }

     func readUInt16BE(_ data: Data, at offset: Int) -> UInt16 {
        guard offset >= 0, data.count >= offset + 2 else { return 0 }
        let bytes = Array(data)
        return (UInt16(bytes[offset]) << 8) | UInt16(bytes[offset + 1])
    }

     func recordViewerFeedback(_ message: Data, side: TileSide) {
        let frameGaps = readUInt32BE(message, at: 4)
        let inputDrops = readUInt32BE(message, at: 8)
        let incompleteAUs = readUInt32BE(message, at: 12)
        let staleFrames = readUInt32BE(message, at: 16)
        let renderedFps = message.count >= 34
            ? UInt32(readUInt16BE(message, at: 32))
            : 0
        let loss = UInt64(frameGaps)
            + UInt64(inputDrops)
            + UInt64(incompleteAUs)
            + UInt64(staleFrames)
        let feedbackV2 = message.count >= 120
        let mediaDatagrams = feedbackV2 ? readUInt64BE(message, at: 60) : 0
        let dataDatagrams = feedbackV2 ? readUInt64BE(message, at: 68) : 0
        let parityDatagrams = feedbackV2 ? readUInt64BE(message, at: 76) : 0
        let restoredFragments = feedbackV2 ? readUInt64BE(message, at: 84) : 0
        let unrecoverableGroups = feedbackV2 ? readUInt32BE(message, at: 92) : 0
        let maxMissingFragments = feedbackV2 ? readUInt16BE(message, at: 96) : 0
        let oneFrameGapEvents = feedbackV2 ? readUInt32BE(message, at: 100) : 0
        let multiFrameGapEvents = feedbackV2 ? readUInt32BE(message, at: 104) : 0
        let pairedIdrEpisodes = feedbackV2 ? readUInt32BE(message, at: 108) : 0
        let suppressedRecovery = feedbackV2 ? readUInt32BE(message, at: 112) : 0
        let fecDecodeFailures = feedbackV2 ? readUInt32BE(message, at: 116) : 0
        // Length-tolerant v3 suffix: each field is read only when the body is
        // long enough to carry it, so a released viewer's exactly-120B body
        // and a future viewer's longer body both parse here.
        let pairedIdrResumes = message.count >= 124 ? readUInt32BE(message, at: 120) : 0
        let splitWireMs = message.count >= 126 ? readUInt16BE(message, at: 124) : 0
        let splitCaptureAgeMs = message.count >= 128 ? readUInt16BE(message, at: 126) : 0
        let splitInputRttMs = message.count >= 130 ? readUInt16BE(message, at: 128) : 0
        // v3 suffix continuation (bytes 130..134): per-tile IDR resumes. A
        // viewer sending this field is also, by construction, per-tile
        // recovery capable — the capability bit drives the side-aware IDR
        // decision and is session-stable, so the decision can never flap
        // between per-tile and paired mid-session.
        let perTileIdrResumes = message.count >= 134 ? readUInt32BE(message, at: 130) : 0
        let feedbackNowNs = DispatchTime.now().uptimeNanoseconds
        stateLock.lock()
        receiverFrameGaps = max(receiverFrameGaps, frameGaps)
        receiverInputDrops = max(receiverInputDrops, inputDrops)
        receiverIncompleteAUs = max(receiverIncompleteAUs, incompleteAUs)
        receiverStaleFrames = max(receiverStaleFrames, staleFrames)
        receiverRttMs = readUInt16BE(message, at: 20)
        receiverWireMs = readUInt16BE(message, at: 22)
        receiverStaleInputDrops = message.count >= 32
            ? readUInt32BE(message, at: 24)
            : nil
        receiverOutputBurstDiscards = message.count >= 32
            ? readUInt32BE(message, at: 28)
            : 0
        receiverRenderedFps = renderedFps > 0 ? renderedFps : nil
        if feedbackV2 {
            receiverMediaDatagrams = max(receiverMediaDatagrams, mediaDatagrams)
            receiverDataDatagrams = max(receiverDataDatagrams, dataDatagrams)
            receiverParityDatagrams = max(receiverParityDatagrams, parityDatagrams)
            receiverFecRestoredFragments = max(
                receiverFecRestoredFragments,
                restoredFragments
            )
            receiverUnrecoverableFecGroups = max(
                receiverUnrecoverableFecGroups,
                unrecoverableGroups
            )
            receiverMaxMissingDataFragments = max(
                receiverMaxMissingDataFragments,
                maxMissingFragments
            )
            receiverOneFrameGapEvents = max(
                receiverOneFrameGapEvents,
                oneFrameGapEvents
            )
            receiverMultiFrameGapEvents = max(
                receiverMultiFrameGapEvents,
                multiFrameGapEvents
            )
            receiverPairedIdrEpisodes = max(
                receiverPairedIdrEpisodes,
                pairedIdrEpisodes
            )
            receiverSuppressedRecoveryRequests = max(
                receiverSuppressedRecoveryRequests,
                suppressedRecovery
            )
            receiverFecDecodeFailures = max(
                receiverFecDecodeFailures,
                fecDecodeFailures
            )
            let decision = udpBurstPolicyState.observe(.init(
                nowNs: feedbackNowNs,
                incompleteAccessUnits: receiverIncompleteAUs,
                oneFrameGapEvents: receiverOneFrameGapEvents,
                multiFrameGapEvents: receiverMultiFrameGapEvents,
                recoveryBoundarySent: false
            ))
            activeUdpBurstDatagrams = decision.burstDatagrams
            activeUdpFecParityShards = decision.fecParityShards
            activeUdpBurstReason = decision.reason.rawValue
        }
        // v3 suffix fields keep their own length gates so a partially
        // extended sender never writes a truncated value. The resume counter
        // is cumulative and merges monotonically like the other v2 counters;
        // the clock-corrected ages are smoothed samples and take the newest.
        if message.count >= 124 {
            receiverPairedIdrResumes = max(receiverPairedIdrResumes, pairedIdrResumes)
        }
        if message.count >= 134 {
            receiverPerTileIdrResumes = max(receiverPerTileIdrResumes, perTileIdrResumes)
        }
        if message.count >= 126 {
            receiverSplitWireMs = splitWireMs
        }
        if message.count >= 128 {
            receiverSplitCaptureAgeMs = splitCaptureAgeMs
        }
        if message.count >= 130 {
            receiverInputRttMs = splitInputRttMs
        }
        if requestedEncoderExperiment == .splitVertical {
            // A counter INCREASE (not merely a high cumulative value) marks
            // the tile as freshly lossy; the side-aware IDR decision reads
            // the marks, not the raw cumulative totals.
            if side == .left {
                if loss > splitLeftReceiverLoss {
                    splitLeftReceiverLossMarkNs = feedbackNowNs
                }
                splitLeftReceiverLoss = loss
                splitLeftRenderedFps = renderedFps
            } else {
                if loss > splitRightReceiverLoss {
                    splitRightReceiverLossMarkNs = feedbackNowNs
                }
                splitRightReceiverLoss = loss
                splitRightRenderedFps = renderedFps
            }
            // Session-stable per-tile capability: the extended LCF1 body
            // carrying the perTileIdrResumes suffix field only exists in
            // viewers that implement per-tile resume.
            if message.count >= 134 {
                splitPerTileKeyframeCapable = true
            }
            if message.count >= 52 {
                splitJoinedRenderedFps = UInt32(readUInt16BE(message, at: 34))
                splitPairReadyDeltaP95Us = UInt64(readUInt32BE(message, at: 36))
                splitPairReadyDeltaMaxUs = UInt64(readUInt32BE(message, at: 40))
                splitPairSyncTimeouts = Int64(readUInt32BE(message, at: 44))
                splitUnmatchedOutputDrops = Int64(readUInt32BE(message, at: 48))
            }
            if let gapRecoveries = splitGapRecoveryCounts(message) {
                splitKeyframeGapRecoveries = max(
                    splitKeyframeGapRecoveries,
                    gapRecoveries.keyframe
                )
                splitDeltaGapRecoveries = max(
                    splitDeltaGapRecoveries,
                    gapRecoveries.delta
                )
            }
        }
        receiverFeedbackNs = feedbackNowNs
        stateLock.unlock()
    }

     func readUInt32BE(_ data: Data, at offset: Int) -> UInt32 {
        guard offset >= 0, data.count >= offset + 4 else { return 0 }
        let bytes = Array(data)
        return (UInt32(bytes[offset]) << 24)
            | (UInt32(bytes[offset + 1]) << 16)
            | (UInt32(bytes[offset + 2]) << 8)
            | UInt32(bytes[offset + 3])
    }

     func readUInt64BE(_ data: Data, at offset: Int) -> UInt64 {
        guard offset >= 0, data.count >= offset + 8 else { return 0 }
        let bytes = Array(data)
        var value: UInt64 = 0
        for byte in bytes[offset..<(offset + 8)] {
            value = (value << 8) | UInt64(byte)
        }
        return value
    }

    /// NTP-style authenticated probe. Echoing the Android send time together
    /// with Host receive/send wall times lets the viewer separate LAN RTT from
    /// Host-to-device media delivery without assuming synchronized clocks.
     func sendLatencyProbeResponse(
        _ message: Data,
        fd: Int32,
        destination: sockaddr_in?
    ) {
        let sequence = readUInt32BE(message, at: 4)
        let viewerSendMs = readUInt64BE(message, at: 8)
        let hostReceiveMs = UInt64(Date().timeIntervalSince1970 * 1000.0)
        var response = Data("LCP2".utf8)
        var sequenceBE = sequence.bigEndian
        var viewerSendBE = viewerSendMs.bigEndian
        var hostReceiveBE = hostReceiveMs.bigEndian
        withUnsafeBytes(of: &sequenceBE) { response.append(contentsOf: $0) }
        withUnsafeBytes(of: &viewerSendBE) { response.append(contentsOf: $0) }
        withUnsafeBytes(of: &hostReceiveBE) { response.append(contentsOf: $0) }
        var hostSendBE = UInt64(Date().timeIntervalSince1970 * 1000.0).bigEndian
        withUnsafeBytes(of: &hostSendBE) { response.append(contentsOf: $0) }
        _ = sendControlPayload(response, fd: fd, destination: destination)
    }

     func sendControlPayload(
        _ data: Data,
        fd: Int32,
        destination: sockaddr_in? = nil
    ) -> Int {
        if mediaTransport.usesTCP {
            return sendTCPFrame(data, fd: fd)
        }
        guard var destination else { return -1 }
        return send(data, fd: fd, to: &destination)
    }

     func sendInputAck(
        sequence: UInt32,
        fd: Int32,
        destination: sockaddr_in?
    ) {
        var ack = Data("LCA1".utf8)
        var sequenceBE = sequence.bigEndian
        withUnsafeBytes(of: &sequenceBE) { ack.append(contentsOf: $0) }
        inputLock.lock()
        let enabled = inputEnabled
        inputLock.unlock()
        ack.append(enabled ? 1 : 0)
        _ = sendControlPayload(ack, fd: fd, destination: destination)
    }
}
