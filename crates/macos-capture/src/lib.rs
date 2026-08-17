//! macOS ScreenCaptureKit capture facade (H16–H17).
//!
//! PROOF LEVEL: E3 (compile + structure). Real capture requires the system
//! permission picker and is validated in the device-lab phase (docs/08 H16).
//! This facade documents the exact production path; the `real` feature wires
//! ScreenCaptureKit once Swift/C ABI shims land.
//!
//! Production path (docs/02 §4.1, docs/03 §7.2):
//! ```text
//! SCContentSharingPicker
//!  -> SCContentFilter(desktopIndependentWindow | display)
//!  -> SCStream (per-source output handler, async)
//!  -> CMSampleBuffer validation (width/height/pixel format/timestamp)
//!  -> IOSurface/CVPixelBuffer zero-copy handle
//!  -> bounded channel into the encoder (no network I/O in the callback)
//! ```
//! Permission model: screen-recording TCC consent; deny/revoke/expiry are
//! normal states surfaced as `CapturePermissionState` (docs/02 §4.2).

use media_model::ports::{ApprovedSource, CaptureConfig, FrameSink, RawFrame};

/// Which ScreenCaptureKit entity to capture.
#[derive(Debug, Clone)]
pub enum CaptureTarget {
    /// desktopIndependentWindow: one app window, survives occlusion
    AppWindow { sc_content_id: u64 },
    /// full physical display
    Display { sc_display_id: u32 },
}

/// Approved reference produced by the system picker. The native id is opaque
/// here and never crosses into control-contract types (docs/04 §3).
#[derive(Debug, Clone)]
pub struct ApprovedMacSource {
    pub target: CaptureTarget,
    pub filter_persisted: bool,
}

/// Stream configuration knobs mapped from SCStreamConfiguration (docs/02 §4.1):
/// size, pixel format, color space, cursor, minimum frame interval, queue depth.
#[derive(Debug, Clone)]
pub struct ScStreamTuning {
    pub queue_depth: usize,
    pub minimum_frame_interval_ns: u64,
    pub shows_cursor: bool,
    pub pixel_format: Ost,
}

/// Pixel formats we accept from SCStream output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ost {
    Bgra8,
    VideoToolbox420f,
}

/// Facade entry: start a capture stream for an approved source.
///
/// Callback rules (docs/03 §7.2): the SCStream output handler must not do
/// network I/O or heavy allocation — frames cross a bounded channel and the
/// handler returns immediately.
pub fn start_capture(
    _source: &ApprovedMacSource,
    _config: &CaptureConfig,
    _sink: &dyn FrameSink,
) -> Result<ActiveStream, CaptureStartError> {
    // Without the `real` feature there is no ScreenCaptureKit linkage; the
    // facade only proves the contract shape compiles and the structure tests
    // can reference the types.
    Err(CaptureStartError::RealBackendNotLinked {
        reason: "build with --features real after the Swift/C ABI shim lands (H16)",
    })
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureStartError {
    #[error("real backend not linked: {reason}")]
    RealBackendNotLinked { reason: &'static str },
    #[error("permission denied")]
    PermissionDenied,
    #[error("source unavailable")]
    SourceUnavailable,
}

/// Handle to a running SCStream.
pub struct ActiveStream {
    pub target: CaptureTarget,
}

impl ActiveStream {
    pub fn stop(&mut self) {
        // real impl: invalidate SCStream, release IOSurface refs
    }
}

/// Map an approved target to the shared ApprovedSource view.
pub fn to_approved_source(approved: &ApprovedMacSource, id: domain::SourceId) -> ApprovedSource {
    ApprovedSource {
        source_id: id,
        kind: match approved.target {
            CaptureTarget::Display { .. } => domain::SourceKind::Display,
            CaptureTarget::AppWindow { .. } => domain::SourceKind::Window,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_compiles_and_reports_unlinked_backend() {
        let approved = ApprovedMacSource {
            target: CaptureTarget::AppWindow { sc_content_id: 42 },
            filter_persisted: true,
        };
        struct NoopSink;
        #[async_trait::async_trait]
        impl FrameSink for NoopSink {
            async fn on_frame(&self, _frame: RawFrame) {}
        }
        let config = CaptureConfig {
            max_width: 1920,
            max_height: 1080,
            max_fps: 60,
            include_cursor: true,
        };
        let result = start_capture(&approved, &config, &NoopSink);
        // E3 scope: without the real feature, starting fails loudly (never a
        // silent software fallback — docs/01 출시 중단 조건).
        assert!(matches!(
            result,
            Err(CaptureStartError::RealBackendNotLinked { .. })
        ));
    }

    #[test]
    fn window_target_maps_to_window_kind() {
        let approved = ApprovedMacSource {
            target: CaptureTarget::AppWindow { sc_content_id: 1 },
            filter_persisted: true,
        };
        let source = to_approved_source(&approved, domain::SourceId::from_raw("s").unwrap());
        assert!(matches!(source.kind, domain::SourceKind::Window));
    }
}
