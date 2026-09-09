use super::*;

#[derive(Clone)]
pub(super) struct FramePacket {
    pub(super) id: u16,
    pub(super) au: Vec<u8>,
    pub(super) capture_wall_ms: Option<u64>,
    pub(super) encode_wall_ms: Option<u64>,
    pub(super) send_wall_ms: Option<u64>,
}

pub(super) fn queue_reassembled_frame(
    frame_sequencer: &mut CompletedFrameSequencer,
    completed_frames: &mut [Option<(std::net::SocketAddr, FramePacket)>],
    completed_count: &mut usize,
    peer: std::net::SocketAddr,
    reassembled: ReassembledFrame,
) {
    for completed in frame_sequencer.push(reassembled) {
        let frame = FramePacket {
            id: completed.id,
            au: completed.au,
            capture_wall_ms: completed.capture_wall_ms,
            encode_wall_ms: completed.encode_wall_ms,
            send_wall_ms: Some(completed.send_wall_ms),
        };
        if *completed_count < completed_frames.len() {
            completed_frames[*completed_count] = Some((peer, frame));
            *completed_count += 1;
        }
    }
}

pub(super) fn queue_expired_frames(
    frame_sequencer: &mut CompletedFrameSequencer,
    completed_frames: &mut [Option<(std::net::SocketAddr, FramePacket)>],
    completed_count: &mut usize,
    peer: std::net::SocketAddr,
) {
    for completed in frame_sequencer.drain_expired() {
        if *completed_count >= completed_frames.len() {
            break;
        }
        completed_frames[*completed_count] = Some((
            peer,
            FramePacket {
                id: completed.id,
                au: completed.au,
                capture_wall_ms: completed.capture_wall_ms,
                encode_wall_ms: completed.encode_wall_ms,
                send_wall_ms: Some(completed.send_wall_ms),
            },
        ));
        *completed_count += 1;
    }
}

pub(super) fn queue_restored_fragments(
    restored: Option<Vec<RestoredFragment>>,
    reassembler: &mut FrameReassembler,
    frame_sequencer: &mut CompletedFrameSequencer,
    completed_frames: &mut [Option<(std::net::SocketAddr, FramePacket)>],
    completed_count: &mut usize,
    peer: std::net::SocketAddr,
) {
    for recovered in restored.into_iter().flatten() {
        let fragment = FrameFragment {
            index: recovered.index,
            count: recovered.count,
            id: recovered.id,
            capture_wall_ms: recovered.capture_wall_ms,
            encode_wall_ms: recovered.encode_wall_ms,
            send_wall_ms: recovered.send_wall_ms,
            payload: &recovered.payload,
        };
        if let Some(reassembled) = reassembler.push(fragment) {
            queue_reassembled_frame(
                frame_sequencer,
                completed_frames,
                completed_count,
                peer,
                reassembled,
            );
        }
    }
}

#[derive(Default)]
pub(super) struct RendererStats {
    pub(super) queued: u64,
    pub(super) input_drops: u64,
    pub(super) frame_gaps: u64,
    pub(super) intentional_live_edge_gaps: u64,
    pub(super) recovery_skipped_frames: u64,
    pub(super) max_feed_us: u64,
    pub(super) stale_inputs: u64,
    pub(super) completed_batch: usize,
    pub(super) live_edge_batch: usize,
    pub(super) max_completed_batch: usize,
    pub(super) pressure: ReceiverPressure,
    pub(super) consecutive_stale: u32,
    // NTP-style authenticated probes estimate Host clock minus Android clock.
    // Do not infer this from the first video frame: that would erase the very
    // one-way delivery latency the HUD is intended to show.
    pub(super) host_clock_offset_ms: Option<i128>,
    // When the last latency-probe response arrived. Silence here means the
    // RTT/stage estimates are stale and the HUD must show "unmeasured"
    // instead of a frozen number.
    pub(super) last_probe_received: Option<std::time::Instant>,
}

pub(super) const LATENCY_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
pub(super) const RESIZE_RECOVERY_SUPPRESSION_US: u64 = 350_000;
/// Probe responses arriving after this silence window leave the smoothed
/// latency estimates stale; decay them toward "unknown" instead of freezing.
pub(super) const PROBE_STALE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

#[allow(clippy::too_many_arguments)]
pub(super) fn queue_parity_packet(
    packet: &[u8],
    peer: std::net::SocketAddr,
    fec_groups: &mut HashMap<(u16, u16), FecGroup>,
    fec_group_order: &mut VecDeque<(u16, u16)>,
    completed_fec_groups: &mut CompletedFecGroups,
    reassembler: &mut FrameReassembler,
    frame_sequencer: &mut CompletedFrameSequencer,
    completed_frames: &mut [Option<(std::net::SocketAddr, FramePacket)>; MEDIA_BATCH_SIZE],
    completed_count: &mut usize,
    renderer_stats: &mut RendererStats,
) -> bool {
    if packet.first() == Some(&PARITY_MARKER) {
        let Some(parity) = parse_parity(packet) else {
            return true;
        };
        let key = (parity.id, parity.base);
        if completed_fec_groups.contains(&key) {
            return true;
        }
        if !fec_groups.contains_key(&key) {
            while fec_groups.len() >= crate::media_datagram::MAX_ACTIVE_FEC_GROUPS {
                let Some(oldest) = fec_group_order.pop_front() else {
                    break;
                };
                if let Some(group) = fec_groups.remove(&oldest) {
                    renderer_stats
                        .pressure
                        .record_unrecoverable_group(group.missing_data_fragments());
                }
            }
            let Some(group) = FecGroup::new(parity.id, parity.k, parity.base, parity.total) else {
                return true;
            };
            fec_groups.insert(key, group);
            fec_group_order.push_back(key);
        }
        let mut restored = None;
        let mut complete = false;
        if let Some(group) = fec_groups.get_mut(&key) {
            restored = group.push_parity_and_restore(parity);
            complete = group.is_complete();
        }
        if let Some(restored) = restored.as_ref() {
            renderer_stats.pressure.record_fec_recovery(restored.len());
        }
        queue_restored_fragments(
            restored,
            reassembler,
            frame_sequencer,
            completed_frames,
            completed_count,
            peer,
        );
        if complete {
            fec_groups.remove(&key);
            fec_group_order.retain(|queued| *queued != key);
            completed_fec_groups.remember(key);
        }
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub(super) fn queue_fragment_packet(
    packet: &[u8],
    peer: std::net::SocketAddr,
    fec_groups: &mut HashMap<(u16, u16), FecGroup>,
    fec_group_order: &mut VecDeque<(u16, u16)>,
    completed_fec_groups: &mut CompletedFecGroups,
    reassembler: &mut FrameReassembler,
    frame_sequencer: &mut CompletedFrameSequencer,
    completed_frames: &mut [Option<(std::net::SocketAddr, FramePacket)>; MEDIA_BATCH_SIZE],
    completed_count: &mut usize,
    renderer_stats: &mut RendererStats,
) {
    let Some(fragment) = parse_fragment(packet) else {
        return;
    };
    let group_base = (fragment.index / 8) * 8;
    let group_k = (fragment.count - group_base).min(8);
    let group_key = (fragment.id, group_base);
    let group_already_complete = completed_fec_groups.contains(&group_key);
    if group_k > 1 && !group_already_complete && !fec_groups.contains_key(&group_key) {
        while fec_groups.len() >= crate::media_datagram::MAX_ACTIVE_FEC_GROUPS {
            let Some(oldest) = fec_group_order.pop_front() else {
                break;
            };
            if let Some(group) = fec_groups.remove(&oldest) {
                renderer_stats
                    .pressure
                    .record_unrecoverable_group(group.missing_data_fragments());
            }
        }
        if let Some(group) = FecGroup::new(fragment.id, group_k as u8, group_base, fragment.count) {
            fec_groups.insert(group_key, group);
            fec_group_order.push_back(group_key);
        }
    }
    let mut restored = None;
    if !group_already_complete {
        if let Some(group) = fec_groups.get_mut(&group_key) {
            restored = group.push_data_and_restore(fragment);
            if group.is_complete() {
                fec_groups.remove(&group_key);
                fec_group_order.retain(|queued| *queued != group_key);
                completed_fec_groups.remember(group_key);
            }
        }
    }
    if let Some(restored) = restored.as_ref() {
        renderer_stats.pressure.record_fec_recovery(restored.len());
    }
    queue_restored_fragments(
        restored,
        reassembler,
        frame_sequencer,
        completed_frames,
        completed_count,
        peer,
    );
    let Some(reassembled) = reassembler.push(fragment) else {
        return;
    };
    queue_reassembled_frame(
        frame_sequencer,
        completed_frames,
        completed_count,
        peer,
        reassembled,
    );
}
