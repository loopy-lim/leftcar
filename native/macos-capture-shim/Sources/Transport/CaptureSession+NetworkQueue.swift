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
     func enqueuePacket(
        config: Data? = nil,
        frame: Data? = nil,
        isKeyframe: Bool = false,
        isRecoveryKeyframe: Bool = false
    ) {
        networkLock.lock()
        var shouldRequestRecoveryAfterUnlock = false
        if let config {
            pendingConfig = config
        }
        if let frame {
            if shouldPrioritizeNetworkKeyframe(isKeyframe: isKeyframe) {
                // A keyframe is an independent decoder boundary. Never make
                // it wait behind stale deltas that may already have filled
                // the pacing queue; once it is queued, the next delta either
                // follows this boundary or triggers a later recovery.
                pendingFrames.removeAll(keepingCapacity: true)
                networkRecoveryBoundary.setAwaitingKeyframe(networkAwaitingKeyframeAfterEnqueue(
                    currentAwaitingKeyframe: networkRecoveryBoundary.awaitingKeyframe,
                    isKeyframe: true,
                    isRecoveryKeyframe: isRecoveryKeyframe
                ))
                pendingFrames.append(
                    PendingEncodedFrame(
                        data: frame,
                        isKeyframe: true,
                        isRecoveryKeyframe: isRecoveryKeyframe
                    )
                )
            } else if networkRecoveryBoundary.awaitingKeyframe {
                if isKeyframe {
                    pendingFrames.removeAll(keepingCapacity: true)
                    networkRecoveryBoundary.setAwaitingKeyframe(networkAwaitingKeyframeAfterEnqueue(
                        currentAwaitingKeyframe: networkRecoveryBoundary.awaitingKeyframe,
                        isKeyframe: true,
                        isRecoveryKeyframe: isRecoveryKeyframe
                    ))
                    pendingFrames.append(
                        PendingEncodedFrame(
                            data: frame,
                            isKeyframe: true,
                            isRecoveryKeyframe: isRecoveryKeyframe
                        )
                    )
                } else {
                    stateLock.lock()
                    framesDropped &+= 1
                    networkQueueDropped &+= 1
                    recoveryFramesDropped &+= 1
                    stateLock.unlock()
                    shouldRequestRecoveryAfterUnlock = true
                }
            } else if pendingFrames.count < maxPendingNetworkFrames {
                pendingFrames.append(
                    PendingEncodedFrame(
                        data: frame,
                        isKeyframe: isKeyframe,
                        isRecoveryKeyframe: isRecoveryKeyframe
                    )
                )
            } else {
                // The unsent frames are a dependency chain. Once the bounded
                // queue is full, sending a newer delta while skipping any of
                // them would create visible corruption. Discard the whole
                // unsent chain and recover on the next independently decodable
                // IDR instead.
                let discarded = pendingFrames.count + (isKeyframe ? 0 : 1)
                let keyframeInFlight = networkKeyframeInFlight
                let queuedKeyframe = pendingFrames.last(where: { $0.isKeyframe })
                pendingFrames.removeAll(keepingCapacity: true)
                if let queuedKeyframe, !isKeyframe {
                    // Preserve an already queued recovery boundary. Dropping
                    // it would make the next deltas undecodable and request a
                    // second IDR burst for the same loss episode.
                    pendingFrames.append(queuedKeyframe)
                }
                stateLock.lock()
                if queuedKeyframe != nil && !isKeyframe {
                    let retained = discarded - 1
                    framesDropped &+= Int64(retained)
                    networkQueueDropped &+= Int64(retained)
                    recoveryFramesDropped &+= Int64(retained)
                } else {
                    framesDropped &+= Int64(discarded)
                    networkQueueDropped &+= Int64(discarded)
                    if keyframeInFlight || networkRecoveryBoundary.awaitingKeyframe {
                        recoveryFramesDropped &+= Int64(discarded)
                    }
                }
                stateLock.unlock()
                if isKeyframe {
                    networkRecoveryBoundary.setAwaitingKeyframe(networkAwaitingKeyframeAfterEnqueue(
                        currentAwaitingKeyframe: networkRecoveryBoundary.awaitingKeyframe,
                        isKeyframe: true,
                        isRecoveryKeyframe: isRecoveryKeyframe
                    ))
                    pendingFrames.append(
                        PendingEncodedFrame(
                            data: frame,
                            isKeyframe: true,
                            isRecoveryKeyframe: isRecoveryKeyframe
                        )
                    )
                } else {
                    // A keyframe already being written is the recovery
                    // boundary. Dropping deltas behind that boundary is
                    // expected; asking for another IDR here would create a
                    // second burst before the first one has arrived.
                    if shouldRecoverAfterNetworkOverflow(
                        incomingIsKeyframe: isKeyframe,
                        keyframeQueued: queuedKeyframe != nil,
                        keyframeInFlight: keyframeInFlight
                    ) {
                        networkRecoveryBoundary.establishAwaitingKeyframe()
                        shouldRequestRecoveryAfterUnlock = true
                    }
                }
            }
        }
        let schedule = !networkDrainScheduled
        if schedule {
            networkDrainScheduled = true
        }
        networkLock.unlock()

        if shouldRequestRecoveryAfterUnlock {
            requestRecoveryKeyframe()
        }
        if schedule {
            networkQueue.async { [weak self] in
                self?.drainNetwork()
            }
        }
    }

    func drainNetwork() {
        while true {
            networkLock.lock()
            let splitAccessUnit = pendingSplitAccessUnits.isEmpty
                ? nil
                : pendingSplitAccessUnits.removeFirst()
            let frame = splitAccessUnit == nil && !pendingFrames.isEmpty
                ? pendingFrames.removeFirst()
                : nil
            var configs = [(TileSide?, Data)]()
            if let side = frame?.tileSide,
               let tileConfig = pendingTileConfigs.removeValue(forKey: side) {
                configs.append((side, tileConfig))
            } else if let singleConfig = pendingConfig {
                pendingConfig = nil
                configs.append((nil, singleConfig))
            } else if let side = TileSide.allCases.first(where: {
                pendingTileConfigs[$0] != nil
            }), let tileConfig = pendingTileConfigs.removeValue(forKey: side) {
                configs.append((side, tileConfig))
            }
            let isKeyframe = splitAccessUnit?.isKeyframe == true || frame?.isKeyframe == true
            if isKeyframe {
                networkKeyframeInFlight = true
                // A recovery boundary must not inherit pacing debt from stale
                // deltas. Those bytes are no longer useful once the boundary
                // is available, and carrying their deadline directly turns a
                // short LAN burst into another latency spike.
                nextUdpSendNs = DispatchTime.now().uptimeNanoseconds
            }
            if configs.isEmpty && frame == nil && splitAccessUnit == nil {
                networkDrainScheduled = false
                networkLock.unlock()
                return
            }
            networkLock.unlock()

            // Config always precedes the next queued frame. The queue is
            // intentionally tiny and overflow is recovered by an IDR, so it
            // absorbs scheduler jitter without accumulating stale video.
            for (side, config) in configs {
                writePacket(config, tileSide: side)
            }
            if let splitAccessUnit {
                let result = writeSplitPacketPair(splitAccessUnit)
                networkLock.lock()
                if splitAccessUnit.isKeyframe {
                    networkRecoveryBoundary.setAwaitingKeyframe(networkAwaitingKeyframeAfterSend(
                        currentAwaitingKeyframe: networkRecoveryBoundary.awaitingKeyframe,
                        isKeyframe: true,
                        isRecoveryKeyframe: splitAccessUnit.isRecoveryKeyframe,
                        sendSucceeded: result.succeeded
                    ))
                }
                networkKeyframeInFlight = false
                networkLock.unlock()
                if result.succeeded {
                    _ = finishSplitFlowLease(splitAccessUnit.lease)
                    if splitAccessUnit.dropRightForTest {
                        beginSplitTransportRecovery(reason: "injected right AU loss")
                    }
                } else {
                    beginSplitTransportRecovery(
                        reason: "split pair send failed",
                        invalidatePendingBoundary: true
                    )
                }
            } else if let frame {
                let sendSucceeded = writePacket(
                    frame.data,
                    isFrame: true,
                    isKeyframe: frame.isKeyframe,
                    isRecoveryKeyframe: frame.isRecoveryKeyframe,
                    tileSide: frame.tileSide
                )
                networkLock.lock()
                if frame.isKeyframe {
                    networkRecoveryBoundary.setAwaitingKeyframe(networkAwaitingKeyframeAfterSend(
                        currentAwaitingKeyframe: networkRecoveryBoundary.awaitingKeyframe,
                        isKeyframe: true,
                        isRecoveryKeyframe: frame.isRecoveryKeyframe,
                        sendSucceeded: sendSucceeded
                    ))
                }
                networkKeyframeInFlight = false
                networkLock.unlock()
            }
        }
    }

    /// Consume nonce-authenticated viewer-to-host datagrams on a dedicated
    /// queue. Pointer motion is latest-wins; buttons and keys are accepted in
    /// sequence and acknowledged so the viewer can retry without duplicating
    /// host events.
}
