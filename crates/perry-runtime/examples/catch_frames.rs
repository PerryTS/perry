//! Instruction-count probe: compare `catch` with its `plain` loop control.
use perry_runtime::exception::catch_js_throw;
use std::hint::black_box;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "catch".into());
    let count: u64 = std::env::args()
        .nth(2)
        .map(|n| n.parse().unwrap())
        .unwrap_or(1_000_000);
    let mut sum = 0;
    for i in 0..count {
        sum += match mode.as_str() {
            "catch" => catch_js_throw(|| black_box(i)).unwrap(),
            "plain" => black_box(i),
            _ => panic!("expected catch or plain"),
        };
    }
    println!("{sum}");
}
