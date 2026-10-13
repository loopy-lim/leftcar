extern crate fec_baseline;
extern crate fec_updated;
use std::{hint::black_box, time::Instant};

fn main() {
    let data: Vec<Vec<u8>> = (0..8)
        .map(|i| (0..1124).map(|j| (i * 31 + j) as u8).collect())
        .collect();
    let before = fec_baseline::encode_group_with_parity(&data, 4).unwrap();
    let after = fec_updated::encode_group_with_parity(&data, 4).unwrap();
    assert_eq!(before.parity, after.parity);
    for missing in [0, 1, 2, 4] {
        let mut received = before.all_shards();
        for shard in received.iter_mut().take(missing) {
            *shard = None;
        }
        let run_before = || {
            let start = Instant::now();
            for _ in 0..20_000 {
                black_box(
                    fec_baseline::decode_group_with_max_parity(
                        black_box(received.clone()),
                        8,
                        1126,
                        4,
                    )
                    .unwrap(),
                );
            }
            start.elapsed().as_nanos() as f64 / 20_000.0
        };
        let run_after = || {
            let start = Instant::now();
            for _ in 0..20_000 {
                black_box(
                    fec_updated::decode_group_with_max_parity(
                        black_box(received.clone()),
                        8,
                        1126,
                        4,
                    )
                    .unwrap(),
                );
            }
            start.elapsed().as_nanos() as f64 / 20_000.0
        };
        for repetition in 0..3 {
            let (before_ns, after_ns) = if repetition % 2 == 0 {
                (run_before(), run_after())
            } else {
                let a = run_after();
                (run_before(), a)
            };
            println!("missing={missing} repetition={repetition} before_ns={before_ns:.1} after_ns={after_ns:.1} speedup={:.2}x", before_ns / after_ns);
        }
    }
}
