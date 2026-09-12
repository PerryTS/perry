use super::*;
use validator::{ValidateEmail, ValidateUrl};

#[test]
fn uuid_matches_previous_grammar_under_edits() {
    let reference = regex::Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .unwrap();
    let original = "550e8400-e29b-41d4-a716-446655440000";
    let mut checked = 0;
    let mut check = |s: &str| {
        assert_eq!(is_uuid(s), reference.is_match(s), "{s:?}");
        checked += 1;
    };
    for at in 0..original.len() {
        for c in 0..=255u8 {
            let mut s = original.to_owned();
            s.replace_range(at..at + 1, &char::from(c).to_string());
            check(&s);
        }
        let mut s = original.to_owned();
        s.remove(at);
        check(&s);
    }
    for at in 0..=original.len() {
        for c in ['0', '-', '\0', '\n', 'é', '𝟘'] {
            let mut s = original.to_owned();
            s.insert(at, c);
            check(&s);
        }
    }
    for s in [
        "00000000-0000-0000-0000-000000000000",
        "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF",
        "550E8400-e29B-F1d4-0716-446655440000",
        "",
    ] {
        check(s);
    }
    assert_eq!(checked, 9478);
}

#[test]
fn email_matches_previous_grammar_for_short_structures() {
    // Exercise every placement of the grammar's punctuation, including the
    // previous unanchored IP-literal search, against the actual old library.
    let alphabet = ['a', '0', '-', '.', '[', ']', ':', '@', '_', '!'];
    let mut checked = 0;
    for len in 0..=4 {
        for mut code in 0..alphabet.len().pow(len) {
            let mut s = String::new();
            for _ in 0..len {
                s.push(alphabet[code % alphabet.len()]);
                code /= alphabet.len();
            }
            for input in [s.clone(), format!("{s}@a"), format!("a@{s}")] {
                assert_eq!(is_email(&input), input.validate_email(), "{input:?}");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 33333);
}

#[test]
fn email_preserves_unicode_lengths_ip_literals_and_idna() {
    let local_parts = [
        "a".to_owned(),
        "a".repeat(63),
        "a".repeat(64),
        "a".repeat(65),
        "!#$%&'*+/=?^_`{|}~.-".to_owned(),
        "é".repeat(32),
        "a\n".to_owned(),
        "".to_owned(),
    ];
    let mut domains: Vec<String> = [
        "localhost",
        "a.b",
        "a..b",
        ".a",
        "a.",
        "-a",
        "a-",
        "a_b",
        "127.0.0.1",
        "[127.0.0.1]",
        "[127.0.0.256]",
        "[01.2.3.4]",
        "[2001:dB8::1]",
        "[::ffff:127.0.0.1]",
        "[2001:db8::12345]",
        "[::1%eth0]",
        "prefix[127.0.0.1]",
        "[[::1]",
        "[::1]suffix",
        "[::1]\n",
        "[::1]\r\n",
        "a\0b",
        "a\nb",
        "exam_ple.com",
        "例え.テスト",
        "उदाहरण.परीक्षा",
        "bücher.de",
        "xn--bcher-kva.de",
        "K.com",
        "Ａ.com",
        "。",
        "a。b",
        "a。",
        "a\u{200d}b.com",
        "a\u{200c}b.com",
        "a\u{00ad}b.com",
        "a\u{0301}.com",
        "😀.com",
        "é[::1]",
        "é",
        "",
        "[::]",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for n in [1, 62, 63, 64, 254, 255, 256] {
        domains.push("a".repeat(n));
        domains.push(format!("{}.com", "a".repeat(n)));
        domains.push("é".repeat(n));
        domains.push(format!("{}[::1]", "é".repeat(n)));
    }
    for n in [252, 253, 254, 255, 256] {
        let mut s = "a.".repeat(n / 2);
        if n % 2 != 0 {
            s.push('a');
        }
        domains.push(s);
    }
    for local in &local_parts {
        for domain in &domains {
            let s = format!("{local}@{domain}");
            assert_eq!(is_email(&s), s.validate_email(), "{s:?}");
        }
    }
    assert!(
        is_email("a@prefix[127.0.0.1]"),
        "retain the prior suffix behavior"
    );
    assert!(!is_email("a@[127.0.0.1]\n"));
}

#[test]
fn email_preserves_each_byte_in_local_and_domain_positions() {
    for b in 0..=255u8 {
        let c = char::from(b);
        for s in [
            format!("{c}@example.com"),
            format!("a{c}b@example.com"),
            format!("a@{c}b.com"),
            format!("a@a{c}b.com"),
            format!("a@ab{c}.com"),
            format!("a@{c}[::1]"),
            format!("a@[127.0.0.{c}]"),
            format!("a@[::{c}]"),
        ] {
            assert_eq!(is_email(&s), s.validate_email(), "{s:?}");
        }
    }
}

#[test]
fn url_uses_the_same_parser_as_the_previous_trait() {
    for s in [
        "https://example.com",
        "http://localhost:80",
        "ftp://host/",
        "mailto:a@b",
        "file:///a",
        "data:,x",
        "https://例え.テスト/a",
        "http",
        "//example.com",
        "",
        "https://[::1]/",
        "https://[invalid]/",
        "https://x\n.y",
    ] {
        assert_eq!(is_url(s), s.validate_url(), "{s:?}");
    }
}
