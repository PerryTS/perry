#![allow(dead_code)]
mod simd;
mod before;
mod after;
mod inlined;
use std::hint::black_box;
fn old(input: &[u8]) -> usize {
 let mut p=before::DirectParser::new(input);let mut sum=0;
 while p.pos<input.len() {
  if input[p.pos]==b'"' {
   if let Some(value)=p.parse_string_bytes() {let b=black_box(value.as_bytes());sum+=b.len()+b.first().copied().unwrap_or(0) as usize;}
   else {break;}
  } else {p.pos+=1;}
 }
 black_box(sum)
}
fn new(input: &[u8]) -> usize {
 let mut p=after::DirectParser::new(input);let mut sum=0;
 while p.pos<input.len() {
  if input[p.pos]==b'"' {
   if let Some(value)=p.parse_string_bytes() {let b=black_box(value.as_bytes());sum+=b.len()+b.first().copied().unwrap_or(0) as usize;}
   else {break;}
  } else {p.pos+=1;}
 }
 black_box(sum)
}
fn new_inlined(input: &[u8]) -> usize {
 let mut p=inlined::DirectParser::new(input);let mut sum=0;
 while p.pos<input.len() {
  if input[p.pos]==b'"' {
   if let Some(value)=p.parse_string_bytes() {let b=black_box(value.as_bytes());sum+=b.len()+b.first().copied().unwrap_or(0) as usize;}
   else {break;}
  } else {p.pos+=1;}
 }
 black_box(sum)
}
fn main() {
 let args: Vec<_> = std::env::args().collect();let bytes=std::fs::read(&args[1]).unwrap();
 let n: usize=args[2].parse().unwrap();let run=match args[3].as_str() {"before" => old, "after" => new, "inlined" => new_inlined, _ => panic!()};
 let start=std::time::Instant::now();let mut sum=0;
 for _ in 0..n {sum+=run(black_box(&bytes));}
 println!("{} {} {}",args[3],start.elapsed().as_nanos(),black_box(sum));
}
#[cfg(test)]
mod tests {
 fn check(bytes: &[u8]) {
  let mut a=super::before::DirectParser::new(bytes);let mut b=super::after::DirectParser::new(bytes);
  let aa=a.parse_string_bytes().map(|v|v.as_bytes().to_vec());let bb=b.parse_string_bytes().map(|v|v.as_bytes().to_vec());
  assert_eq!((&aa,a.pos,a.valid),(&bb,b.pos,b.valid),"input={bytes:?}");
  let mut c=super::inlined::DirectParser::new(bytes);
  let cc=c.parse_string_bytes().map(|v|v.as_bytes().to_vec());
  assert_eq!((aa,a.pos,a.valid),(cc,c.pos,c.valid),"input={bytes:?}");
 }
 #[test]
 fn escapes_truncations_and_arbitrary_bytes_match_original() {
  let escapes: &[&[u8]]=&[br#"\n"#,br#"\""#,br#"\\"#,br#"\u0000"#,br#"\uD800"#,br#"\uDC00"#,br#"\uD800\uDC00"#,br#"\uDBFF\uDFFF"#,br#"\uD800\u0061"#,br#"\uZ100"#,br#"\q"#];
  for prefix in 0..130 {
   for escape in escapes {
    let mut bytes=vec![b'"'];bytes.extend(std::iter::repeat_n(b'a',prefix));bytes.extend_from_slice(escape);
    bytes.extend_from_slice("é中🙂".repeat(12).as_bytes());bytes.extend_from_slice(escape);bytes.push(b'"');
    for end in 0..=bytes.len() {check(&bytes[..end]);}
   }
  }
  let alphabet=b"abc\\\"\0\r\n\tuD09ABCDEFfedcba/[]{}: \xed\x80\xff";let mut state=0x194621390c82ebc3u64;
  for n in 0..20000 {
   let mut bytes=vec![b'"';1+n%1024];
   for b in &mut bytes[1..] {state^=state<<13;state^=state>>7;state^=state<<17;*b=alphabet[state as usize%alphabet.len()];}
   check(&bytes);
  }
 }
 #[test]
 #[cfg(unix)]
 fn guarded_source_and_all_window_tails() {
  unsafe {
   let page=libc::sysconf(libc::_SC_PAGESIZE) as usize;
   let raw=libc::mmap(std::ptr::null_mut(),page*2,libc::PROT_READ|libc::PROT_WRITE,libc::MAP_ANON|libc::MAP_PRIVATE,-1,0);
   assert_ne!(raw,libc::MAP_FAILED);let base=raw.cast::<u8>();assert_eq!(libc::mprotect(base.add(page).cast(),page,libc::PROT_NONE),0);
   for len in 0..=1024 {
    let source=br#""\uD800\uDC00\nabc\"\\\t0123456789"#;
    let mut bytes:Vec<u8>=source.iter().copied().cycle().take(len).collect();
    if len>0 {bytes[0]=b'"';}
    let at=base.add(page-len);std::ptr::copy_nonoverlapping(bytes.as_ptr(),at,len);check(std::slice::from_raw_parts(at,len));
   }
   assert_eq!(libc::munmap(raw,page*2),0);
  }
 }
}
