//! 표면 릴리스 간격(surface release interval) 히스토그램.
//!
//! docs/06 위생: Surface release는 물리 표시가 아니므로 지표명을
//! `release_gap_*`로 쓴다. split(`release_output_at`)과 단일(freshness/
//! 스무딩/paced/pump) 경로가 모두 이 모듈을 통과하므로 한 계층 계측으로
//! 전 창이 프로세스 전역 집계된다. 유일한 소비자는 진단 샘플러 틱의
//! `read_and_reset`이다(기록 공백 자체가 사망 증거라 reset은 간격 사슬의
//! 마지막 릴리스 시각을 보존한다).

use std::sync::{Mutex, OnceLock};

/// 버킷 폭 1ms. 0..=1023ms를 버킷으로 잡고 그 이상은 overflow로 센다.
pub const CADENCE_BUCKET_COUNT: usize = 1024;

/// `read_and_reset`이 돌려주는 집계 윈도우. 값 단위는 ms다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CadenceWindow {
    pub count: u64,
    pub p50_ms: u32,
    pub p95_ms: u32,
    pub p99_ms: u32,
    pub max_ms: u32,
}

#[derive(Debug)]
pub struct CadenceHistogram {
    buckets: [u32; CADENCE_BUCKET_COUNT],
    overflow_count: u64,
    total_count: u64,
    max_ms: u32,
    last_release_ns: Option<u64>,
}

impl Default for CadenceHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl CadenceHistogram {
    pub const fn new() -> Self {
        Self {
            buckets: [0; CADENCE_BUCKET_COUNT],
            overflow_count: 0,
            total_count: 0,
            max_ms: 0,
            last_release_ns: None,
        }
    }

    /// 성공한 Surface release 한 번을 기록한다. 최초 릴리스는 간격이 없어
    /// 사슬 시작만 하고 집계하지 않는다.
    pub fn record_release(&mut self, now_ns: u64) {
        if let Some(last_ns) = self.last_release_ns {
            let gap_ms = u32::try_from(now_ns.saturating_sub(last_ns) / 1_000_000)
                .unwrap_or(u32::MAX);
            let bucket = usize::try_from(gap_ms).unwrap_or(usize::MAX);
            if bucket >= CADENCE_BUCKET_COUNT {
                self.overflow_count += 1;
            } else {
                self.buckets[bucket] += 1;
            }
            self.total_count += 1;
            self.max_ms = self.max_ms.max(gap_ms);
        }
        self.last_release_ns = Some(now_ns);
    }

    /// 집계를 비우고 윈도우를 돌려준다. 간격 사슬의 마지막 릴리스 시각은
    /// 보존해 다음 윈도우가 이어지게 한다.
    pub fn read_and_reset(&mut self) -> Option<CadenceWindow> {
        if self.total_count == 0 {
            return None;
        }
        let window = CadenceWindow {
            count: self.total_count,
            p50_ms: self.percentile_ms(50).unwrap_or_default(),
            p95_ms: self.percentile_ms(95).unwrap_or_default(),
            p99_ms: self.percentile_ms(99).unwrap_or_default(),
            max_ms: self.max_ms,
        };
        self.buckets = [0; CADENCE_BUCKET_COUNT];
        self.overflow_count = 0;
        self.total_count = 0;
        self.max_ms = 0;
        Some(window)
    }

    /// 분위수 계산. 버킷 순회 + overflow(1024ms 취급) 포함. 비어 있으면 None.
    fn percentile_ms(&self, percentile: u32) -> Option<u32> {
        if self.total_count == 0 {
            return None;
        }
        // ceil(percentile * count / 100)번째 누적 도달 버킷이 분위수다.
        let target = (percentile as u64 * self.total_count).div_ceil(100);
        let mut cumulative = 0u64;
        for (bucket, &hits) in self.buckets.iter().enumerate() {
            cumulative += u64::from(hits);
            if cumulative >= target {
                return Some(u32::try_from(bucket).unwrap_or(u32::MAX));
            }
        }
        Some(CADENCE_BUCKET_COUNT as u32)
    }
}

static PRESENTATION_CADENCE: OnceLock<Mutex<CadenceHistogram>> = OnceLock::new();

fn global_cadence() -> &'static Mutex<CadenceHistogram> {
    PRESENTATION_CADENCE.get_or_init(|| Mutex::new(CadenceHistogram::new()))
}

/// 렌더 경로가 성공한 Surface release마다 호출한다. 락 경합 시 기록은
/// 포기한다(계측이 재생에 간섭하지 않는다는 원칙).
pub fn record_surface_release() {
    let now = super::output::monotonic_now_ns();
    if let Ok(mut histogram) = global_cadence().lock() {
        histogram.record_release(now);
    }
}

/// 진단 샘플러 틱이 유일한 소비자다. 윈도우가 비면 None.
pub fn read_and_reset_surface_cadence() -> Option<CadenceWindow> {
    global_cadence().lock().ok().and_then(|mut histogram| histogram.read_and_reset())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ms 간격 시퀀스를 밀리초 나노 타임스탬프로 변환해 기록한다.
    /// 첫 기록은 사슬 시작(미집계), 이후 각 기록이 앞 간격 하나가 된다.
    fn feed(histogram: &mut CadenceHistogram, gaps_ms: &[u64]) {
        let mut now_ns: u64 = 1_000_000_000;
        histogram.record_release(now_ns);
        for &gap in gaps_ms {
            now_ns += gap * 1_000_000;
            histogram.record_release(now_ns);
        }
    }

    #[test]
    fn first_release_starts_the_chain_without_a_sample() {
        let mut histogram = CadenceHistogram::new();
        histogram.record_release(1_000_000_000);
        assert!(histogram.read_and_reset().is_none());
    }

    #[test]
    fn known_gap_sequence_produces_expected_percentiles() {
        // 값 6개: 10,10,10,20,30,100 → p50=10, p95=100, p99=100, max=100
        let mut histogram = CadenceHistogram::new();
        feed(&mut histogram, &[10, 10, 10, 20, 30, 100]);
        let window = histogram.read_and_reset().expect("samples recorded");
        assert_eq!(window.count, 6);
        assert_eq!(window.p50_ms, 10);
        assert_eq!(window.p95_ms, 100);
        assert_eq!(window.p99_ms, 100);
        assert_eq!(window.max_ms, 100);
    }

    #[test]
    fn read_and_reset_empties_but_keeps_the_gap_chain() {
        let mut histogram = CadenceHistogram::new();
        feed(&mut histogram, &[16, 16]);
        assert!(histogram.read_and_reset().is_some());
        assert!(histogram.read_and_reset().is_none());
        // reset 후 첫 기록은 마지막 릴리스(사슬 보존) 기준 간격이다.
        histogram.record_release(1_000_000_000 + 3 * 16 * 1_000_000);
        let window = histogram.read_and_reset().expect("chain continues");
        assert_eq!(window.count, 1);
        assert_eq!(window.p50_ms, 16);
    }

    #[test]
    fn oversized_gaps_land_in_overflow_but_still_report_max() {
        let mut histogram = CadenceHistogram::new();
        feed(&mut histogram, &[2_000]);
        let window = histogram.read_and_reset().expect("overflow recorded");
        assert_eq!(window.count, 1);
        assert_eq!(window.p50_ms, CADENCE_BUCKET_COUNT as u32);
        assert_eq!(window.max_ms, 2_000);
    }

    #[test]
    fn bucket_boundary_matches_last_in_range_bucket() {
        let mut histogram = CadenceHistogram::new();
        feed(&mut histogram, &[1_023]);
        let window = histogram.read_and_reset().expect("in-range bucket");
        assert_eq!(window.p50_ms, 1_023);
        assert_eq!(window.max_ms, 1_023);
    }

    #[test]
    fn single_sample_percentiles_collapse_to_the_same_value() {
        let mut histogram = CadenceHistogram::new();
        feed(&mut histogram, &[33]);
        let window = histogram.read_and_reset().expect("one sample");
        assert_eq!(window.p50_ms, 33);
        assert_eq!(window.p95_ms, 33);
        assert_eq!(window.p99_ms, 33);
        assert_eq!(window.max_ms, 33);
    }

    #[test]
    fn mixed_buckets_interpolate_percentile_targets() {
        // 값 100개: 90개는 8ms, 10개는 40ms → p50=8, p95=40, p99=40
        let mut gaps = vec![8u64; 90];
        gaps.extend(vec![40u64; 10]);
        let mut histogram = CadenceHistogram::new();
        feed(&mut histogram, &gaps);
        let window = histogram.read_and_reset().expect("samples recorded");
        assert_eq!(window.count, 100);
        assert_eq!(window.p50_ms, 8);
        assert_eq!(window.p95_ms, 40);
        assert_eq!(window.p99_ms, 40);
    }
}
