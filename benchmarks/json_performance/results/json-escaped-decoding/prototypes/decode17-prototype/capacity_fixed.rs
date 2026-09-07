#![allow(dead_code)]
#[path="../decode15/simd.rs"] mod simd;
#[path="../decode15/before.rs"] mod before;
#[path="after.rs"] mod after;
fn main(){
for escaped in [false,true] {
 let mut changes=0;
 for power in 6..19 { for delta in -8isize..=8 {
  let len=((1usize<<power) as isize+delta) as usize;
  let mut source=br#""\n"#.to_vec();
  source.extend(std::iter::repeat_n(b'a',len-1-if escaped {24} else {0}));
  if escaped {source.extend_from_slice(br"\n".repeat(24).as_slice());} source.push(b'"');
  let a=before::DirectParser::new(&source).parse_string_bytes().unwrap();
  let b=after::DirectParser::new(&source).parse_string_bytes().unwrap();
  if let (before::ParsedStr::Owned(a),after::ParsedStr::Owned(b))=(a,b) {
    if a.capacity()!=b.capacity(){changes+=1; if changes==1 {println!("escaped={escaped} len={len} before={} after={}",a.capacity(),b.capacity());}}
  }
 }}
 println!("escaped={escaped} changed={changes}");
}}
