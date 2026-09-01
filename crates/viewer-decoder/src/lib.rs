//! Real hardware H.264 decoder via Android NDK `AMediaCodec` (H07).
//!
//! Direct `libmediandk` linkage — no Kotlin, no MediaCodec Java API. Input:
//! Annex-B access units (SPS/PPS/IDR/delta) from the assembler. Output: frames
//! rendered to the attached `ANativeWindow` (Surface). On host (non-Android)
//! builds the externs are not linked; tests cover the pure-Rust parser.

#![allow(non_camel_case_types)]

mod android;
mod annex_b;

pub use android::*;
pub use annex_b::*;

// -- host-side tests (pure parser) ----------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn au(parts: &[&[u8]]) -> Vec<u8> {
        let mut v = Vec::new();
        for p in parts {
            v.extend_from_slice(&[0, 0, 0, 1]);
            v.extend_from_slice(p);
        }
        v
    }

    #[test]
    fn split_annexb_finds_three_and_four_byte_start_codes() {
        let mut buf = vec![0, 0, 1, 0x67, 0xAA]; // 3-byte SC + SPS-ish
        buf.extend_from_slice(&[0, 0, 0, 1, 0x68, 0xBB]); // 4-byte SC + PPS-ish
        let nals = split_annexb(&buf);
        assert_eq!(nals.len(), 2);
        assert_eq!(nal_type(nals[0].bytes), Some(7));
        assert_eq!(nal_type(nals[1].bytes), Some(8));
        assert_eq!(nals[0].bytes, &[0x67, 0xAA]);
        assert_eq!(nals[1].bytes, &[0x68, 0xBB]);
    }

    #[test]
    fn split_annexb_empty_and_garbage() {
        assert!(split_annexb(&[]).is_empty());
        assert!(split_annexb(&[1, 2, 3]).is_empty());
        // start code at end with no payload -> no NAL
        assert!(split_annexb(&[0, 0, 1]).is_empty());
    }

    #[test]
    fn extract_config_returns_sps_pps_with_start_codes() {
        let key = au(&[&[0x67, 0x64], &[0x68, 0x1F]]);
        let (sps, pps) = extract_config(&[&key]).unwrap();
        assert_eq!(&sps[..4], &[0, 0, 0, 1]);
        assert_eq!(sps[4], 0x67);
        assert_eq!(pps[4], 0x68);
    }

    #[test]
    fn decoder_max_frame_size_prefers_explicit_source_bound() {
        assert_eq!(
            decoder_max_frame_size(Some((3840, 2160)), 2560, 1440),
            Some((3840, 2160))
        );
    }

    #[test]
    fn decoder_max_frame_size_without_bound_is_none() {
        assert_eq!(decoder_max_frame_size(None, 2560, 1440), None);
    }

    #[test]
    fn decoder_max_frame_size_covers_active_when_bound_is_smaller() {
        // A malformed bound below the active frame must never clip the
        // currently decodable size; the max is the union of both.
        assert_eq!(
            decoder_max_frame_size(Some((1280, 720)), 2560, 1440),
            Some((2560, 1440))
        );
    }

    #[test]
    fn extract_config_none_without_sps_pps() {
        let delta = au(&[&[0x41, 0x9A]]);
        assert!(extract_config(&[&delta]).is_none());
    }

    #[test]
    fn nal_type_masks_low_five_bits() {
        // 0x65 = IDR with ref/idc bits; 0x41 = non-IDR slice
        assert_eq!(nal_type(&[0x65]), Some(NAL_IDR));
        assert_eq!(nal_type(&[0x41]), Some(NAL_NON_IDR));
    }

    #[test]
    fn parses_cf2_hevc_vps_sps_pps_configuration() {
        let mut packet = b"CF2".to_vec();
        packet.push(VideoCodec::Hevc.id());
        for nal in [
            &[0, 0, 0, 1, 0x40, 0x01, 0xAA][..],
            &[0, 0, 0, 1, 0x42, 0x01, 0xBB][..],
            &[0, 0, 0, 1, 0x44, 0x01, 0xCC][..],
        ] {
            packet.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            packet.extend_from_slice(nal);
        }

        let config = parse_codec_config(&packet).expect("valid HEVC config");
        assert_eq!(config.codec, VideoCodec::Hevc);
        assert_eq!(config.vps.as_deref(), Some(&packet[8..15]));
        assert_eq!(config.sps.as_deref(), Some(&packet[19..26]));
        assert_eq!(config.pps.as_deref(), Some(&packet[30..37]));
    }

    #[test]
    fn parses_legacy_cfg_as_h264_sps_pps_configuration() {
        let mut packet = b"CFG".to_vec();
        let sps = [0, 0, 0, 1, 0x67, 0x64];
        let pps = [0, 0, 0, 1, 0x68, 0x1F];
        for nal in [&sps[..], &pps[..]] {
            packet.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            packet.extend_from_slice(nal);
        }

        let config = parse_codec_config(&packet).expect("valid legacy config");
        assert_eq!(config.codec, VideoCodec::H264);
        assert!(config.vps.is_none());
        assert_eq!(config.sps.as_deref(), Some(sps.as_slice()));
        assert_eq!(config.pps.as_deref(), Some(pps.as_slice()));
    }

    #[test]
    fn rejects_incomplete_or_unknown_codec_configuration() {
        let mut incomplete = b"CF2".to_vec();
        incomplete.push(VideoCodec::Hevc.id());
        incomplete.extend_from_slice(&3u32.to_be_bytes());
        incomplete.extend_from_slice(&[0, 0, 0]);
        assert!(parse_codec_config(&incomplete).is_none());

        let unknown = [b'C', b'F', b'2', 99];
        assert!(parse_codec_config(&unknown).is_none());
    }

    #[test]
    fn repeated_codec_config_does_not_require_decoder_recreation() {
        let config = CodecConfig {
            codec: VideoCodec::H264,
            vps: None,
            sps: Some(vec![0, 0, 0, 1, 0x67]),
            pps: Some(vec![0, 0, 0, 1, 0x68]),
        };
        assert!(config.requires_decoder_reset(None));
        assert!(!config.requires_decoder_reset(Some(&config)));

        let mut changed = config.clone();
        changed.pps.as_mut().unwrap().push(0x01);
        assert!(changed.requires_decoder_reset(Some(&config)));
    }

    #[test]
    fn strict_named_decoder_plan_never_falls_back_to_mime_selection() {
        assert_eq!(
            decoder_candidate_plan(Some("c2.vendor.avc.decoder"), false),
            vec![DecoderCandidate::Named("c2.vendor.avc.decoder")]
        );
        assert!(decoder_candidate_plan(None, false).is_empty());
        assert_eq!(
            decoder_candidate_plan(Some("c2.vendor.avc.decoder"), true),
            vec![
                DecoderCandidate::Named("c2.vendor.avc.decoder"),
                DecoderCandidate::MimeType,
            ]
        );
    }

    #[test]
    fn exposes_codec_mime_and_parameter_set_contract() {
        assert_eq!(VideoCodec::H264.mime(), "video/avc");
        assert_eq!(VideoCodec::Hevc.mime(), "video/hevc");
        assert_eq!(VideoCodec::H264.parameter_set_count(), 2);
        assert_eq!(VideoCodec::Hevc.parameter_set_count(), 3);
        assert_eq!(VideoCodec::H264.csd_names(), &["csd-0", "csd-1"]);
        assert_eq!(VideoCodec::Hevc.csd_names(), &["csd-0", "csd-1", "csd-2"]);
        assert_eq!(hevc_nal_type(&[0x40, 0x01]), Some(32));
    }

    #[test]
    fn frame_id_gap_requires_decoder_resync() {
        assert!(frame_id_is_next(41, 42));
        assert!(frame_id_is_next(u16::MAX, 0));
        assert!(!frame_id_is_next(41, 43));
        assert!(!frame_id_is_next(41, 41));
        assert_eq!(frame_id_missing_count(41, 42), 0);
        assert_eq!(frame_id_missing_count(41, 43), 1);
        assert_eq!(frame_id_missing_count(u16::MAX, 1), 1);
    }

    #[test]
    fn au_without_trailing_zero_no_nal_split_bug() {
        // regression guard: a NAL containing 00 00 01 inside payload? Not
        // valid in real streams (emulation prevention); parser treats found
        // start codes as boundaries — documented behavior.
        let buf = au(&[&[0x65, 0x00, 0x00, 0x01, 0x99]]);
        let nals = split_annexb(&buf);
        assert_eq!(
            nals.len(),
            2,
            "start-code-looking payload splits (documented)"
        );
    }
}

/// Configure-time debug: format keys AMediaCodec expects for AVC decode.
/// Kept public for the harness to cross-check.
pub const FORMAT_KEY_MIME: &str = "mime";
pub const FORMAT_KEY_CSD0: &str = "csd-0";
pub const FORMAT_KEY_CSD1: &str = "csd-1";
pub const FORMAT_KEY_WIDTH: &str = "width";
pub const FORMAT_KEY_HEIGHT: &str = "height";

#[cfg(test)]
mod sps_tests {
    use super::*;

    /// Real VideoToolbox-generated SPS for 320x240 baseline (level 20):
    /// full NAL with header byte 0x27, as split_annexb yields it.
    const VT_SPS_320X240: &[u8] = &[0x27, 0x42, 0x00, 0x14, 0xab, 0x40, 0xa0, 0xfc];
    /// Same SPS with an Annex-B start code, as the harness passes it.
    const VT_SPS_WITH_SC: &[u8] = &[
        0x00, 0x00, 0x00, 0x01, 0x27, 0x42, 0x00, 0x14, 0xab, 0x40, 0xa0, 0xfc,
    ];

    #[test]
    fn parses_videotoolbox_320x240_with_start_code() {
        // regression: the device run showed 32x1280 when the header byte was
        // parsed as profile_idc — this is the exact on-device input shape
        let (w, h) = parse_sps_dimensions(VT_SPS_WITH_SC).expect("parses");
        assert_eq!((w, h), (320, 240));
    }

    #[test]
    fn parses_videotoolbox_320x240() {
        let (w, h) = parse_sps_dimensions(VT_SPS_320X240).expect("parses");
        assert_eq!((w, h), (320, 240));
    }
}
