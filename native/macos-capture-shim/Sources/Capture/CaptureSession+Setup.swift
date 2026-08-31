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

extension CaptureSession {
    // MARK: Setup

     func sendToViewer(_ data: Data, fd: Int32) -> Int {
        var addr = targetAddr
        return send(data, fd: fd, to: &addr)
    }

     func sendToTile(_ data: Data, side: TileSide, fd: Int32) -> Int {
        var addr = targetAddr
        if side == .right {
            guard targetPort < UInt16.max else { return -1 }
            addr.sin_port = (targetPort + 1).bigEndian
        }
        return send(data, fd: fd, to: &addr)
    }

     func sendTCPBytes(_ data: Data, fd: Int32) -> Int {
        data.withUnsafeBytes { raw in
            guard let baseAddress = raw.baseAddress else { return 0 }
            var offset = 0
            while offset < raw.count {
                let sent = Darwin.send(
                    fd,
                    baseAddress.advanced(by: offset),
                    raw.count - offset,
                    0
                )
                guard sent > 0 else { return offset }
                offset += sent
            }
            return offset
        }
    }

    /// TCP carries the existing CFG/G/control payloads as independent frames.
    /// The Android USB bridge converts them back to loopback UDP datagrams, so
    /// the decoder and its recovery telemetry stay identical on both paths.
     func sendTCPFrame(_ data: Data, fd: Int32) -> Int {
        guard isValidTcpMediaFrameLength(data.count) else { return -1 }
        var length = UInt32(data.count).bigEndian
        var framed = Data(capacity: data.count + 4)
        withUnsafeBytes(of: &length) { framed.append(contentsOf: $0) }
        framed.append(data)
        tcpWriteLock.lock()
        defer { tcpWriteLock.unlock() }
        return sendTCPBytes(framed, fd: fd) == framed.count ? data.count : -1
    }

     func send(_ data: Data, fd: Int32, to addr: inout sockaddr_in) -> Int {
        return data.withUnsafeBytes { raw in
            guard let baseAddress = raw.baseAddress else { return -1 }
            return withUnsafePointer(to: &addr) { pointer in
                pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                    sendto(
                        fd,
                        baseAddress,
                        raw.count,
                        0,
                        socketAddress,
                        socklen_t(MemoryLayout<sockaddr_in>.size)
                    )
                }
            }
        }
    }

    /// A non-blocking UDP socket can transiently report EAGAIN/ENOBUFS during
    /// a Wi-Fi scheduling pause. This is an intentional loss-tolerant media
    /// path: never wait for the kernel to drain because the wait would hold
    /// every newer frame behind an already-lost datagram. The next IDR
    /// re-establishes the H.264 dependency chain.
     func sendMediaDatagram(
        _ data: Data,
        fd: Int32,
        tileSide: TileSide? = nil
    ) -> Int {
        if mediaTransport.usesTCP {
            let sent = sendTCPFrame(data, fd: fd)
            if sent == data.count { return sent }
            stateLock.lock()
            udpSendFailures &+= 1
            stateLock.unlock()
            return sent
        }
        let sent = tileSide.map { sendToTile(data, side: $0, fd: fd) }
            ?? sendToViewer(data, fd: fd)
        if sent == data.count { return sent }
        stateLock.lock()
        udpSendFailures &+= 1
        stateLock.unlock()
        return sent
    }

     func paceNetwork(until deadlineNs: UInt64) {
        let now = DispatchTime.now().uptimeNanoseconds
        guard deadlineNs > now else { return }
        let remainingUs = (deadlineNs - now) / 1_000
        if remainingUs > 0 {
            usleep(useconds_t(min(remainingUs, UInt64(useconds_t.max))))
        }
    }

     func paceUdpDatagram(
        bytes: Int,
        isKeyframe: Bool = false,
        accessUnitBytes: Int = 0,
        dataFragmentCount: Int = 0,
        selectedParity: Int? = nil
    ) {
        guard mediaTransport == .udp else { return }
        stateLock.lock()
        let bitrate = max(1, currentAverageBitrate)
        let effectiveBurstDatagrams = motionAdjustedUdpBurstDatagrams(
            base: activeUdpBurstDatagrams,
            mode: adaptiveMotionState.mode(
                at: DispatchTime.now().uptimeNanoseconds
            )
        )
        let pacingRateMultiplier = udpPacingRateMultiplier(
            profile: appliedUdpStability.profile,
            burstDatagrams: effectiveBurstDatagrams
        )
        stateLock.unlock()
        // Pace at the configured media rate, with a small floor to avoid
        // recreating a burst for tiny tail fragments. The deadline is shared
        // by all AUs because each access unit is drained on one serial queue.
        let intervalUs = udpDatagramIntervalUs(
            bytes: bytes,
            isKeyframe: isKeyframe,
            accessUnitBytes: accessUnitBytes,
            targetBitrate: bitrate,
            fps: fps,
            contentMode: contentMode.rawValue,
            dataFragmentCount: dataFragmentCount,
            selectedParity: selectedParity,
            pacingRateMultiplier: pacingRateMultiplier
        )
        let now = DispatchTime.now().uptimeNanoseconds
        let deadline = max(now, nextUdpSendNs)
        paceNetwork(until: deadline)
        let sentAt = DispatchTime.now().uptimeNanoseconds
        nextUdpSendNs = max(deadline, sentAt) + intervalUs * 1_000
    }

     func receiveTCPFrame(fd: Int32, timeoutMs: Int32) -> Data? {
        func readExactly(_ length: Int) -> Data? {
            var result = Data(count: length)
            var offset = 0
            while offset < length {
                var descriptor = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)
                guard poll(&descriptor, 1, timeoutMs) > 0 else { return nil }
                let received = result.withUnsafeMutableBytes { raw in
                    Darwin.recv(
                        fd,
                        raw.baseAddress!.advanced(by: offset),
                        length - offset,
                        MSG_DONTWAIT
                    )
                }
                guard received > 0 else { return nil }
                offset += received
            }
            return result
        }

        guard let header = readExactly(4) else { return nil }
        let headerBytes = Array(header)
        guard headerBytes.count == 4 else { return nil }
        let length = (UInt32(headerBytes[0]) << 24)
            | (UInt32(headerBytes[1]) << 16)
            | (UInt32(headerBytes[2]) << 8)
            | UInt32(headerBytes[3])
        guard isValidTcpMediaFrameLength(Int(length)) else { return nil }
        return readExactly(Int(length))
    }

     func connectTCPSocket(stopOnFailure: Bool) -> Bool {
        sock = socket(AF_INET, SOCK_STREAM, 0)
        guard sock >= 0 else {
            let reason = "TCP socket() failed"
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }
        var noDelay: Int32 = 1
        _ = setsockopt(sock, IPPROTO_TCP, TCP_NODELAY, &noDelay, socklen_t(MemoryLayout<Int32>.size))
        var receiveTimeout = timeval(tv_sec: 2, tv_usec: 0)
        _ = setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &receiveTimeout, socklen_t(MemoryLayout<timeval>.size))
        var sendTimeout = timeval(tv_sec: 0, tv_usec: 100_000)
        _ = setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, &sendTimeout, socklen_t(MemoryLayout<timeval>.size))

        var address = targetAddr
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                Darwin.connect(
                    sock,
                    socketAddress,
                    socklen_t(MemoryLayout<sockaddr_in>.size)
                ) == 0
            }
        }
        guard connected else {
            let reason = "TCP media connection failed for \(targetLabel): errno=\(errno)"
            close(sock)
            sock = -1
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }

        let token = Data(UUID().uuidString.utf8)
        var challenge = Data("LCH1".utf8)
        challenge.append(token)
        guard sendTCPFrame(challenge, fd: sock) == challenge.count,
              let response = receiveTCPFrame(fd: sock, timeoutMs: 2_000),
              response == challenge else {
            let reason = "TCP media handshake failed for \(targetLabel)"
            close(sock)
            sock = -1
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }

        let originalFlags = fcntl(sock, F_GETFL, 0)
        if originalFlags >= 0 {
            _ = fcntl(sock, F_SETFL, originalFlags | O_NONBLOCK)
        }
        viewerControlToken = token
        inputLock.lock()
        lastReliableInputSequence = 0
        lastPointerInputSequence = 0
        inputLock.unlock()
        startInputReceiver(fd: sock)
        sendInputStatus(fd: sock)

        stateLock.lock()
        if stopRequested {
            stateLock.unlock()
            close(sock)
            sock = -1
            return false
        }
        lifecycleState = firstSendNs == nil ? "starting_capture" : "running"
        stateLock.unlock()
        return true
    }

    func connectSocket(stopOnFailure: Bool = true) -> Bool {
        stopInputReceiver()
        stateLock.lock()
        let shouldStop = stopRequested
        stateLock.unlock()
        if shouldStop { return false }

        if mediaTransport.usesTCP {
            return connectTCPSocket(stopOnFailure: stopOnFailure)
        }

        sock = socket(AF_INET, SOCK_DGRAM, 0)
        guard sock >= 0 else {
            let reason = "UDP socket() failed"
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }
        // Keep only a short kernel burst behind the app's latest-frame queue.
        // A multi-megabyte buffer can preserve stale video through Wi-Fi
        // scheduling pauses even though userspace keeps only one frame.
        var sendBuffer: Int32 = 512 * 1024
        setsockopt(sock, SOL_SOCKET, SO_SNDBUF, &sendBuffer, socklen_t(MemoryLayout<Int32>.size))
        // AF41 is a best-effort Wi-Fi/WMM hint for interactive video.
        var videoTos: Int32 = 0x88
        _ = setsockopt(sock, IPPROTO_IP, IP_TOS, &videoTos, socklen_t(MemoryLayout<Int32>.size))
        let originalFlags = fcntl(sock, F_GETFL, 0)
        if originalFlags >= 0 {
            _ = fcntl(sock, F_SETFL, originalFlags | O_NONBLOCK)
        }

        // A paired viewer may reach the control port through a Tailscale
        // subnet router even when both devices share Wi-Fi. Prove that the
        // physical media candidate owns its UDP port before any screen bytes
        // are captured or sent. The echoed nonce also authenticates reverse
        // IDR/BYE messages when their VPN source address differs.
        let token = Data(UUID().uuidString.utf8)
        var challenge = Data("LCH1".utf8)
        challenge.append(token)
        var challengeVerified = false
        let requiresSplitPair = requestedEncoderExperiment == .splitVertical
        var verifiedTilePorts = Set<UInt16>()
        var descriptor = pollfd(fd: sock, events: Int16(POLLIN), revents: 0)
        for attempt in 0..<60 {
            if attempt % 4 == 0 {
                _ = sendToViewer(challenge, fd: sock)
                if requiresSplitPair {
                    _ = sendToTile(challenge, side: .right, fd: sock)
                }
            }
            descriptor.revents = 0
            guard poll(&descriptor, 1, 50) > 0 else { continue }
            var response = [UInt8](repeating: 0, count: 256)
            var source = sockaddr_in()
            var sourceLength = socklen_t(MemoryLayout<sockaddr_in>.size)
            let count = response.withUnsafeMutableBytes { raw in
                withUnsafeMutablePointer(to: &source) { pointer in
                    pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                        recvfrom(
                            sock,
                            raw.baseAddress,
                            raw.count,
                            MSG_DONTWAIT,
                            socketAddress,
                            &sourceLength
                        )
                    }
                }
            }
            if count == challenge.count,
               Data(response[0..<count]) == challenge {
                let sourcePort = UInt16(bigEndian: source.sin_port)
                if sourcePort == targetPort {
                    verifiedTilePorts.insert(targetPort)
                } else if requiresSplitPair,
                          targetPort < UInt16.max,
                          sourcePort == targetPort + 1 {
                    verifiedTilePorts.insert(targetPort + 1)
                }
                challengeVerified = requiresSplitPair
                    ? verifiedTilePorts.count == 2
                    : verifiedTilePorts.contains(targetPort)
                if challengeVerified { break }
            }
        }
        guard challengeVerified else {
            close(sock)
            sock = -1
            let reason = requiresSplitPair
                ? "dual-port UDP reachability proof failed for \(targetLabel)"
                : "UDP reachability proof failed for \(targetLabel)"
            if stopOnFailure {
                setLastError(reason)
                markStopped(reason)
            }
            return false
        }
        viewerControlToken = token
        inputLock.lock()
        lastReliableInputSequence = 0
        lastPointerInputSequence = 0
        inputLock.unlock()
        startInputReceiver(fd: sock)
        sendInputStatus(fd: sock)

        stateLock.lock()
        if stopRequested {
            stateLock.unlock()
            close(sock)
            sock = -1
            return false
        }
        lifecycleState = firstSendNs == nil ? "starting_capture" : "running"
        stateLock.unlock()
        return true
    }

    /// Remote input is opt-in per stream. Reliable packets continue to be
    /// acknowledged while disabled so a viewer cannot build an unbounded
    /// retry queue before the host grants control.
}
