use super::*;
use crate::string::js_string_from_bytes;

fn make_string(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

#[test]
fn js_replacement_expands_special_patterns() {
    let re = regex::Regex::new(r"(\w+)\s(\w+)").unwrap();
    let subj = "John Smith";
    let caps = re.captures(subj).unwrap();
    assert_eq!(
        expand_js_replacement("$2 $1", &caps, subj, false),
        "Smith John"
    );
    assert_eq!(
        expand_js_replacement("[$&]", &caps, subj, false),
        "[John Smith]"
    );

    // $` (before) / $' (after) with a mid-string single-char match.
    let re2 = regex::Regex::new("b").unwrap();
    let s2 = "abc";
    let c2 = re2.captures(s2).unwrap();
    assert_eq!(expand_js_replacement("$`", &c2, s2, false), "a");
    assert_eq!(expand_js_replacement("$'", &c2, s2, false), "c");
    assert_eq!(expand_js_replacement("$&", &c2, s2, false), "b");
    assert_eq!(expand_js_replacement("$$", &c2, s2, false), "$"); // escaped literal
    assert_eq!(expand_js_replacement("$z", &c2, s2, false), "$z"); // invalid → literal
    assert_eq!(expand_js_replacement("end$", &c2, s2, false), "end$"); // trailing $

    // Numbered groups: two-digit-then-one-digit fallback + unmatched → "".
    let re3 = regex::Regex::new(r"(a)(x)?(b)").unwrap();
    let s3 = "ab";
    let c3 = re3.captures(s3).unwrap();
    assert_eq!(expand_js_replacement("$1$2$3", &c3, s3, false), "ab"); // $2 unmatched → ""
    assert_eq!(expand_js_replacement("$10", &c3, s3, false), "a0"); // no group 10 → $1 then '0'
}

#[test]
fn js_replacement_named_group_gate() {
    // No named groups in the regex → `$<name>` is emitted literally (#2421).
    let re = regex::Regex::new("n").unwrap();
    let subj = "end";
    let caps = re.captures(subj).unwrap();
    assert_eq!(
        expand_js_replacement("$<bad>", &caps, subj, false),
        "$<bad>"
    );
    assert_eq!(
        expand_js_replacement("[$<bad>]", &caps, subj, false),
        "[$<bad>]"
    );

    // Named groups present: known name substitutes, unknown name → "".
    let re2 = regex::Regex::new(r"(?<first>\w+)\s(?<last>\w+)").unwrap();
    let subj2 = "John Smith";
    let caps2 = re2.captures(subj2).unwrap();
    assert_eq!(
        expand_js_replacement("$<last>, $<first>", &caps2, subj2, true),
        "Smith, John"
    );
    assert_eq!(
        expand_js_replacement("[$<missing>]", &caps2, subj2, true),
        "[]"
    );
}

// ---- #4797: fancy-regex fallback wired through every operation ----

#[test]
fn fancy_backreference_match() {
    // `(\w)\1` needs backreferences → fancy-regex fallback.
    let re = js_regexp_new(make_string(r"(\w)\1"), make_string(""));
    let result = js_string_match(make_string("hello"), re);
    assert!(!result.is_null());
    unsafe {
        let v = crate::array::js_array_get_f64(result, 0);
        let sp = crate::value::js_get_string_pointer_unified(v) as *const StringHeader;
        assert_eq!(string_as_str(sp), "ll");
    }
}

#[test]
fn fancy_lookbehind_search() {
    let re = js_regexp_new(make_string(r"(?<==)\w+"), make_string(""));
    assert_eq!(js_string_search_regex(make_string("foo=bar"), re), 4);
    // No match → -1.
    let re2 = js_regexp_new(make_string(r"(?<==)\w+"), make_string(""));
    assert_eq!(js_string_search_regex(make_string("nomatch"), re2), -1);
}

#[test]
fn fancy_lookbehind_split() {
    // Zero-width lookbehind split: "a1b2c3" → ["a1","b2","c3",""].
    let re = js_regexp_new(make_string(r"(?<=\d)"), make_string(""));
    let arr = js_string_split_regex(make_string("a1b2c3"), re);
    unsafe {
        assert_eq!((*arr).length, 4);
        let first = crate::array::js_array_get_f64(arr, 0);
        let sp = crate::value::js_get_string_pointer_unified(first) as *const StringHeader;
        assert_eq!(string_as_str(sp), "a1");
    }
}

#[test]
fn fancy_lookbehind_replace_string() {
    // `$&` substitution under a lookbehind pattern the regex crate rejects.
    let re = js_regexp_new(make_string(r"(?<=\$)\d+"), make_string("g"));
    let out = js_string_replace_regex(make_string("$5 and $10"), re, make_string("[$&]"));
    assert_eq!(string_as_str(out), "$[5] and $[10]");
}

#[test]
fn fancy_named_group_replace() {
    // `$<n>` named-group substitution through the fancy fallback.
    let re = js_regexp_new(make_string(r"(?<=\$)(?<n>\d+)"), make_string("g"));
    let out = js_string_replace_regex_named(make_string("$5 and $10"), re, make_string("[$<n>]"));
    assert_eq!(string_as_str(out), "$[5] and $[10]");
}

#[test]
fn fancy_lookbehind_exec_index() {
    // exec() through the fancy path reports the char index of the match.
    let re = js_regexp_new(make_string(r"(?<=\$)\d+"), make_string(""));
    let result = js_regexp_exec(re, make_string("price: $42"));
    assert!(!result.is_null());
    assert_eq!(js_regexp_exec_get_index(), 8.0);
    unsafe {
        let v = crate::array::js_array_get_f64(result, 0);
        let sp = crate::value::js_get_string_pointer_unified(v) as *const StringHeader;
        assert_eq!(string_as_str(sp), "42");
    }
}

#[test]
fn test_regexp_test_basic() {
    let pattern = make_string("hello");
    let flags = make_string("");
    let re = js_regexp_new(pattern, flags);

    let test_str = make_string("hello world");
    assert!(js_regexp_test(re, test_str) != 0);

    let test_str2 = make_string("goodbye world");
    assert!(js_regexp_test(re, test_str2) == 0);
}

#[test]
fn test_regexp_test_case_insensitive() {
    let pattern = make_string("hello");
    let flags = make_string("i");
    let re = js_regexp_new(pattern, flags);

    let test_str = make_string("HELLO World");
    assert!(js_regexp_test(re, test_str) != 0);
}

#[test]
fn test_string_match() {
    let pattern = make_string(r"\w+");
    let flags = make_string("");
    let re = js_regexp_new(pattern, flags);

    let test_str = make_string("hello world");
    let result = js_string_match(test_str, re);
    assert!(!result.is_null());

    unsafe {
        assert_eq!((*result).length, 1); // One match (first word)
    }
}

#[test]
fn test_string_match_global() {
    let pattern = make_string(r"\w+");
    let flags = make_string("g");
    let re = js_regexp_new(pattern, flags);

    let test_str = make_string("hello world");
    let result = js_string_match(test_str, re);
    assert!(!result.is_null());

    unsafe {
        assert_eq!((*result).length, 2); // Two matches (hello, world)
    }
}

#[test]
fn test_string_replace() {
    let pattern = make_string("world");
    let flags = make_string("");
    let re = js_regexp_new(pattern, flags);

    let test_str = make_string("hello world");
    let replacement = make_string("universe");
    let result = js_string_replace_regex(test_str, re, replacement);

    assert_eq!(string_as_str(result), "hello universe");
}

#[test]
fn test_string_replace_global() {
    let pattern = make_string("o");
    let flags = make_string("g");
    let re = js_regexp_new(pattern, flags);

    let test_str = make_string("hello world");
    let replacement = make_string("0");
    let result = js_string_replace_regex(test_str, re, replacement);

    assert_eq!(string_as_str(result), "hell0 w0rld");
}

#[test]
fn escaped_hyphen_in_class_stays_literal() {
    // #4425: `\-` inside a character class is always a literal hyphen. The
    // Rust `regex` crate reads a bare `-` flanked by members as a range
    // operator, so the escape must be preserved or `[a\- ]` translates to
    // the invalid range `[a- ]`.
    assert_eq!(js_regex_to_rust(r"[a\- ]"), r"[a\- ]");
    assert_eq!(js_regex_to_rust(r"[:\- ]"), r"[:\- ]");
    assert_eq!(js_regex_to_rust(r"[\-]"), r"[\-]");
    // Outside a class a hyphen carries no range meaning, so it stays bare.
    assert_eq!(js_regex_to_rust(r"a\-b"), "a-b");

    // The patterns that crashed `marked` at module-init must now compile.
    for pat in [r"[a\- ]", r"[:\- ]", r" {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n"] {
        let flags = make_string("");
        let re = js_regexp_new(make_string(pat), flags);
        assert!(!re.is_null(), "pattern failed to construct: {pat}");
    }
}

#[test]
fn annexb_legacy_decimal_escapes() {
    // #5594: a `\<n>` with no matching capture group is an Annex B.1.4
    // legacy octal escape, not a backreference — `\1` → `\x01`, never the
    // bare `\1` the `regex`/`fancy-regex` crates reject.
    assert_eq!(js_regex_to_rust(r"\1"), r"\x{01}");
    assert_eq!(js_regex_to_rust(r"\b(\w+) \2\b"), r"\b(\w+) \x{02}\b");
    // Multi-digit octal: `\12` = 0o12 = 0x0A, `\14` = 0o14 = 0x0C.
    assert_eq!(js_regex_to_rust(r"[\12-\14]"), r"[\x{0A}-\x{0C}]");
    // Inside a class a decimal escape is always octal, never a backref —
    // even when that group exists.
    assert_eq!(js_regex_to_rust(r"(a)[\1]"), r"(a)[\x{01}]");
    // A real backward backreference is preserved for fancy-regex.
    assert_eq!(js_regex_to_rust(r"(a)\1"), r"(a)\1");
    // `\8` / `\9` are non-octal decimal escapes → literal digit.
    assert_eq!(js_regex_to_rust(r"\8"), "8");
    // `\0` is NUL; legacy `\012` = 0o12 = 0x0A.
    assert_eq!(js_regex_to_rust(r"\0"), r"\x{00}");
    assert_eq!(js_regex_to_rust(r"\012"), r"\x{0A}");

    // The patterns that threw at construction must now compile and behave.
    for pat in [r"\1", r"\b(\w+) \2\b", r"[\d][\12-\14]{1,}[^\d]"] {
        let re = js_regexp_new(make_string(pat), make_string(""));
        assert!(!re.is_null(), "pattern failed to construct: {pat}");
    }
}

#[test]
fn annexb_invalid_control_escape_is_literal_backslash_c() {
    // #5594: `\c` not followed by an ASCII control letter is the literal
    // two-char sequence `\c`, not a control escape. The `regex`/`fancy-regex`
    // crates reject a bare `\c`, so emit an escaped backslash + `c`.
    assert_eq!(js_regex_to_rust(r"\cА"), r"\\cА"); // Cyrillic А (U+0410)
    assert_eq!(js_regex_to_rust(r"\c "), r"\\c "); // space follows
    assert_eq!(js_regex_to_rust(r"\c"), r"\\c"); // trailing
    assert_eq!(js_regex_to_rust(r"[\c ]"), r"[\\c ]"); // inside a class
                                                       // A valid control letter still lowers to its control byte (`\cA` = 0x01).
    assert_eq!(js_regex_to_rust(r"\cA"), r"\x{01}");

    for pat in [r"\cА", r"\c!", r"[\c ]"] {
        let re = js_regexp_new(make_string(pat), make_string(""));
        assert!(!re.is_null(), "pattern failed to construct: {pat}");
    }
}

#[test]
fn surrogate_pairs_fold_to_astral_scalars() {
    // High escape + low class → contiguous astral range.
    assert_eq!(
        js_regex_to_rust(r"\uD800[\uDC00-\uDC0B]"),
        r"[\x{10000}-\x{1000b}]"
    );
    // Two consecutive surrogate escapes → single astral scalar.
    assert_eq!(js_regex_to_rust(r"\uD83D\uDE00"), r"\x{1f600}");
    // High class + full low class → coalesced astral block.
    assert_eq!(
        js_regex_to_rust(r"[\uD80C\uD81C-\uD820][\uDC00-\uDFFF]"),
        r"[\x{13000}-\x{133ff}\x{17000}-\x{183ff}]"
    );
    // Non-surrogate escapes and ordinary classes are untouched.
    assert_eq!(js_regex_to_rust(r"[ˁ\xAA]"), r"[ˁ\xAA]");
    assert_eq!(js_regex_to_rust(r"[A-Za-z]"), r"[A-Za-z]");
    // A lone high surrogate (no following low surrogate) cannot be represented in
    // Rust's Unicode-only `regex` crate — lone surrogates are not valid Unicode
    // scalars and cannot appear in any UTF-8 string. Leaving `\uD800` verbatim
    // would cause the Rust regex engine to reject the pattern at construction time.
    // We emit a never-match atom `[^\s\S]` so the compiled pattern is valid but
    // correctly matches nothing (JS/WTF-8 lone-surrogate matching is a known gap).
    assert_eq!(js_regex_to_rust(r"\uD800x"), r"[^\s\S]x");

    // The Test262 `nativeFunctionMatcher.js` ID regexes must now compile.
    let pat = r"(?:[A-Za-z\xAA]|\uD800[\uDC00-\uDC0B\uDC0D-\uDC26]|\uD801[\uDC00-\uDC9D])";
    let flags = make_string("");
    let re = js_regexp_new(make_string(pat), flags);
    assert!(!re.is_null(), "ID_Start-shaped pattern failed to construct");
}

/// `@colors/colors` (a winston dep) builds the escape regex
/// `escapeStringRegexp = s => s.replace(/[|\\{}()[\]^$+*?.]/g, '\\$&')`
/// and then `new RegExp(escapeStringRegexp(ansiStyles[k].close), 'g')` where
/// `close` is e.g. `"\x1b[0m"`. Node escapes the literal `[` to `\[`, giving
/// the valid pattern `\x1b\[0m`. Perry must do the same: the char-class
/// `[|\\{}()[\]^$+*?.]` contains a *literal* `[` (legal in a JS class but not
/// in the Rust `regex` crate) and an escaped `\]`. If the class compiles
/// empty or the `[` isn't a member, `escapeStringRegexp` returns its input
/// unchanged, the bare `[0m` reaches `new RegExp`, and you get
/// `SyntaxError: Invalid regular expression: /[0m/`. This pins the whole
/// build + match + `$&`-expand path against that regression.
#[test]
fn colors_escape_string_regexp_char_class() {
    let pat = r"[|\\{}()[\]^$+*?.]";
    // Source is preserved verbatim (no empty `(?:)`).
    let re = js_regexp_new(make_string(pat), make_string("g"));
    assert!(
        !re.is_null(),
        "@colors char-class pattern failed to construct"
    );
    let src = js_regexp_get_source(re);
    assert_eq!(string_as_str(src), pat, "source must round-trip the class");

    // The literal `[` is a member of the class.
    assert!(
        js_regexp_test(re, make_string("[")) != 0,
        "`[` must match the class"
    );

    // `escapeStringRegexp("\x1b[0m")` → `"\x1b\\[0m"` (only `[` is escaped;
    // ESC and the digits/`m` are not operators). `$&` → the matched char.
    let out = js_string_replace_regex_named(make_string("\u{1b}[0m"), re, make_string(r"\$&"));
    assert_eq!(
        string_as_str(out),
        "\u{1b}\\[0m",
        "the `[` must be escaped so `new RegExp(out)` is valid"
    );

    // And the escaped output is itself a constructible pattern (what
    // @colors then feeds to `new RegExp(..., 'g')`).
    let re2 = js_regexp_new(out, make_string("g"));
    assert!(!re2.is_null(), "escaped output `\\x1b\\[0m` must construct");
}

/// Regression: emoji-regex (npm `emoji-regex`, used by `string-width` → ink,
/// #348) factors a shared high surrogate out before a non-capturing group, so
/// the high half is no longer directly adjacent to its low half. Before
/// `distribute_high_over_group` the lone `\uD83C`/`\uD83D`/`\uD83E` reached the
/// `regex` crate as a surrogate scalar and the whole pattern was rejected as
/// `invalid pattern` (importing `string-width` then threw at module init).
#[test]
fn high_surrogate_distributes_over_group() {
    // Each shape must now translate to a buildable Rust-regex pattern.
    let patterns = [
        // plain group, alts led by a low-surrogate single or pair
        r"\uD83C(?:\uDDE6\uD83C[\uDDE8-\uDDEC]|\uDDE7🇴)",
        // plain group, alt led by a low-surrogate class
        r"\uD83E(?:[\uDD0C\uDD0F]️?|[\uDD18-\uDD1F])",
        // optional group then a trailing low unit (ZWJ "kiss"/"family" idiom)
        r"\uD83D(?:\uDC8B‍\uD83D)?[\uDC68\uDC69]",
    ];
    for p in patterns {
        let translated = js_regex_to_rust(p);
        assert!(
            build_std_regex(&translated).is_ok(),
            "should compile: {p}\n -> {translated}"
        );
    }

    // Semantics are preserved: the rewrite matches the astral scalars (and the
    // ZWJ sequence), not lone surrogates.
    let re = build_std_regex(&js_regex_to_rust(r"\uD83D(?:\uDC8B‍\uD83D)?[\uDC68\uDC69]")).unwrap();
    assert!(re.is_match("\u{1F468}"), "matches man (U+1F468)");
    assert!(re.is_match("\u{1F469}"), "matches woman (U+1F469)");
    assert!(
        re.is_match("\u{1F48B}\u{200D}\u{1F469}"),
        "matches kiss-ZWJ-woman"
    );
    assert!(!re.is_match("AB"), "does not match plain ASCII");
}

/// Unicode 17.0 scripts (`Beria_Erfe`, `Sidetic`, `Tai_Yo`, `Tolong_Siki`) are
/// absent from `regex-syntax`'s bundled Unicode-16 UCD. Instead of throwing a
/// `SyntaxError` or compiling to a never-matching class, Perry expands them to
/// the explicit code-point ranges Unicode 17 assigns — so `built-ins/RegExp/`
/// `property-escapes` Test262 cases that expect real matches pass. Covers every
/// alias form (`Script`/`sc`/`Script_Extensions`/`scx`) and long + short names.
#[test]
fn unicode17_scripts_expand_to_codepoint_ranges() {
    // Positive `\p{Script=...}` → explicit class of the script's ranges.
    assert_eq!(
        js_regex_to_rust(r"\p{Script=Beria_Erfe}"),
        r"[\x{16EA0}-\x{16EB8}\x{16EBB}-\x{16ED3}]"
    );
    // Short alias, `sc=` key, and `scx=` all resolve to the same body.
    assert_eq!(
        js_regex_to_rust(r"\p{sc=Berf}"),
        r"[\x{16EA0}-\x{16EB8}\x{16EBB}-\x{16ED3}]"
    );
    assert_eq!(
        js_regex_to_rust(r"\p{scx=Beria_Erfe}"),
        r"[\x{16EA0}-\x{16EB8}\x{16EBB}-\x{16ED3}]"
    );
    assert_eq!(
        js_regex_to_rust(r"\p{Script_Extensions=Berf}"),
        r"[\x{16EA0}-\x{16EB8}\x{16EBB}-\x{16ED3}]"
    );
    // The other three scripts.
    assert_eq!(
        js_regex_to_rust(r"\p{sc=Sidetic}"),
        r"[\x{10940}-\x{10959}]"
    );
    assert_eq!(
        js_regex_to_rust(r"\p{sc=Tai_Yo}"),
        r"[\x{1E6C0}-\x{1E6DE}\x{1E6E0}-\x{1E6F5}\x{1E6FE}-\x{1E6FF}]"
    );
    assert_eq!(
        js_regex_to_rust(r"\p{sc=Tolong_Siki}"),
        r"[\x{11DB0}-\x{11DDB}\x{11DE0}-\x{11DE9}]"
    );
    // Negated form → complemented class.
    assert_eq!(
        js_regex_to_rust(r"\P{sc=Sidetic}"),
        r"[^\x{10940}-\x{10959}]"
    );

    // End-to-end: the compiled anchored regex matches the script's own code
    // points and rejects an adjacent non-member (mirrors the Test262 shape).
    let re = js_regexp_new(make_string(r"^\p{Script=Beria_Erfe}+$"), make_string("u"));
    assert!(!re.is_null(), "Beria_Erfe pattern must construct");
    assert!(
        js_regexp_test(re, make_string("\u{16EA0}\u{16EB8}\u{16EBB}\u{16ED3}")) != 0,
        "matches Beria_Erfe code points"
    );
    // U+16EB9/U+16EBA sit in the gap between the two ranges → not members.
    assert!(
        js_regexp_test(re, make_string("\u{16EB9}")) == 0,
        "gap code point U+16EB9 is not Beria_Erfe"
    );

    // Negated: `\P{sc=Sidetic}` matches an ASCII letter, not a Sidetic point.
    let rn = js_regexp_new(make_string(r"^\P{sc=Sidetic}$"), make_string("u"));
    assert!(!rn.is_null(), "negated Sidetic pattern must construct");
    assert!(
        js_regexp_test(rn, make_string("A")) != 0,
        "ASCII is non-Sidetic"
    );
    assert!(
        js_regexp_test(rn, make_string("\u{10940}")) == 0,
        "U+10940 is Sidetic, excluded by the negation"
    );
}
