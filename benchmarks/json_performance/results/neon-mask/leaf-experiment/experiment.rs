#[path="old.rs"] mod old;
#[path="new.rs"] mod new;
#[no_mangle]
pub unsafe extern "C" fn old_scan(p: *const u8, len: usize) -> usize {
 old::find_string_terminator(std::slice::from_raw_parts(p,len)).unwrap_or(len)
}
#[no_mangle]
pub unsafe extern "C" fn new_scan(p: *const u8, len: usize) -> usize {
 new::find_string_terminator(std::slice::from_raw_parts(p,len)).unwrap_or(len)
}
fn main() {
 let mut checked=0usize;
 for n in 0..=96 {
  for offset in 0..=31 {
   for special in 0..=255u8 {
    let mut backing=vec![b'a';offset+n];let slice=&mut backing[offset..];
    for pos in 0..n {
     slice[pos]=special;
     let want=slice.iter().position(|&b| b==b'"' || b==b'\\' || b<32);
     assert_eq!(new::find_string_terminator(slice),want);
     assert_eq!(old::find_string_terminator(slice),want);
     let want=slice.iter().position(|&b| b==b'"' || b==b'\\' || b<32 || b==0xed);
     assert_eq!(new::find_string_escape(slice),want);
     let want=slice.iter().position(|&b| b==b'"' || b==b'\\');
     assert_eq!(new::find_quote_or_backslash(slice),want);
     slice[pos]=b'a';checked+=1;
    }
   }
  }
 }
 for bits in 0..65536u32 {
  let mut bytes=[b'a';16];for i in 0..16 {if bits&(1<<i)!=0 {bytes[i]=b'"';}}
  assert_eq!(new::find_string_terminator(&bytes),bytes.iter().position(|&b| b==b'"'));checked+=1;
 }
 println!("CHECKED {}",checked);
}
