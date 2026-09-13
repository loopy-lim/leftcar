use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
struct Meter;
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
}
#[global_allocator]
static ALLOCATOR: Meter = Meter;
fn main() {
    let mut group = FecGroup::new(1, 8, 0, 8).unwrap();
    let data = vec![1; 1367];
    let parity = vec![2; 1400];
    for index in 0..2 {
        group.push_data(FrameFragment { index, count: 8, id: 1,
            capture_wall_ms: None, encode_wall_ms: None, send_wall_ms: 0, payload: &data });
    }
    group.push_parity(ParityFragment { id: 1, k: 8, index: 0, base: 0, total: 8,
        send_wall_ms: 0, payload: &parity });
    let start = std::time::Instant::now();
    ALLOCS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    let mut none_count = 0;
    for _ in 0..100 {
        none_count += usize::from(black_box(black_box(&mut group).try_restore()).is_none());
    }
    let calls = ALLOCS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    let ns = start.elapsed().as_nanos();
    assert_eq!(none_count, 100);
    println!("{{\"attempts\":100,\"available\":3,\"required\":8,\"allocations\":{calls},\"requestedBytes\":{bytes},\"ns\":{ns}}}");
    if std::env::args().any(|a| a == "--assert-zero") { assert_eq!((calls, bytes), (0, 0)); }
}
