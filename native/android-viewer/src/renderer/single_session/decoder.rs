use super::*;

pub(super) fn is_keyframe(au: &[u8], codec: viewer_decoder::VideoCodec) -> bool {
    viewer_decoder::split_annexb(au)
        .iter()
        .any(|nal| match codec {
            viewer_decoder::VideoCodec::H264 => {
                viewer_decoder::nal_type(nal.bytes) == Some(viewer_decoder::NAL_IDR)
            }
            viewer_decoder::VideoCodec::Hevc => viewer_decoder::hevc_nal_type(nal.bytes)
                .is_some_and(|nal_type| matches!(nal_type, 19..=21)),
        })
}

pub(super) fn reset_decoder(
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    codec_config: &mut Option<viewer_decoder::CodecConfig>,
    awaiting_keyframe: &mut bool,
) {
    if let Some(d) = decoder.as_mut() {
        d.stop();
    }
    *decoder = None;
    *codec_config = None;
    *awaiting_keyframe = true;
}

/// A missing encoded AU invalidates the reference chain of subsequent delta
/// frames. Flush the codec before waiting for the next IDR so any queued
/// output that was decoded from the damaged chain cannot keep the Surface in a
/// corrupted state.
pub(super) fn resync_decoder_after_frame_gap(
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    awaiting_keyframe: &mut bool,
) {
    if let Some(Err(error)) = decoder.as_mut().map(|decoder| decoder.flush()) {
        log_info!(
            "decoder flush during recovery failed: {}; rebuilding",
            error
        );
        if let Some(mut decoder) = decoder.take() {
            decoder.stop();
        }
    }
    *awaiting_keyframe = true;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FeedOutcome {
    Queued,
    ResyncRequired,
    FatalError,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_codec_config_packet(
    packet: &[u8],
    window_handle: usize,
    width: u32,
    height: u32,
    fps: u32,
    decoder: &mut Option<viewer_decoder::AndroidDecoder>,
    codec_config: &mut Option<viewer_decoder::CodecConfig>,
    awaiting_keyframe: &mut bool,
) -> bool {
    if packet.starts_with(b"CFG") || packet.starts_with(b"CF2") {
        let marker = if packet.starts_with(b"CF2") {
            "CF2"
        } else {
            "CFG"
        };
        log_info!("Received {} datagram ({} bytes)", marker, packet.len());
        let Some(config) = viewer_decoder::parse_codec_config(packet) else {
            log_info!("Ignoring malformed {} codec configuration", marker);
            *awaiting_keyframe = true;
            return true;
        };
        if decoder.is_none() {
            let sps = config.sps.as_deref().expect("complete codec config SPS");
            let pps = config.pps.as_deref().expect("complete codec config PPS");
            let codec_name = match config.codec {
                viewer_decoder::VideoCodec::H264 => "c2.qti.avc.decoder.low_latency",
                viewer_decoder::VideoCodec::Hevc => "c2.qti.hevc.decoder.low_latency",
            };
            log_info!(
                "Creating {:?} AndroidDecoder with Surface window=0x{:x} vps={}B sps={}B pps={}B",
                config.codec,
                window_handle,
                config.vps.as_ref().map_or(0, Vec::len),
                sps.len(),
                pps.len()
            );
            let created = unsafe {
                viewer_decoder::AndroidDecoder::new_video_named(
                    viewer_decoder::VideoDecoderConfig {
                        codec: config.codec,
                        vps: config.vps.as_deref(),
                        sps,
                        pps,
                        width,
                        height,
                        window: window_handle,
                        fps,
                        codec_name: Some(codec_name),
                        allow_mime_fallback: true,
                    },
                )
            };
            match created {
                Ok(d) => {
                    log_info!(
                        "AndroidDecoder created successfully: codec={:?} actualCodec={}",
                        config.codec,
                        d.codec_name()
                    );
                    *decoder = Some(d);
                    *codec_config = Some(config);
                }
                Err(e) => {
                    log_info!("AndroidDecoder creation FAILED: {}", e);
                }
            }
        }
        // CFG/CF2 is emitted with an IDR on the host. Do not feed
        // delta frames until that recovery keyframe arrives.
        *awaiting_keyframe = true;
        return true;
    }
    false
}
