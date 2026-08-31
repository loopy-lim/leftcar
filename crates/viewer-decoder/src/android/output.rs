use super::decoder::{AndroidDecoder, DecoderError};
use super::ffi::*;

/// Interactive desktop streaming values freshness over displaying every
/// decoded image. Retaining one ready output bounds the final decoder-to-
/// Surface queue without dropping compressed reference inputs.
pub const MAX_RENDERABLE_OUTPUTS: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyOutput {
    pub index: usize,
    pub pts_us: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyOutputSelection<T> {
    pub discard: Vec<T>,
    pub keep: Vec<T>,
}

pub fn select_ready_outputs<T: Copy>(
    outputs: &[T],
    maximum_kept: usize,
) -> ReadyOutputSelection<T> {
    let discard_count = outputs.len().saturating_sub(maximum_kept);
    ReadyOutputSelection {
        discard: outputs[..discard_count].to_vec(),
        keep: outputs[discard_count..].to_vec(),
    }
}

pub const fn release_timestamp_ns(timestamp_ns: i64) -> i64 {
    timestamp_ns
}

impl AndroidDecoder {
    /// Dequeue one output without rendering it. The returned index must be
    /// released or discarded exactly once by the same worker thread.
    pub fn dequeue_ready_output(
        &mut self,
        timeout_us: i64,
    ) -> Result<Option<ReadyOutput>, DecoderError> {
        if !self.started {
            return Err(DecoderError::NotStarted);
        }
        let mut timeout = timeout_us;
        for _ in 0..4 {
            let mut info = AMediaCodecBufferInfo {
                offset: 0,
                size: 0,
                presentation_time_us: 0,
                flags: 0,
            };
            let index = unsafe { AMediaCodec_dequeueOutputBuffer(self.codec, &mut info, timeout) };
            timeout = 0;
            match index {
                AMEDIACODEC_INFO_TRY_AGAIN_LATER => return Ok(None),
                AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
                | AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED => continue,
                value if value >= 0 => {
                    return Ok(Some(ReadyOutput {
                        index: value as usize,
                        pts_us: info.presentation_time_us,
                    }));
                }
                error => {
                    return Err(DecoderError::OpFailed {
                        status: error as i32,
                    });
                }
            }
        }
        Ok(None)
    }

    pub fn release_output_at(
        &mut self,
        output: ReadyOutput,
        timestamp_ns: i64,
    ) -> Result<(), DecoderError> {
        let status = unsafe {
            AMediaCodec_releaseOutputBufferAtTime(
                self.codec,
                output.index,
                release_timestamp_ns(timestamp_ns),
            )
        };
        if status != AMEDIA_OK {
            return Err(DecoderError::OpFailed { status });
        }
        self.frames_rendered = self.frames_rendered.saturating_add(1);
        Ok(())
    }

    pub fn discard_output(&mut self, output: ReadyOutput) -> Result<(), DecoderError> {
        let status = unsafe { AMediaCodec_releaseOutputBuffer(self.codec, output.index, false) };
        if status != AMEDIA_OK {
            return Err(DecoderError::OpFailed { status });
        }
        self.frames_discarded = self.frames_discarded.saturating_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_output_budget_keeps_only_the_latest_surface_frame() {
        assert_eq!(MAX_RENDERABLE_OUTPUTS, 1);
    }

    #[test]
    fn output_burst_keeps_only_newest_bounded_entries() {
        let decision = select_ready_outputs(&[1, 2, 3, 4], MAX_RENDERABLE_OUTPUTS);
        assert_eq!(decision.discard, vec![1, 2, 3]);
        assert_eq!(decision.keep, vec![4]);
    }

    #[test]
    fn timed_release_uses_exact_coordinator_timestamp() {
        assert_eq!(release_timestamp_ns(7_000_000), 7_000_000);
    }
}
