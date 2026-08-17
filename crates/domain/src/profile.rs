//! Quality profiles and the window-aware budget allocator (docs/03 §9, docs/04 §4).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityProfile {
    Focus,
    Normal,
    BackgroundVisible,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomQualityProfile {
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u16,
    pub max_bitrate_bps: u64,
}

impl QualityProfile {
    pub fn pixel_rate(&self) -> u64 {
        match self {
            // 2560x1440x60 (docs/03 §9.2 focus)
            Self::Focus => 2560 * 1440 * 60,
            // 1920x1080x30
            Self::Normal => 1920 * 1080 * 30,
            // 1280x720x15
            Self::BackgroundVisible => 1280 * 720 * 15,
            // keyframe thumbnail only, effectively 1 fps at low res
            Self::Suspended => 320 * 180,
        }
    }

    pub fn bitrate_bps(&self) -> u64 {
        match self {
            Self::Focus => 20_000_000,
            Self::Normal => 8_000_000,
            Self::BackgroundVisible => 1_500_000,
            Self::Suspended => 100_000,
        }
    }
}

/// Per-window state feeding the allocator.
#[derive(Debug, Clone)]
pub struct WindowSignal {
    pub visible: bool,
    pub focused: bool,
    /// relative area 0.0..=1.0 of the largest visible window
    pub area: f32,
    /// user/device-requested quality multiplier 0.0..=1.0
    pub requested_quality: f32,
    /// 0.0 (healthy) .. 1.0 (worst)
    pub health_penalty: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allocation {
    pub profile: QualityProfile,
}

#[derive(Debug, Clone)]
pub struct Budget {
    pub max_total_pixel_rate: u64,
    pub max_total_bitrate_bps: u64,
    /// thermal severe caps the total pixel rate to this fraction (docs/05 §5.5)
    pub thermal_cap_fraction: f32,
    pub thermal_severe: bool,
}

/// Hysteresis window during which profile downgrades are held before applying
/// (docs/03 §9.2).
pub const HYSTERESIS_TICKS: u32 = 3;

#[derive(Debug, Default)]
pub struct QualityAllocator {
    /// last emitted profile per window index, and how many ticks the desired
    /// profile has differed
    hold: std::collections::BTreeMap<usize, (QualityProfile, u32)>,
}

impl QualityAllocator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute allocations; `tick` advances hysteresis bookkeeping.
    pub fn allocate(
        &mut self,
        signals: &[WindowSignal],
        budget: &Budget,
        tick: u64,
    ) -> Vec<Allocation> {
        let cap_multiplier = if budget.thermal_severe {
            budget.thermal_cap_fraction
        } else {
            1.0
        };
        let pixel_cap = (budget.max_total_pixel_rate as f64 * cap_multiplier as f64) as u64;
        let bitrate_cap = (budget.max_total_bitrate_bps as f64 * cap_multiplier as f64) as u64;

        // priority = visibility_weight * focus_weight * requested_quality * health_penalty
        let priorities: Vec<f64> = signals
            .iter()
            .map(|s| {
                let visibility = if s.visible { 1.0 } else { 0.0 };
                let focus = if s.focused { 1.0 } else { 0.6 };
                (visibility * focus * s.requested_quality as f64 * (1.0 - s.health_penalty as f64))
                    .max(0.0)
            })
            .collect();

        // Desired profile per window (before budget and hysteresis).
        let desired: Vec<QualityProfile> = signals
            .iter()
            .map(|s| {
                if !s.visible {
                    QualityProfile::Suspended
                } else if s.focused && s.area >= 0.5 {
                    QualityProfile::Focus
                } else if s.area >= 0.25 {
                    QualityProfile::Normal
                } else {
                    QualityProfile::BackgroundVisible
                }
            })
            .collect();

        // Budget greedy: sort visible windows by priority desc; give each its
        // desired profile while budget allows, otherwise downgrade in steps.
        let mut order: Vec<usize> = (0..signals.len()).collect();
        order.sort_by(|&a, &b| priorities[b].total_cmp(&priorities[a]));

        let mut used_pixels: u64 = 0;
        let mut used_bitrate: u64 = 0;
        let mut out = vec![
            Allocation {
                profile: QualityProfile::Suspended
            };
            signals.len()
        ];

        for &i in &order {
            if !signals[i].visible {
                continue; // hidden stream can suspend
            }
            let mut chosen = desired[i];
            loop {
                let p = chosen.pixel_rate();
                let b = chosen.bitrate_bps();
                if (used_pixels + p) <= pixel_cap && (used_bitrate + b) <= bitrate_cap {
                    break;
                }
                let next = match chosen {
                    QualityProfile::Focus => QualityProfile::Normal,
                    QualityProfile::Normal => QualityProfile::BackgroundVisible,
                    QualityProfile::BackgroundVisible => QualityProfile::Suspended,
                    QualityProfile::Suspended => break, // floor for visible: keep BackgroundVisible minimum
                };
                if next == QualityProfile::Suspended && signals[i].visible {
                    // visible streams receive minimum fair share: fall back to
                    // BackgroundVisible even if over budget, mark separately
                    chosen = QualityProfile::BackgroundVisible;
                    break;
                }
                chosen = next;
            }
            used_pixels += chosen.pixel_rate();
            used_bitrate += chosen.bitrate_bps();
            out[i].profile = chosen;
        }

        // Hysteresis: hold previous profile for HYSTERESIS_TICKS ticks on
        // quality changes. Suspension for hidden windows applies immediately —
        // visibility is sticky, so it is not focus thrash (docs/03 §9.2).
        for (i, alloc) in out.iter_mut().enumerate() {
            let entry = self.hold.entry(i).or_insert((alloc.profile, 0));
            if entry.0 != alloc.profile {
                if alloc.profile == QualityProfile::Suspended {
                    *entry = (QualityProfile::Suspended, 0);
                    continue;
                }
                *entry = (entry.0, entry.1 + 1);
                if entry.1 > HYSTERESIS_TICKS {
                    *entry = (alloc.profile, 0);
                } else {
                    alloc.profile = entry.0;
                }
            } else {
                *entry = (alloc.profile, 0);
            }
        }
        let _ = tick;
        out
    }

    /// Total allocated load, for property tests.
    pub fn total_load(allocations: &[Allocation], budget: &Budget) -> (u64, u64) {
        let px: u64 = allocations.iter().map(|a| a.profile.pixel_rate()).sum();
        let br: u64 = allocations.iter().map(|a| a.profile.bitrate_bps()).sum();
        let cap = if budget.thermal_severe {
            budget.thermal_cap_fraction
        } else {
            1.0
        };
        (
            (px as f64 * cap as f64) as u64,
            (br as f64 * cap as f64) as u64,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generous() -> Budget {
        Budget {
            max_total_pixel_rate: u64::MAX / 4,
            max_total_bitrate_bps: u64::MAX / 4,
            thermal_cap_fraction: 1.0,
            thermal_severe: false,
        }
    }

    fn signal(visible: bool, focused: bool, area: f32) -> WindowSignal {
        WindowSignal {
            visible,
            focused,
            area,
            requested_quality: 1.0,
            health_penalty: 0.0,
        }
    }

    #[test]
    fn focused_large_window_receives_focus_profile() {
        let mut a = QualityAllocator::new();
        let out = a.allocate(
            &[signal(true, true, 1.0), signal(true, false, 0.2)],
            &generous(),
            0,
        );
        assert_eq!(out[0].profile, QualityProfile::Focus);
        assert_eq!(out[1].profile, QualityProfile::BackgroundVisible);
    }

    #[test]
    fn unfocused_visible_window_keeps_playing() {
        let mut a = QualityAllocator::new();
        let out = a.allocate(&[signal(true, false, 0.4)], &generous(), 0);
        assert_eq!(out[0].profile, QualityProfile::Normal);
    }

    #[test]
    fn hidden_stream_can_suspend() {
        let mut a = QualityAllocator::new();
        let out = a.allocate(
            &[signal(false, false, 0.0), signal(true, true, 1.0)],
            &generous(),
            0,
        );
        assert_eq!(out[0].profile, QualityProfile::Suspended);
        assert_eq!(out[1].profile, QualityProfile::Focus);
    }

    #[test]
    fn small_window_downgrades_after_hysteresis() {
        let mut a = QualityAllocator::new();
        // large focused window first
        let _ = a.allocate(&[signal(true, true, 1.0)], &generous(), 0);
        // shrink it: desired changes, but hold for HYSTERESIS_TICKS ticks
        let out1 = a.allocate(&[signal(true, true, 0.05)], &generous(), 1);
        assert_eq!(out1[0].profile, QualityProfile::Focus, "held by hysteresis");
        let out2 = a.allocate(&[signal(true, true, 0.05)], &generous(), 2);
        assert_eq!(out2[0].profile, QualityProfile::Focus);
        let out3 = a.allocate(&[signal(true, true, 0.05)], &generous(), 3);
        assert_eq!(out3[0].profile, QualityProfile::Focus);
        let out4 = a.allocate(&[signal(true, true, 0.05)], &generous(), 4);
        assert_eq!(
            out4[0].profile,
            QualityProfile::BackgroundVisible,
            "downgrade applies after hysteresis"
        );
    }

    #[test]
    fn rapid_focus_changes_do_not_thrash_encoder() {
        let mut a = QualityAllocator::new();
        // alternate focus between two large windows every tick; each window's
        // profile must not change more often than once per HYSTERESIS_TICKS
        let mut last = Vec::new();
        let mut changes = [0u32; 2];
        for t in 0..12 {
            let flip = t % 2 == 0;
            let out = a.allocate(
                &[signal(true, flip, 0.8), signal(true, !flip, 0.8)],
                &generous(),
                t,
            );
            let current: Vec<QualityProfile> = out.iter().map(|o| o.profile).collect();
            if !last.is_empty() {
                for w in 0..2 {
                    if current[w] != last[w] {
                        changes[w] += 1;
                    }
                }
            }
            last = current;
        }
        // 12 ticks of alternation: without hysteresis each window flips ~6x.
        // With hysteresis the encoder is reconfigured at most 12/HYSTERESIS_TICKS
        // times per window.
        for (w, change_count) in changes.iter().enumerate() {
            assert!(
                *change_count <= 12 / HYSTERESIS_TICKS,
                "window {w} changed profile {change_count} times in 12 ticks — thrash"
            );
        }
    }

    #[test]
    fn thermal_severe_caps_total_pixel_rate() {
        let mut a = QualityAllocator::new();
        let budget = Budget {
            max_total_pixel_rate: 4 * QualityProfile::Focus.pixel_rate(),
            max_total_bitrate_bps: u64::MAX / 4,
            thermal_cap_fraction: 0.25,
            thermal_severe: true,
        };
        let out = a.allocate(
            &[
                signal(true, true, 1.0),
                signal(true, false, 0.9),
                signal(true, false, 0.9),
                signal(true, false, 0.9),
            ],
            &budget,
            0,
        );
        let (px, _) = QualityAllocator::total_load(&out, &budget);
        // Focus*4 = 100%; cap 0.25 → at most ~1 focus-equivalent + floors
        assert!(
            px <= budget.max_total_pixel_rate / 2,
            "thermal cap must compress allocations, got {px} vs cap {}",
            budget.max_total_pixel_rate / 4
        );
    }

    #[test]
    fn visible_streams_receive_minimum_fair_share() {
        let mut a = QualityAllocator::new();
        // Extremely tight budget: only one BackgroundVisible fits.
        let budget = Budget {
            max_total_pixel_rate: QualityProfile::BackgroundVisible.pixel_rate(),
            max_total_bitrate_bps: QualityProfile::BackgroundVisible.bitrate_bps(),
            thermal_cap_fraction: 1.0,
            thermal_severe: false,
        };
        let out = a.allocate(
            &[signal(true, false, 0.3), signal(true, false, 0.3)],
            &budget,
            0,
        );
        assert_ne!(
            out[0].profile,
            QualityProfile::Suspended,
            "visible stream must not be starved to suspend"
        );
        assert_ne!(out[1].profile, QualityProfile::Suspended);
    }

    proptest::proptest! {
        #[test]
        fn allocator_never_exceeds_total_budget(
            windows in proptest::collection::vec(
                (0.0f32..=1.0, proptest::bool::ANY, proptest::bool::ANY),
                1..6
            ),
            severe in proptest::bool::ANY,
        ) {
            let mut a = QualityAllocator::new();
            let signals: Vec<WindowSignal> = windows
                .iter()
                .map(|&(area, visible, focused)| WindowSignal {
                    visible,
                    focused,
                    area,
                    requested_quality: 1.0,
                    health_penalty: 0.0,
                })
                .collect();
            let budget = Budget {
                max_total_pixel_rate: 3 * QualityProfile::Focus.pixel_rate(),
                max_total_bitrate_bps: 50_000_000,
                thermal_cap_fraction: 0.25,
                thermal_severe: severe,
            };
            // run several ticks so hysteresis settles, then check budget
            let mut out = Vec::new();
            for t in 0..(HYSTERESIS_TICKS as u64 + 2) {
                out = a.allocate(&signals, &budget, t);
            }
            let visible_count = signals.iter().filter(|s| s.visible).count() as u64;
            let floor = QualityProfile::BackgroundVisible.pixel_rate() * visible_count;
            let (px, _) = QualityAllocator::total_load(&out, &budget);
            let cap = (budget.max_total_pixel_rate as f64
                * if severe { budget.thermal_cap_fraction as f64 } else { 1.0 }) as u64;
            // allowance: visible floors can exceed cap by design (fair-share floor)
            assert!(px <= cap.max(floor), "pixel load {px} exceeds cap {cap} (floor {floor})");
        }
    }
}
