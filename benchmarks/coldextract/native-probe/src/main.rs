use sha2::Digest;
use std::time::Instant;
fn main(){
 let arm=std::env::args().nth(1).unwrap();
 let data:Vec<_>=(0..21).map(|i|std::fs::read(format!("/root/lanes/perry-tarloop/micro/corpus/{i:02}.tgz")).unwrap()).collect();
 let t=Instant::now();let mut check=0;
 for _ in 0..100{
  for bytes in &data{
   if arm=="sha2"{let mut h=sha2::Sha512::new();for chunk in bytes.chunks(1048576){h.update(chunk);}check^=h.finalize()[0];}
   else{let mut h=ring::digest::Context::new(&ring::digest::SHA512);for chunk in bytes.chunks(1048576){h.update(chunk);}check^=h.finish().as_ref()[0];}
  }
 }
 println!("{check}");eprintln!("{}",t.elapsed().as_secs_f64());
}
