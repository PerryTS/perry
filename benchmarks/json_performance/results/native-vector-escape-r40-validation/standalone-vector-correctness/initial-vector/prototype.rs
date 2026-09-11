use std::fmt::Write;

fn reference(source: &[u8]) -> Vec<u8> {
    let mut out = vec![b'"'];
    for &b in source {
        match b {
            b'"' | b'\\' => out.extend_from_slice(&[b'\\', b]),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            8 => out.extend_from_slice(b"\\b"),
            12 => out.extend_from_slice(b"\\f"),
            0..=31 => out.extend_from_slice(format!("\\u{b:04x}").as_bytes()),
            _ => out.push(b),
        }
    }
    out.push(b'"');
    out
}

unsafe fn escape(b: u8, out: *mut u8) -> usize {
    out.write(b'\\');
    let short = match b {
        b'"' | b'\\' => b,
        b'\n' => b'n', b'\r' => b'r', b'\t' => b't', 8 => b'b', 12 => b'f',
        _ => 0,
    };
    if short != 0 {
        out.add(1).write(short);
        2
    } else {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.add(1).write(b'u'); out.add(2).write(b'0'); out.add(3).write(b'0');
        out.add(4).write(HEX[(b >> 4) as usize]); out.add(5).write(HEX[(b & 15) as usize]);
        6
    }
}

#[cfg(target_arch = "aarch64")]
unsafe fn vector_quote(source: &[u8], output: &mut [u8]) -> usize {
    use std::arch::aarch64::*;
    let dest = output.as_mut_ptr();
    dest.write(b'"');
    let mut pos = 0;
    let mut at = 1;
    while source.len() - pos >= 16 {
        let block = vld1q_u8(source.as_ptr().add(pos));
        let mask = vorrq_u8(
            vorrq_u8(vceqq_u8(block, vdupq_n_u8(b'"')), vceqq_u8(block, vdupq_n_u8(b'\\'))),
            vcltq_u8(block, vdupq_n_u8(32)),
        );
        // Copy only within the exact planned output. Bytes after the first
        // escape remain uncommitted and are overwritten on subsequent steps.
        assert!(output.len() - at >= 16);
        vst1q_u8(dest.add(at), block);
        if vmaxvq_u8(mask) == 0 {
            pos += 16; at += 16;
            continue;
        }
        let words = vreinterpretq_u64_u8(mask);
        let low = u64::from_le(vgetq_lane_u64::<0>(words));
        let prefix = if low != 0 {
            low.trailing_zeros() as usize / 8
        } else {
            8 + u64::from_le(vgetq_lane_u64::<1>(words)).trailing_zeros() as usize / 8
        };
        pos += prefix; at += prefix;
        at += escape(source[pos], dest.add(at));
        pos += 1;
    }
    for &b in &source[pos..] {
        if b >= 32 && b != b'"' && b != b'\\' {
            dest.add(at).write(b); at += 1;
        } else {
            at += escape(b, dest.add(at));
        }
    }
    dest.add(at).write(b'"');
    at + 1
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes { write!(&mut s, "{b:02x}").unwrap(); }
    s
}

fn check(text: &str, alignment: usize) {
    let bytes = text.as_bytes();
    let wanted = reference(bytes);
    let mut input = vec![0xcc; alignment];
    input.extend_from_slice(bytes);
    let prefix = 17 + alignment;
    let mut output = vec![0xa5; prefix + wanted.len() + 64];
    let count = unsafe { vector_quote(&input[alignment..], &mut output[prefix..prefix + wanted.len()]) };
    assert_eq!(count, wanted.len());
    assert_eq!(&output[prefix..prefix + count], wanted.as_slice());
    assert!(output[..prefix].iter().all(|&b| b == 0xa5));
    assert!(output[prefix + count..].iter().all(|&b| b == 0xa5));
    println!("{}\t{}", hex(bytes), hex(&output[prefix..prefix + count]));
}

fn main() {
    let pools = ["plain-ascii", "\n\"\\\t", "東京🙂한\n", "\0\u{1}\u{8}\u{c}\r", "€\u{80}\u{10ffff}"];
    let mut seed = 0x6d35_917f_u64;
    for n in [0,1,7,8,15,16,17,31,32,33,63,64,65,255,256,257,1023,1024,1025] {
        for pool in pools {
            let chars: Vec<char> = pool.chars().collect();
            let text: String = (0..n).map(|_| {
                seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
                chars[seed as usize % chars.len()]
            }).collect();
            for alignment in [0,1,7,15,31] { check(&text, alignment); }
        }
    }
    let ascii: String = (0..=127).map(|b| char::from_u32(b).unwrap()).collect();
    for alignment in 0..32 { check(&ascii.repeat(5), alignment); }
    check(&"line\n\"quote\"\\tab\t".repeat(65536), 0);
    check(&"東京🙂한\n".repeat(65536), 15);
}
