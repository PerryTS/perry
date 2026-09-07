#![allow(dead_code)]
mod simd;
mod before;
mod after;
mod scan;
use std::hint::black_box;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).unwrap();
    let n: usize = args[2].parse().unwrap();
    let run = if args[3] == "before" { before::nesting_depth_exceeds } else { after::nesting_depth_exceeds };
    let start = std::time::Instant::now();
    let mut sum = 0;
    for _ in 0..n { sum += usize::from(run(black_box(&bytes), black_box(1024))); }
    println!("{} {} {}", args[3], start.elapsed().as_nanos(), black_box(sum));
}

#[cfg(test)]
mod tests {
    fn check(bytes: &[u8]) {
        let mut pos = 0;
        let mut expected = None;
        while pos < bytes.len() {
            if bytes[pos] == b'"' { expected=Some(pos); break; }
            pos += if bytes[pos] == b'\\' {2} else {1};
        }
        assert_eq!(super::scan::quoted_end(bytes), expected, "quoted span={bytes:?}");
        for limit in [0, 1, 2, 8, 1024] {
            assert_eq!(super::before::nesting_depth_exceeds(bytes,limit),
                       super::after::nesting_depth_exceeds(bytes,limit), "limit={limit}, bytes={bytes:?}");
        }
    }

    #[test]
    fn all_backslash_masks_and_both_carries_match_scalar_state() {
        for pattern in 0..65536u64 {
            let mut slashes = 0u64;
            for i in 0..16 { if pattern & (1<<i) != 0 { slashes |= 0xf << (4*i); } }
            for incoming in [false,true] {
                let (mut escaped, mut quotes) = (incoming, 0u64);
                for i in 0..16 {
                    if escaped { escaped=false; }
                    else if pattern & (1<<i) != 0 { escaped=true; }
                    else { quotes |= 1 << (4*i); }
                }
                assert_eq!(super::scan::unescaped(!slashes,slashes,incoming), (quotes,escaped), "pattern={pattern:x}, incoming={incoming}");
            }
        }
    }

    #[test]
    fn quote_boundaries_truncations_and_arbitrary_bytes_match_old_depth_check() {
        let forms: &[&[u8]] = &[br#"\""#, br#"\\"#, br"\u0022", br"\q", b"\x00", b"[{}]"];
        for prefix in 0..80 {
            for form in forms {
                let mut bytes = br#"[{"text":"\n"#.to_vec();
                bytes.extend(std::iter::repeat_n(b'a',prefix));
                for _ in 0..9 { bytes.extend_from_slice(form); bytes.extend_from_slice("é中🙂".as_bytes()); }
                bytes.extend_from_slice(br#"","nested":[[[[[0]]]]]}]"#);
                for end in 0..=bytes.len() { check(&bytes[..end]); }
            }
        }
        let alphabet = b"abc\\\"\0\r\n\tuD09ABCDEFfedcba/[]{}: \xed\x80\xff";
        let mut state = 0x172621390c83ebc3u64;
        for n in 0..30000 {
            let mut bytes = vec![0; n%2048];
            for byte in &mut bytes {
                state ^= state << 13; state ^= state >> 7; state ^= state << 17;
                *byte = alphabet[state as usize % alphabet.len()];
            }
            check(&bytes);
        }
    }

    #[test]
    #[cfg(unix)]
    fn guard_page_and_long_backslash_runs() {
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
            let raw=libc::mmap(std::ptr::null_mut(),page*2,libc::PROT_READ|libc::PROT_WRITE,libc::MAP_ANON|libc::MAP_PRIVATE,-1,0);
            assert_ne!(raw,libc::MAP_FAILED);
            let base=raw.cast::<u8>();assert_eq!(libc::mprotect(base.add(page).cast(),page,libc::PROT_NONE),0);
            for len in 0..1024 {
                for suffix in [b"".as_slice(),b"\"[[[[]]]]",b"x\"[[[[]]]]",b"\x00\"[]"] {
                    let mut bytes=vec![b'\\';len];if len>0 { bytes[0]=b'"'; }
                    bytes.extend_from_slice(suffix);
                    let at=base.add(page-bytes.len());std::ptr::copy_nonoverlapping(bytes.as_ptr(),at,bytes.len());
                    check(std::slice::from_raw_parts(at,bytes.len()));
                }
            }
            assert_eq!(libc::munmap(raw,page*2),0);
        }
    }
}
