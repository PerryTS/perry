#![allow(dead_code)]
mod before;
mod after;
use std::hint::black_box;
fn main() {
 let args: Vec<_> = std::env::args().collect();
 let bytes = std::fs::read(&args[1]).unwrap();
 let n: usize = args[2].parse().unwrap();
 let scan = if args[3] == "before" { before::find_quote_or_backslash } else { after::find_quote_or_backslash };
 let start = std::time::Instant::now();
 let mut sum = 0;
 for _ in 0..n {
  let mut pos = 0;
  while let Some(hit) = scan(black_box(&bytes[pos..])) {
   pos += hit;
   sum += pos;
   pos += if bytes[pos] == b'\\' { 2 } else { 1 };
   pos = pos.min(bytes.len());
  }
 }
 println!("{} {} {}", args[3], start.elapsed().as_nanos(), black_box(sum));
}
