// Reference comparisons and allocation checks for the production UTF-16 counter.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
struct Counted;
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
fn check(bytes: &[u8]) {
    let expected = match std::str::from_utf8(bytes) {
        Ok(s) => s.encode_utf16().count() as u32,
        Err(_) => compute_utf16_len_wtf8(bytes),
    };
    let before = ALLOCS.load(Ordering::Relaxed);
    let actual = counter::count_bytes(std::hint::black_box(bytes));
    let after = ALLOCS.load(Ordering::Relaxed);
    assert_eq!(before, after, "temporary allocation");
    assert_eq!(actual, expected);
}
fn main() {
    let all: String = (0..=0x10ffff).filter_map(char::from_u32).collect();
    check(all.as_bytes());
    let mut bytes = "aé中🙂\0".repeat(12).into_bytes();
    let mut cases = 1;
    for position in 0..bytes.len() {
        let original = bytes[position];
        for b in 0..=255 {
            bytes[position] = b;
            check(&bytes);
            cases += 1;
        }
        bytes[position] = original;
    }
    println!("PASS {cases} validated/WTF-8 counts; zero temporary allocations");
}
