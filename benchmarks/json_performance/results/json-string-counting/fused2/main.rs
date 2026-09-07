#![allow(dead_code)]
mod count;
mod compat;
mod basic;
use std::hint::black_box;
fn legacy(bytes: &[u8]) -> u32 {
 let mut units = 0; let mut i = 0;
 while i < bytes.len() {
  let b = bytes[i];
  if b < 0x80 { units += 1; i += 1; }
  else if b < 0xc0 { i += 1; }
  else if b < 0xe0 { units += 1; i += 2; }
  else if b < 0xf0 { units += 1; i += 3; }
  else { units += 2; i += 4; }
 }
 units
}
fn fused(bytes: &[u8]) -> u32 { count::count(bytes).unwrap_or_else(|| legacy(bytes)) }
fn main() {
 let args: Vec<_> = std::env::args().collect();
 let bytes = std::fs::read(&args[1]).unwrap();
 let n: usize = args[2].parse().unwrap();
 let run = match args[3].as_str() { "compat" => compat::count_bytes, "basic" => basic::count_bytes, "fused" => fused, _ => panic!() };
 let start = std::time::Instant::now();let mut sum = 0u64;
 for _ in 0..n { sum += run(black_box(&bytes)) as u64; }
 println!("{} {} {}", args[3], start.elapsed().as_nanos(), black_box(sum));
}
#[test]
fn unicode_and_arbitrary_bytes() {
 let all: String = (0..=0x10ffff).filter_map(char::from_u32).collect();
 assert_eq!(fused(all.as_bytes()),all.encode_utf16().count() as u32);
 let mut state=0x8D140A35BC72690Fu64;
 for n in 0..20000 {
  let mut bytes=vec![0;n%4097];
  for b in &mut bytes {state^=state<<13;state^=state>>7;state^=state<<17;*b=state as u8;}
  assert_eq!(fused(&bytes),legacy(&bytes),"n={n}");
 }
 let mut bytes="aé中🙂".repeat(128).into_bytes();
 for i in 0..bytes.len() {
  let saved=bytes[i];
  for b in 0..=255 {bytes[i]=b;assert_eq!(fused(&bytes),legacy(&bytes),"i={i},b={b}");}
  bytes[i]=saved;
 }
 for n in 0..bytes.len() {assert_eq!(fused(&bytes[..n]),legacy(&bytes[..n]),"truncation={n}");}
}
