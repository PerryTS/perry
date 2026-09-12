//! The tooling API's own contract. Agreement with the `regex` crate is checked
//! separately, against the crate itself, in `tests/dialect_parity.rs`.
use super::dialect::translate;
use super::*;

#[test]
fn finds_and_slices_by_byte_offset() {
    let re = Regex::new(r"\w+").unwrap();
    let m = re.find("  hello world").unwrap();
    assert_eq!(m.as_str(), "hello");
    assert_eq!((m.start(), m.end()), (2, 7));
}

#[test]
fn offsets_are_bytes_even_when_the_engine_counts_utf16_units() {
    // Past the BMP a character is two UTF-16 units and four bytes, so a span
    // that was not converted would slice this wrongly or panic on a boundary.
    let re = Regex::new(r"needle").unwrap();
    let text = "😀😀 needle 😀";
    let m = re.find(text).unwrap();
    assert_eq!(m.as_str(), "needle");
    assert_eq!(&text[m.range()], "needle");

    let re = Regex::new(r"[0-9]+").unwrap();
    let text = "héllo 42 wörld 7";
    let found: Vec<_> = re.find_iter(text).map(|m| m.as_str()).collect();
    assert_eq!(found, ["42", "7"]);
}

#[test]
fn captures_are_addressable_by_index_and_by_name() {
    let re = Regex::new(r"(?P<user>\w+)@(\w+)\.com").unwrap();
    let caps = re.captures("mail user@example.com now").unwrap();
    assert_eq!(&caps[0], "user@example.com");
    assert_eq!(&caps[1], "user");
    assert_eq!(&caps[2], "example");
    assert_eq!(caps.name("user").unwrap().as_str(), "user");
    assert!(caps.name("missing").is_none());
    assert!(caps.get(9).is_none());
}

#[test]
fn an_unset_group_is_none_not_empty() {
    let re = Regex::new(r"a(b)?c").unwrap();
    let caps = re.captures("ac").unwrap();
    assert!(caps.get(1).is_none());
    let caps = re.captures("abc").unwrap();
    assert_eq!(caps.get(1).unwrap().as_str(), "b");
}

#[test]
fn replace_all_borrows_when_nothing_matches() {
    let re = Regex::new(r"[0-9]+").unwrap();
    assert_eq!(re.replace_all("a1b22c333", "#"), "a#b#c#");
    assert!(matches!(re.replace_all("none", "#"), Cow::Borrowed("none")));
}

#[test]
fn escape_output_round_trips_as_a_literal() {
    for text in ["a.b*c(d)", "r$1", "x#y&z-w~v", "[^]{}|\\"] {
        let re = Regex::new(&format!("^{}$", escape(text))).unwrap();
        assert!(re.is_match(text), "{text}");
        assert!(!re.is_match(&format!("{text}!")), "{text}");
    }
}

#[test]
fn a_rejected_pattern_is_an_error_not_a_panic() {
    assert!(Regex::new(r"(").is_err());
    assert!(Regex::new(r"a{").is_err());
    assert!(Regex::new(r"[a").is_err());
}

#[test]
fn flags_become_engine_flags_or_are_spelt_out() {
    assert_eq!(translate(r"(?i)abc").unwrap().flags, "iu");
    assert_eq!(translate(r"abc").unwrap().flags, "u");
    // `m` and `s` are never passed through; the anchors and `.` they govern are
    // written out so that only `\n` is a line break, as in `regex`.
    assert_eq!(
        translate(r"(?m)^a$").unwrap().source,
        r"(?<![^\n])a(?![^\n])"
    );
    assert_eq!(translate(r"^a$").unwrap().source, r"^a$");
    assert_eq!(translate(r"(?s).").unwrap().source, r"[^]");
    assert_eq!(translate(r".").unwrap().source, r"[^\n]");
}

#[test]
fn only_a_crlf_newline_starts_a_line() {
    let re = Regex::new(r"(?m)^import").unwrap();
    let text = "x\r\nimport\rimport\u{2028}import";
    let starts: Vec<_> = re.find_iter(text).map(|m| m.start()).collect();
    assert_eq!(starts, [3]);
}

#[test]
fn shorthand_classes_are_unicode_as_in_regex() {
    assert!(Regex::new(r"^\d$").unwrap().is_match("\u{664}"));
    assert!(Regex::new(r"^\w+$").unwrap().is_match("café"));
    assert!(Regex::new(r"^\s$").unwrap().is_match("\u{85}"));
    assert!(!Regex::new(r"^\s$").unwrap().is_match("\u{feff}"));
    assert!(Regex::new(r"^[\w\s]+$").unwrap().is_match("café au lait"));
    // `\b` is the boundary of that `\w`: no boundary inside "café".
    assert!(!Regex::new(r"caf\b").unwrap().is_match("café"));
    assert!(Regex::new(r"\bcafé\b").unwrap().is_match("un café."));
}

#[test]
fn a_bounded_window_counts_characters_not_units() {
    let re = Regex::new(r"^.{2}$").unwrap();
    assert!(re.is_match("😀😀"));
}

#[test]
fn case_folding_is_unicode_as_in_regex() {
    let re = Regex::new(r"(?i)discord").unwrap();
    assert!(re.is_match("DI\u{17f}CORD"));
    let re = Regex::new(r"(?i)token").unwrap();
    assert!(re.is_match("TO\u{212a}EN"));
}

#[test]
fn iteration_skips_an_empty_match_where_the_last_one_ended() {
    // `regex` reports `a*` over "baaa" as 0..0 and 1..4, but not the empty
    // match at 4 that ECMAScript's `matchAll` would.
    let re = Regex::new(r"a*").unwrap();
    let spans: Vec<_> = re.find_iter("baaa").map(|m| m.range()).collect();
    assert_eq!(spans, [0..0, 1..4]);
    let re = Regex::new(r"b*").unwrap();
    assert_eq!(re.find_iter("aaa").count(), 4);
    // Never inside a character.
    let spans: Vec<_> = Regex::new(r"")
        .unwrap()
        .find_iter("é😀")
        .map(|m| m.start())
        .collect();
    assert_eq!(spans, [0, 2, 6]);
}

#[test]
fn what_cannot_be_translated_is_refused_with_a_reason() {
    for (pattern, reason) in [
        (r"(?x)a b", "the `x` flag"),
        (r"(?U)a+", "the `U` flag"),
        (r"a(?i)b", "inline flags"),
        (r"(?i:a)", "inline flags"),
        (r"(a)+", "a capture inside a repeated group"),
        (r"(?:(a)|b)*", "a capture inside a repeated group"),
        (r"(?:x(a)?){2}", "a capture inside a repeated group"),
        (r"[[:alpha:]]", "a nested or POSIX class"),
        (r"[a&&b]", "class intersection"),
        (r"[^\W]", "`\\W` inside a class"),
        (r"\1", "backreference"),
        (r"\Q", "no ECMAScript equivalent"),
    ] {
        let error = translate(pattern).expect_err(pattern);
        assert!(
            error.reason.contains(reason),
            "{pattern}: `{}` does not mention `{reason}`",
            error.reason
        );
        assert!(
            matches!(Regex::new(pattern), Err(Error::Dialect(_))),
            "{pattern}"
        );
    }
}

#[test]
fn a_capture_that_repeats_at_most_once_is_not_refused() {
    for pattern in [r"(a)?", r"(a){1}", r"(a){0,1}", r"(?:(a)b)?", r"((a)b)"] {
        assert!(translate(pattern).is_ok(), "{pattern}");
    }
}

#[test]
fn punctuation_escapes_survive_u_mode() {
    // `\"` and `\#` are fine in `regex` and syntax errors in ECMAScript `u`
    // mode, where only syntax characters may be escaped.
    let re = Regex::new(r##"\"(x)\"\#\&\~"##).unwrap();
    assert!(re.is_match(r##""x"#&~"##));
    let re = Regex::new(r#"[\"'\-]+"#).unwrap();
    assert_eq!(re.find(r#"a"'-b"#).unwrap().as_str(), r#""'-"#);
    // A literal `]` or `}` is legal in `regex` and needs its escape added.
    assert!(Regex::new(r"a]}").unwrap().is_match("a]}"));
}
