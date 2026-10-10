use flate2::{Compression,GzBuilder};
use flate2::read::{ZlibEncoder,DeflateEncoder};
use std::io::Read;
fn crc(bytes:&[u8])->u32 {
    let mut value=0xffffffffu32;
    for byte in bytes {
        value^=*byte as u32;
        for _ in 0..8 {value=(value>>1)^if value&1!=0 {0xedb88320} else {0};}
    }
    !value
}
fn main(){
    let data="one-shot/native-payload ".repeat(100);
    let mut gzip=Vec::new();
    GzBuilder::new().mtime(0).operating_system(3).read(data.as_bytes(),Compression::default()).read_to_end(&mut gzip).unwrap();
    let mut zlib=Vec::new();
    ZlibEncoder::new(data.as_bytes(),Compression::default()).read_to_end(&mut zlib).unwrap();
    let mut raw=Vec::new();
    DeflateEncoder::new(data.as_bytes(),Compression::default()).read_to_end(&mut raw).unwrap();
    println!("gzip {}\ndeflate {}\ndeflateRaw {}",crc(&gzip),crc(&zlib),crc(&raw));
}
