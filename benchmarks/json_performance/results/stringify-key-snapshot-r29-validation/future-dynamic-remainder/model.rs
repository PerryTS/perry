fn fast_remainder(a: f64, b: f64) -> (f64, bool) {
    if !a.is_sign_negative() {
        let dividend = a as u32;
        let divisor = b as u32;
        if divisor != 0 && dividend as f64 == a && divisor as f64 == b {
            return ((dividend % divisor) as f64, true);
        }
    }
    (a % b, false)
}
fn main() {
    let mut cases = 0u64;
    let mut admitted = 0u64;
    let mut verify = |a: f64, b: f64| {
        let (actual, fast) = fast_remainder(std::hint::black_box(a), std::hint::black_box(b));
        let expected = std::hint::black_box(a) % std::hint::black_box(b);
        assert!(actual.to_bits() == expected.to_bits() || (actual.is_nan() && expected.is_nan()), "a={a:?} b={b:?}, {actual:?} != {expected:?}");
        if fast {
            assert!(a.is_finite() && b.is_finite() && !a.is_sign_negative() && b > 0.0);
            assert!(a <= u32::MAX as f64 && b <= u32::MAX as f64 && a.trunc() == a && b.trunc() == b);
        }
        cases += 1;
        admitted += fast as u64;
    };
    let mut edges = vec![0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::from_bits(1), -f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX];
    for n in [1.0f64, 2.0, 255.0, 256.0, 65535.0, 65536.0, 2147483647.0, 2147483648.0, 4294967294.0, 4294967295.0, 4294967296.0, 9007199254740991.0, 9007199254740992.0] {
        for x in [n, n + 0.5, n - 0.5, f64::from_bits(n.to_bits() - 1), f64::from_bits(n.to_bits() + 1)] { edges.push(x); edges.push(-x); }
    }
    for &a in &edges { for &b in &edges { verify(a, b); } }
    for a in 0..=255 { for b in 0..=255 { verify(a as f64, b as f64); verify(-(a as f64), b as f64); } }
    let mut state = 0x7f4a7c159e3779b9u64;
    let mut next = || { state ^= state << 13; state ^= state >> 7; state ^= state << 17; state };
    for i in 0..8_000_000 {
        let a = next(); let b = next();
        if i % 2 == 0 { verify(a as u32 as f64, b as u32 as f64); }
        else { verify(f64::from_bits(a), f64::from_bits(b)); }
    }
    println!("{{\"cases\":{cases},\"admitted\":{admitted},\"declined\":{},\"result\":\"PASS\",\"signed_zero_bitwise\":true,\"nan_comparison\":\"both NaN\",\"production_test\":false}}", cases-admitted);
}
