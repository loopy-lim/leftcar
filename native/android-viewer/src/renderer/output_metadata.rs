//! Bounded metadata owned by one renderer decoder lifetime. Latest released
//! PTS is a sample of the last output, not a distribution of every drain output.
use std::collections::VecDeque;
#[derive(Default)]
pub struct OutputMetadata {
    epoch: u64,
    inputs: VecDeque<(i64, Option<u64>)>,
}
impl OutputMetadata {
    pub fn reset(&mut self) {
        self.epoch += 1;
        self.inputs.clear();
    }
    pub fn observe(
        &mut self,
        pts: i64,
        capture: Option<u64>,
        queued: bool,
        released: Option<i64>,
        released_count: u64,
    ) -> Option<u64> {
        if self.inputs.back().is_some_and(|(last, _)| pts <= *last) {
            self.reset();
        }
        if queued {
            self.inputs.push_back((pts, capture));
            while self.inputs.len() > 128 {
                self.inputs.pop_front();
            }
        }
        if released_count == 0 {
            return None;
        }
        let released = released?;
        let capture = self
            .inputs
            .iter()
            .find(|(pts, _)| *pts == released)
            .and_then(|(_, capture)| *capture);
        while self.inputs.front().is_some_and(|(pts, _)| *pts <= released) {
            self.inputs.pop_front();
        }
        capture
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_joins_older_pts_even_when_current_input_is_rejected() {
        let mut map = OutputMetadata::default();
        assert_eq!(map.observe(10, Some(100), true, None, 0), None);
        assert_eq!(map.observe(20, Some(200), false, Some(10), 2), Some(100));
        assert_eq!(map.observe(30, Some(300), true, Some(20), 1), None);
    }
    #[test]
    fn reset_reuse_eviction_and_missing_are_unknown() {
        let mut map = OutputMetadata::default();
        map.observe(10, Some(100), true, None, 0);
        map.reset();
        assert_eq!(map.epoch(), 1);
        assert_eq!(map.observe(10, Some(200), false, Some(10), 1), None);
        for pts in 1..=130 {
            map.observe(pts, Some(pts as u64), true, None, 0);
        }
        assert_eq!(map.observe(131, None, false, Some(1), 1), None);
        assert_eq!(map.observe(132, None, false, Some(130), 1), Some(130));
        assert_eq!(map.observe(133, None, false, Some(130), 0), None);
    }
}
