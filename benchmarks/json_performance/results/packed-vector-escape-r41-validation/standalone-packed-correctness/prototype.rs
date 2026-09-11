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

#[path = "proposal/stringify_native_escape.rs"]
mod native;

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}

fn check(text: &str, alignment: usize) {
    let bytes = text.as_bytes();
    let wanted = reference(bytes);
    let mut input = vec![0xcc; alignment];
    input.extend_from_slice(bytes);
    let prefix = 17 + alignment;
    let mut output = vec![0xa5; prefix + wanted.len() + 64];
    let count = unsafe { native::write(&input[alignment..], output.as_mut_ptr().add(prefix)) };
    assert_eq!(count, wanted.len());
    assert_eq!(&output[prefix..prefix + count], wanted.as_slice());
    assert!(output[..prefix].iter().all(|&b| b == 0xa5));
    assert!(output[prefix + count..].iter().all(|&b| b == 0xa5));
    println!("{}\t{}", hex(bytes), hex(&output[prefix..prefix + count]));
}

fn main() {
    let pools = [
        "plain-ascii",
        "\n\"\\\t",
        "東京🙂한\n",
        "\0\u{1}\u{8}\u{c}\r",
        "€\u{80}\u{10ffff}",
    ];
    let mut seed = 0x6d35_917f_u64;
    for n in [
        0, 1, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 65, 255, 256, 257, 1023, 1024, 1025,
    ] {
        for pool in pools {
            let chars: Vec<char> = pool.chars().collect();
            let text: String = (0..n)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    chars[seed as usize % chars.len()]
                })
                .collect();
            for alignment in [0, 1, 7, 15, 31] {
                check(&text, alignment);
            }
        }
    }
    let ascii: String = (0..=127).map(|b| char::from_u32(b).unwrap()).collect();
    for alignment in 0..32 {
        check(&ascii.repeat(5), alignment);
    }
    check(&"line\n\"quote\"\\tab\t".repeat(65536), 0);
    check(&"東京🙂한\n".repeat(65536), 15);
}
