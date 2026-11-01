//! Exercise both backported WASIp3 RNGs through actual canonical imports.
const SENTINEL: u8 = 0xa5;

fn check(label: &str, mut fill: impl FnMut(&mut [u8])) {
    let mut changed = 0;
    let mut cases = 0;
    // Exercise every prefix alignment, word boundary and partial suffix.
    for offset in 0..16 {
        for length in [0, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 255, 256, 257] {
            let mut bytes = [SENTINEL; 304];
            let start = 16 + offset;
            let end = start + length;
            fill(&mut bytes[start..end]);
            assert!(bytes[..start].iter().all(|&b| b == SENTINEL));
            assert!(bytes[end..].iter().all(|&b| b == SENTINEL));
            changed += bytes[start..end].iter().filter(|&&b| b != SENTINEL).count();
            cases += 1;
        }
    }
    // Never require an individual random byte to differ from its sentinel.
    assert!(
        changed > 1000,
        "{label}: no evidence of entropy being written"
    );
    println!("PASS: {label} {cases} aligned/partial fills; untouched sentinels");
}

fn main() {
    check("getrandom 0.2.17", |bytes| {
        getrandom02::getrandom(bytes).unwrap()
    });
    check("getrandom 0.3.4", |bytes| getrandom03::fill(bytes).unwrap());
    let mut any_nonzero = false;
    for _ in 0..32 {
        any_nonzero |= getrandom03::u32().unwrap() != 0;
        any_nonzero |= getrandom03::u64().unwrap() != 0;
    }
    assert!(any_nonzero, "scalar RNG imports returned only zeros");
    println!("PASS: WASI 0.3 entropy backports");
}
