import Foundation
import CoreMedia
import Darwin

extension CaptureSession {
    private func recordAudioMetrics(codec: String, encoder: OpusAudioEncoder? = nil) {
        audioMetricsLock.lock()
        audioMetrics = [
            "audioRequestedCodec": audioOpusRequested ? "opus128k" : "pcm",
            "audioEffectiveCodec": codec,
            "audioFallback": audioOpusRequested && codec == "pcm",
            "audioTargetBitrate": encoder == nil ? NSNull() : NSNumber(value: 128000),
            "audioEncodedPackets": encoder.map { NSNumber(value: $0.encodedPackets) } ?? NSNull(),
            "audioEncodedBytes": encoder.map { NSNumber(value: $0.encodedBytes) } ?? NSNull(),
            "audioEncodeMeanUs": encoder.flatMap { $0.encodedPackets > 0 ? NSNumber(value: Double($0.encodeNanoseconds) / Double($0.encodedPackets) / 1000) : nil } ?? NSNull(),
            "audioEncoderPreSkipFrames": encoder.map { NSNumber(value: $0.preSkip) } ?? NSNull(),
            "audioAvSkewUs": NSNull(),
        ]
        audioMetricsLock.unlock()
    }

    /// One datagram carries at most 300 stereo frames (1200 B of PCM), so an
    /// LCAU packet stays comfortably under the media MTU and a lost datagram
    /// is a ~6ms skip instead of a broken stream.
    static let audioMaxFramesPerDatagram = 300

    /// Wire layout shared with the Android receiver:
    /// `LCAU | sequence:u16 BE | rate:u16 BE | channels:u8 | rsv:u8
    ///   | frames:u16 BE | int16 LE interleaved PCM`.
    static func audioDatagram(
        sequence: UInt16,
        sampleRate: Int,
        channels: Int,
        pcm: ArraySlice<UInt8>
    ) -> Data {
        var data = Data("LCAU".utf8)
        withUnsafeBytes(of: sequence.bigEndian) { data.append(contentsOf: $0) }
        withUnsafeBytes(of: UInt16(clamping: sampleRate).bigEndian) {
            data.append(contentsOf: $0)
        }
        data.append(UInt8(clamping: channels))
        data.append(0)
        let frames = pcm.count / (channels * 2)
        withUnsafeBytes(of: UInt16(clamping: frames).bigEndian) {
            data.append(contentsOf: $0)
        }
        data.append(contentsOf: pcm)
        return data
    }

    /// Convert interleaved Float32 samples to clamped int16 little-endian.
    static func int16LeAudio(_ interleaved: [Float32]) -> [UInt8] {
        var out = [UInt8](repeating: 0, count: interleaved.count * 2)
        for (index, sample) in interleaved.enumerated() {
            let clamped = max(-1.0, min(1.0, sample))
            let scaled = Int16((clamped * 32_767.0).rounded())
            out[index * 2] = UInt8(truncatingIfNeeded: scaled)
            out[index * 2 + 1] = UInt8(truncatingIfNeeded: scaled >> 8)
        }
        return out
    }

    /// Convert planar Float32 channel samples to interleaved int16 LE.
    static func int16LeAudio(planar: [[Float32]]) -> [UInt8] {
        guard let channels = planar.first?.count else { return [] }
        var interleaved = [Float32](repeating: 0, count: channels * planar.count)
        for (frame, channelSamples) in planar.enumerated() {
            for (channel, sample) in channelSamples.enumerated() where channel < channels {
                interleaved[frame * channels + channel] = sample
            }
        }
        return int16LeAudio(interleaved)
    }

    /// System audio captured alongside the video plane. ScreenCaptureKit
    /// delivers Float32 CMSampleBuffers; they are converted to int16 LE and
    /// chunked straight onto the media transport — no codec, no queueing,
    /// one datagram per chunk. The serial audioQueue owns the sequence, so
    /// no additional lock domain is needed.
     func handleAudioSampleBuffer(_ sampleBuffer: CMSampleBuffer) {
        // audioSequence wraps (UInt16) roughly every six minutes at 48kHz;
        // gate the one-shot logs on their own flag instead.
        let isFirstAudioBuffer = !loggedFirstAudioCallback
        if isFirstAudioBuffer {
            loggedFirstAudioCallback = true
            NSLog("Leftcar first audio callback for %@", targetLabel)
        }
        stateLock.lock()
        let fd = sock
        let stopped = stopRequested
        stateLock.unlock()
        guard fd >= 0, !stopped,
              shouldCaptureSystemAudio(owner: isSystemAudioOwner(), viewerKey: viewerAddressKey) else {
            opusAudioEncoder = nil
            return
        }

        guard let format = CMSampleBufferGetFormatDescription(sampleBuffer),
              let description = CMAudioFormatDescriptionGetStreamBasicDescription(format)
        else { return }
        let asbd = description.pointee
        // ScreenCaptureKit delivers Float32 PCM; anything else is dropped
        // rather than misinterpreted.
        guard asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0,
              asbd.mBitsPerChannel == 32,
              asbd.mChannelsPerFrame >= 1, asbd.mChannelsPerFrame <= 2
        else { return }
        if isFirstAudioBuffer {
            NSLog(
                "Leftcar first audio buffer rate=%.0f channels=%d flags=%x",
                asbd.mSampleRate,
                Int(asbd.mChannelsPerFrame),
                UInt32(asbd.mFormatFlags)
            )
        }

        // Read the raw Float32 bytes straight off the sample buffer's block.
        // CoreMedia may hand back a non-contiguous block, so walk it by
        // offset; SCK audio is a single small contiguous region in practice.
        guard let dataBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }
        audioPCMStorage.samples.removeAll(keepingCapacity: true)
        var offset = 0
        var totalLength = 0
        var pointer: UnsafeMutablePointer<Int8>?
        var segmentLength = 0
        while CMBlockBufferGetDataPointer(
            dataBuffer,
            atOffset: offset,
            lengthAtOffsetOut: &segmentLength,
            totalLengthOut: &totalLength,
            dataPointerOut: &pointer
        ) == noErr, let pointer, segmentLength > 0 {
            let count = segmentLength / 4
            pointer.withMemoryRebound(to: Float32.self, capacity: count) { rebound in
                for index in 0..<count {
                    audioPCMStorage.samples.append(rebound[index])
                }
            }
            offset += segmentLength
        }
        let channels = Int(asbd.mChannelsPerFrame)
        let sampleCount = audioPCMStorage.samples.count
        guard sampleCount >= channels, sampleCount % channels == 0 else { return }
        let samples = audioPCMStorage.samples
        let pcm = audioPCMStorage.convert(samples: samples, channels: channels,
            planar: asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0)
        let wanted = systemAudioOpusPreferred(forKey: viewerAddressKey)
        if wanted != audioOpusRequested {
            audioOpusRequested = wanted
            audioOpusFailed = false
            opusAudioEncoder = nil
        }
        if wanted && !audioOpusFailed && asbd.mSampleRate == 48000 {
            do {
                if opusAudioEncoder?.channels != channels { opusAudioEncoder = try OpusAudioEncoder(channels: channels) }
                if let encoder = opusAudioEncoder {
                    for packet in try encoder.appendPCM(pcm) {
                        audioSequence &+= 1
                        var datagram = Data("LCO1".utf8)
                        func append<T>(_ value: T) { withUnsafeBytes(of: value) { datagram.append(contentsOf: $0) } }
                        append(audioSequence.bigEndian)
                        append(UInt16(48000).bigEndian)
                        datagram.append(UInt8(channels)); datagram.append(0)
                        append(UInt16(480).bigEndian)
                        append(encoder.epoch.bigEndian)
                        append(encoder.preSkip.bigEndian)
                        append(UInt16(packet.count).bigEndian)
                        datagram.append(packet)
                        _ = sendMediaDatagram(datagram, fd: fd)
                    }
                    recordAudioMetrics(codec: "opus128k", encoder: encoder)
                    return
                }
            } catch {
                // Latch for this preference lifetime: repeated capability
                // refreshes must not repeatedly construct a failing codec.
                audioOpusFailed = true
                opusAudioEncoder = nil
                NSLog("Leftcar Opus unavailable or failed; falling back to PCM: %@", String(describing: error))
            }
        } else {
            opusAudioEncoder = nil
        }

        recordAudioMetrics(codec: "pcm")
        let frameBytes = channels * 2
        let totalFrames = pcm.count / frameBytes
        var frameOffset = 0
        while frameOffset < totalFrames {
            let frames = min(
                CaptureSession.audioMaxFramesPerDatagram,
                totalFrames - frameOffset
            )
            audioSequence &+= 1
            let datagram = CaptureSession.audioDatagram(
                sequence: audioSequence,
                sampleRate: Int(asbd.mSampleRate),
                channels: channels,
                pcm: pcm[(frameOffset * frameBytes)..<((frameOffset + frames) * frameBytes)]
            )
            _ = sendMediaDatagram(datagram, fd: fd)
            frameOffset += frames
        }
    }
}
