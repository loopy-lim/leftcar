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
     func consumeViewerControl(_ fd: Int32) {
        if mediaTransport.usesTCP {
            consumeViewerTCPControl(fd)
            return
        }
        var bytes = [UInt8](repeating: 0, count: 512)
        let token = viewerControlToken
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
            let payload = Data(bytes[0..<count])
            guard payload.count >= token.count,
                  payload.suffix(token.count) == token else {
                continue
            }
            let message = Data(payload.dropLast(token.count))
            let sourcePort = UInt16(bigEndian: source.sin_port)
            let sourceSide = viewerControlSide(
                sourcePort: sourcePort,
                targetPort: targetPort,
                split: requestedEncoderExperiment == .splitVertical
            )
            if message == Data("BYE".utf8) {
                print("viewer close signal received for \(targetLabel)")
                requestViewerStop()
                return
            }
            if message == Data("IDR".utf8) {
                beginNetworkRecovery()
                // A prepared Android listener can consume the initial status
                // before its Surface exists. IDR is the renderer handoff
                // signal, so refresh both codec state and input-lock state.
                sendInputStatus(fd: fd)
                continue
            }
            if message == Data("LCDON".utf8) || message == Data("LCDOFF".utf8) {
                // Token-authenticated above, same frame class as IDR/BYE.
                // Older hosts never match these and drop the datagram, which
                // is exactly the design's backward-compatibility behavior.
                handleCursorStreamCommand(message, fd: fd, destination: source)
                continue
            }
            if message == Data("SNDON".utf8) || message == Data("SNDOFF".utf8) {
                // Same token-authenticated command class; audio needs no
                // reply address because it rides the existing media socket.
                handleSystemAudioCommand(message)
                continue
            }
            if message.count == 16,
               message.prefix(4) == Data("LCP1".utf8) {
                sendLatencyProbeResponse(message, fd: fd, destination: source)
                continue
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
                let payloadBytes = Array(payload)
                let tokenBytes = Array(viewerControlToken)
                guard payloadBytes.count >= tokenBytes.count,
                      Array(payloadBytes.suffix(tokenBytes.count)) == tokenBytes else {
                    continue
                }
                let messageBytes = Array(payloadBytes.dropLast(tokenBytes.count))
                if messageBytes == Array("BYE".utf8) {
                    print("viewer close signal received for \(targetLabel)")
                    requestViewerStop()
                    return
                }
                if messageBytes == Array("IDR".utf8) {
                    beginNetworkRecovery()
                    sendInputStatus(fd: fd)
                    continue
                }
                if messageBytes == Array("LCDON".utf8)
                    || messageBytes == Array("LCDOFF".utf8) {
                    let message = Data(messageBytes)
                    handleCursorStreamCommand(message, fd: fd, destination: nil)
                    continue
                }
                if messageBytes == Array("SNDON".utf8)
                    || messageBytes == Array("SNDOFF".utf8) {
                    handleSystemAudioCommand(Data(messageBytes))
                    continue
                }
                if messageBytes.count == 16,
                   Array(messageBytes.prefix(4)) == Array("LCP1".utf8) {
                    let message = Data(messageBytes)
                    sendLatencyProbeResponse(message, fd: fd, destination: nil)
                    continue
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
        if requestedEncoderExperiment == .splitVertical {
            if side == .left {
                splitLeftReceiverLoss = loss
                splitLeftRenderedFps = renderedFps
            } else {
                splitRightReceiverLoss = loss
                splitRightRenderedFps = renderedFps
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
        response.append(viewerControlToken)
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
        ack.append(viewerControlToken)
        _ = sendControlPayload(ack, fd: fd, destination: destination)
    }
}
