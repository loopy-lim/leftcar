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
        guard sourceAuthorization?.begin() ?? true else { return -1 }
        defer { sourceAuthorization?.end() }
        return data.withUnsafeBytes { raw in
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
    /// The 4-byte length prefix stays plaintext for framing; the payload is
    /// AEAD-sealed once here, at the lowest-level sender.
     func sendTCPFrame(_ data: Data, fd: Int32) -> Int {
        guard !data.isEmpty, let sealed = mediaCrypto.seal(data),
              isValidTcpMediaFrameLength(sealed.count) else { return -1 }
        var length = UInt32(sealed.count).bigEndian
        var framed = Data(capacity: sealed.count + 4)
        withUnsafeBytes(of: &length) { framed.append(contentsOf: $0) }
        framed.append(sealed)
        tcpWriteLock.lock()
        defer { tcpWriteLock.unlock() }
        return sendTCPBytes(framed, fd: fd) == framed.count ? data.count : -1
    }

    /// Lowest-level UDP sender. Sealing happens exactly here so every
    /// datagram this session emits (video, audio, cursor, acks, notices,
    /// reachability challenge) shares one AEAD boundary. Sealing adds 24
    /// wire bytes (8B counter + 16B tag), so the raw sendto count is
    /// normalized through `normalizedSendResult` before returning.
     func send(_ data: Data, fd: Int32, to addr: inout sockaddr_in) -> Int {
        guard sourceAuthorization?.begin() ?? true else { return -1 }
        defer { sourceAuthorization?.end() }
        guard let sealed = mediaCrypto.seal(data) else { return -1 }
        let result = sealed.withUnsafeBytes { raw in
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
        return normalizedSendResult(
            result,
            sealedCount: sealed.count,
            plaintextCount: data.count
        )
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
        // Record the plaintext envelope before sealing: the viewer may ask
        // for this exact datagram again via NAK. Only DATA (`G`) fragments
        // pass the ring's marker filter — parity, config, audio, and control
        // never retransmit. On the TCP transport the ring is harmless (TCP
        // does not lose datagrams) and the NAK reply re-seals over TCP.
        retransmitRing.store(data, side: tileSide)
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

        // Reachability proof: a random nonce sealed under the session media
        // key. Only a viewer holding the key can open it and echo the same
        // plaintext sealed in its own direction — the nonce itself is not a
        // secret and the token-suffix scheme it replaced is gone.
        let challenge = sealedChallenge()
        guard sendTCPFrame(challenge, fd: sock) == challenge.count,
              let response = receiveTCPFrame(fd: sock, timeoutMs: 2_000),
              let opened = mediaCrypto.open(response),
              opened == challenge else {
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
        return finishMediaSocketHandshake(fd: sock)
    }

    /// 미디어 소켓이 인증을 통과한 뒤의 공통 마무리: 입력 시퀀스 리셋, 입력
    /// 수신기 시작, 입력 상태 전송, 중지 검사, 라이프사이클 전환.
    func finishMediaSocketHandshake(fd: Int32) -> Bool {
        inputLock.lock()
        lastReliableInputSequence = 0
        lastPointerInputSequence = 0
        inputLock.unlock()
        startInputReceiver(fd: fd)
        sendInputStatus(fd: fd)

        stateLock.lock()
        if stopRequested {
            stateLock.unlock()
            close(fd)
            sock = -1
            return false
        }
        lifecycleState = firstSendNs == nil ? "starting_capture" : "running"
        stateLock.unlock()
        return true
    }

    /// LCH1 도전 본문: 세션 미디어 키로 봉인하는 랜덤 논스. 논스 자체는
    /// 비밀이 아니고, 열어서 같은 평문을 역방향으로 봉인해 돌려보낼 수
    /// 있다는 AEAD 보유가 인증이다.
    func sealedChallenge() -> Data {
        var challenge = Data("LCH1".utf8)
        challenge.append(Data(UUID().uuidString.utf8))
        return challenge
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
        // are captured or sent. The challenge is sealed under the session
        // media key and the echo must open to the identical plaintext; AEAD
        // possession authenticates every later reverse message.
        let challenge = sealedChallenge()
        var challengeVerified = false
        let requiresSplitPair = requestedEncoderExperiment == .splitVertical
        var verifiedTilePorts = Set<UInt16>()
        var descriptor = pollfd(fd: sock, events: Int16(POLLIN), revents: 0)
        leftcarPerformanceLogger.notice("reachability proof: sealed challenge -> \(self.targetLabel, privacy: .public)")
        for attempt in 0..<60 {
            if attempt % 4 == 0 {
                let sent = sendToViewer(challenge, fd: sock)
                if attempt == 0 {
                    leftcarPerformanceLogger.notice("reachability proof: challenge send=\(sent, privacy: .public)B challenge=\(challenge.count, privacy: .public)B")
                }
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
            // open은 카운터를 소모하므로 반드시 한 번만 한다.
            let opened = count > 0 ? mediaCrypto.open(Data(response[0..<count])) : nil
            if count > 0 {
                leftcarPerformanceLogger.notice("reachability proof: response \(count, privacy: .public)B openOk=\(opened != nil, privacy: .public)")
            }
            if let opened = opened,
               opened == challenge {
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
        return finishMediaSocketHandshake(fd: sock)
    }

    /// Remote input is opt-in per stream. Reliable packets continue to be
    /// acknowledged while disabled so a viewer cannot build an unbounded
    /// retry queue before the host grants control.
}
