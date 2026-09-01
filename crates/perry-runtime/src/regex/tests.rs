use super::*;
use crate::string::js_string_from_bytes;

fn make_string(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

fn make_wtf8(bytes: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn string_payload(s: *const StringHeader) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize).to_vec()
    }
}

#[test]
fn regexp_has_dedicated_gc_kind_and_is_not_a_shaped_object() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string("x"));
    let flags = scope.root_string_ptr(make_string("g"));
    let re = pattern.with_mut_ptr::<StringHeader, _>(|pattern| {
        flags.with_mut_ptr::<StringHeader, _>(|flags| js_regexp_new(pattern, flags))
    });
    let gc = unsafe { crate::value::addr_class::try_read_gc_header(re as usize) }
        .expect("RegExp must be a GC allocation");
    assert_eq!(gc.obj_type, crate::gc::GC_TYPE_REGEXP);
    assert!(regex_header_has_magic(re));
    assert!(!unsafe { crate::object::object_is_shaped(re.cast::<crate::object::ObjectHeader>()) });
}

#[test]
fn malloc_finalize_clears_regexp_address_owned_tables() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string("finalize"));
    let flags = scope.root_string_ptr(make_string("g"));
    let re = pattern.with_mut_ptr::<StringHeader, _>(|pattern| {
        flags.with_mut_ptr::<StringHeader, _>(|flags| js_regexp_new(pattern, flags))
    });
    let addr = re as usize;
    assert!(test_regex_pointer_entry_exists(addr));
    assert!(test_regex_source_entry_exists(addr));
    crate::object::exotic_expando::test_seed_exotic_expando_entry(
        addr,
        "owned",
        crate::value::TAG_TRUE,
    );
    assert!(crate::object::exotic_expando::test_exotic_expando_entry_exists(addr));

    unsafe {
        crate::gc::gc_type_finalize_unmarked_payload(crate::gc::GC_TYPE_REGEXP, re.cast::<u8>());
    }

    assert!(!test_regex_pointer_entry_exists(addr));
    assert!(!test_regex_source_entry_exists(addr));
    assert!(!crate::object::exotic_expando::test_exotic_expando_entry_exists(addr));
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

#[test]
fn literal_replace_expands_every_subject_token() {
    let result = js_string_replace_all_string(
        make_string("abcabc"),
        make_string("abc"),
        make_string("$`<$&>$'"),
    );
    assert_eq!(string_as_str(result), "<abc>abcabc<abc>");
}

#[test]
fn literal_replace_all_empty_pattern_splits_astral_utf16_units() {
    let result = js_string_replace_all_string(make_string("😀"), make_string(""), make_string("|"));
    assert_eq!(
        string_payload(result),
        [b'|', 0xED, 0xA0, 0xBD, b'|', 0xED, 0xB8, 0x80, b'|']
    );
    unsafe {
        assert_eq!((*result).utf16_len, 5);
        assert_ne!(
            (*result).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
            0
        );
    }
}

#[test]
fn literal_replace_canonicalizes_a_new_surrogate_boundary() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let low = scope.root_string_ptr(make_wtf8(&[0xED, 0xB8, 0x80]));
    let high = scope.root_string_ptr(make_wtf8(&[0xED, 0xA0, 0xBD]));
    let empty = scope.root_string_ptr(make_string(""));
    let result = low.with_const_ptr(|low: *const StringHeader| {
        empty.with_const_ptr(|empty: *const StringHeader| {
            high.with_const_ptr(|high: *const StringHeader| {
                js_string_replace_string(low, empty, high)
            })
        })
    });
    assert_eq!(string_payload(result), "😀".as_bytes());
    unsafe {
        assert_eq!((*result).utf16_len, 2);
        assert_eq!(
            (*result).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
            0
        );
    }
}

#[test]
fn literal_replace_nonempty_pattern_preserves_wtf8_boundaries() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_string_ptr(make_wtf8(&[0xED, 0xA0, 0xBD, b'X']));
    let pattern = scope.root_string_ptr(make_string("X"));
    let replacement = scope.root_string_ptr(make_wtf8(&[0xED, 0xB8, 0x80]));
    let result = source.with_const_ptr(|source: *const StringHeader| {
        pattern.with_const_ptr(|pattern: *const StringHeader| {
            replacement.with_const_ptr(|replacement: *const StringHeader| {
                js_string_replace_string(source, pattern, replacement)
            })
        })
    });
    assert_eq!(string_payload(result), "😀".as_bytes());
    unsafe {
        assert_eq!((*result).utf16_len, 2);
        assert_eq!(
            (*result).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
            0
        );
    }
}

// ---- #4797: fancy-regex fallback wired through every operation ----

#[test]
fn fancy_backreference_match() {
    // `(\w)\1` needs backreferences → fancy-regex fallback.
    let re = js_regexp_new(make_string(r"(\w)\1"), make_string(""));
    let result = js_string_match(make_string("hello"), re);
    assert!(!result.is_null());
    {
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
    {
        let v = crate::array::js_array_get_f64(result, 0);
        let sp = crate::value::js_get_string_pointer_unified(v) as *const StringHeader;
        assert_eq!(string_as_str(sp), "42");
    }
}

fn match_capture_text(arr: *const ArrayHeader, index: u32) -> Option<String> {
    let value = crate::array::js_array_get_f64(arr, index);
    if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        return None;
    }
    let string = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    Some(string_as_str(string).to_string())
}

#[test]
fn repeat_matcher_resets_nested_captures_each_iteration() {
    let re = js_regexp_new(make_string(r"(z)((a+)?(b+)?(c))*"), make_string(""));
    let matched = js_regexp_exec(re, make_string("zaacbbbcac"));
    assert!(!matched.is_null());
    assert_eq!(
        (0..6)
            .map(|index| match_capture_text(matched, index))
            .collect::<Vec<_>>(),
        vec![
            Some("zaacbbbcac".to_string()),
            Some("z".to_string()),
            Some("ac".to_string()),
            Some("a".to_string()),
            None,
            Some("c".to_string()),
        ]
    );
}

#[test]
fn repeat_matcher_discards_empty_optional_iterations() {
    let re = js_regexp_new(make_string(r"(a?b??)*"), make_string(""));
    let matched = js_regexp_exec(re, make_string("ab"));
    assert!(!matched.is_null());
    assert_eq!(match_capture_text(matched, 0).as_deref(), Some("ab"));
    assert_eq!(match_capture_text(matched, 1).as_deref(), Some("b"));
}

#[test]
fn repeat_matcher_clears_captures_when_optional_lookahead_is_skipped() {
    for pattern in [r"(?:(?=(abc)))?a", r"(?:(?=(abc))){0,1}a"] {
        let re = js_regexp_new(make_string(pattern), make_string(""));
        let matched = js_string_match(make_string("abc"), re);
        assert!(!matched.is_null(), "{pattern}");
        assert_eq!(match_capture_text(matched, 0).as_deref(), Some("a"));
        assert_eq!(match_capture_text(matched, 1), None, "{pattern}");
    }

    for pattern in [r"(?:(?=(abc)))a", r"(?:(?=(abc))){1,1}a"] {
        let re = js_regexp_new(make_string(pattern), make_string(""));
        let matched = js_string_match(make_string("abc"), re);
        assert!(!matched.is_null(), "{pattern}");
        assert_eq!(match_capture_text(matched, 1).as_deref(), Some("abc"));
    }
}

#[test]
fn repeat_matcher_preserves_negative_lookahead_capture_semantics() {
    let re = js_regexp_new(make_string(r"(.*?)a(?!(a+)b\2c)\2(.*)"), make_string(""));
    let result = js_regexp_exec(re, make_string("baaabaac"));
    assert!(!result.is_null());
    assert_eq!(match_capture_text(result, 0).as_deref(), Some("baaabaac"));
    assert_eq!(match_capture_text(result, 1).as_deref(), Some("ba"));
    assert_eq!(match_capture_text(result, 2), None);
    assert_eq!(match_capture_text(result, 3).as_deref(), Some("abaac"));
}

#[test]
fn regex_replace_matches_lone_surrogates_as_utf16_units() {
    let source = make_wtf8(&[0xED, 0xA0, 0x80]);
    let re = js_regexp_new(make_string(r"\S+"), make_string("g"));
    let result = js_string_replace_regex(source, re, make_string("test262"));
    assert_eq!(string_as_str(result), "test262");
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
fn regex_replace_preserves_and_canonicalizes_wtf8_boundaries() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string("X"));
    let flags = scope.root_string_ptr(make_string(""));
    let re = pattern.with_const_ptr(|pattern: *const StringHeader| {
        flags.with_const_ptr(|flags: *const StringHeader| js_regexp_new(pattern, flags))
    });
    let re = scope.root_raw_const_ptr(re);
    let source = scope.root_string_ptr(make_wtf8(&[0xED, 0xA0, 0xBD, b'X']));
    let replacement = scope.root_string_ptr(make_wtf8(&[0xED, 0xB8, 0x80]));
    let result = source.with_const_ptr(|source: *const StringHeader| {
        re.with_const_ptr(|re: *const RegExpHeader| {
            replacement.with_const_ptr(|replacement: *const StringHeader| {
                js_string_replace_regex(source, re, replacement)
            })
        })
    });
    assert_eq!(string_payload(result), "😀".as_bytes());
    unsafe {
        assert_eq!((*result).utf16_len, 2);
        assert_eq!(
            (*result).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
            0
        );
    }
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
fn ecmascript_word_escapes_and_boundaries_use_the_spec_word_set() {
    fn matches(pattern: &str, flags: &str, subject: &str) -> bool {
        let re = js_regexp_new(make_string(pattern), make_string(flags));
        js_regexp_test(re, make_string(subject)) != 0
    }

    // Neither `u` nor `i` alone widens the ASCII set. Rust's native `\w`
    // admits all of these, which was the silent wrong-answer bug.
    for flags in ["", "i", "u"] {
        for subject in ["é", "Ω", "漢", "K", "ſ"] {
            assert!(
                !matches(r"^\w$", flags, subject),
                "/^\\w$/{flags} {subject}"
            );
            assert!(matches(r"^\W$", flags, subject), "/^\\W$/{flags} {subject}");
            assert!(
                !matches(r"^[\w]$", flags, subject),
                "/^[\\w]$/{flags} {subject}"
            );
            assert!(
                matches(r"^[\W]$", flags, subject),
                "/^[\\W]$/{flags} {subject}"
            );
        }
    }

    // `i`+`u` adds exactly the two non-ASCII simple folds into ASCII.
    for subject in ["K", "ſ"] {
        assert!(matches(r"^\w$", "iu", subject));
        assert!(!matches(r"^\W$", "iu", subject));
        assert!(matches(r"^[\w]$", "iu", subject));
        assert!(!matches(r"^[\W]$", "iu", subject));
        assert!(matches(r"^\b.\b$", "iu", subject));
        assert!(!matches(r"^\B.\B$", "iu", subject));
    }
    for subject in ["é", "Ω", "漢"] {
        assert!(!matches(r"^\w$", "iu", subject));
        assert!(matches(r"^\W$", "iu", subject));
        assert!(!matches(r"^\b.\b$", "iu", subject));
        assert!(matches(r"^\B.\B$", "iu", subject));
    }

    // Mixed classes under non-Unicode `i` need separate exact-word and
    // normally-folded arms; the outer Rust `(?i)` must not fold the word arm.
    assert!(!matches(r"^[a\w]+$", "i", "Ω"));
    assert!(matches(r"^[^a\w]+$", "i", "Ω"));
    assert!(matches(r"^[a\W]+$", "i", "Ω"));
    assert!(!matches(r"^[^a\W]+$", "i", "Ω"));
    assert!(matches(r"^[^a\W]+$", "i", "cfx"));
    assert!(!matches(r"^[^a\W]+$", "i", "café"));

    // Boundary word-ness is the same predicate as `\w`.
    assert!(!matches(r"^\bΩ\b$", "", "Ω"));
    assert!(matches(r"^\BΩ\B$", "", "Ω"));
    assert!(matches(r"x\bΩ", "", "xΩ"));
    assert!(!matches(r"x\BΩ", "", "xΩ"));
}

#[test]
fn ecmascript_dot_excludes_all_line_terminators_without_dotall() {
    fn matches(pattern: &str, flags: &str, subject: &str) -> bool {
        let re = js_regexp_new(make_string(pattern), make_string(flags));
        js_regexp_test(re, make_string(subject)) != 0
    }

    for flags in ["", "i", "u", "m", "g"] {
        for terminator in ["\n", "\r", "\u{2028}", "\u{2029}"] {
            assert!(
                !matches(r"^.$", flags, terminator),
                "/^.$/{flags} matched {terminator:?}"
            );
        }
    }
    for terminator in ["\n", "\r", "\u{2028}", "\u{2029}"] {
        assert!(matches(r"^.$", "s", terminator));
        assert!(matches(r"^.$", "isu", terminator));
    }
    assert!(matches(r"^.$", "", "\t"));
    assert!(!matches(r".{2}", "g", "\t\r\n"));
    assert!(matches(r".{2}", "gs", "\t\r\n"));
}

#[test]
fn annexb_legacy_decimal_escapes() {
    // #5594: a `\<n>` with no matching capture group is an Annex B.1.4
    // legacy octal escape, not a backreference — `\1` → `\x01`, never the
    // bare `\1` the `regex`/`fancy-regex` crates reject.
    assert_eq!(js_regex_to_rust(r"\1"), r"\x{01}");
    assert_eq!(
        js_regex_to_rust(r"\b(\w+) \2\b"),
        r"(?-iu:\b)((?-i:[A-Za-z0-9_])+) \x{02}(?-iu:\b)"
    );
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

/// 2026-07-09 GC audit (wave 2 batch A): the compiled-regex caches were
/// unbounded — one entry per distinct `(pattern, flags)` ever compiled, up to
/// 64 MiB each — so `new RegExp(userInput)` was an attacker-driven OOM. The
/// caches are now capped (clear-on-overflow) and every `RegExpHeader` OWNS a
/// leaked Arc reference to its compiled program(s), so a header created
/// before an eviction keeps matching afterwards.
#[test]
fn regex_cache_capped_and_prior_headers_survive_eviction() {
    // A header compiled before the flood.
    let re = js_regexp_new(make_string(r"needle\d+"), make_string(""));
    assert!(js_regexp_test(re, make_string("xx needle42 yy")) != 0);

    // A fancy-fallback header too (lookbehind forces the fancy engine).
    let fancy = js_regexp_new(make_string(r"(?<=pre)\d+"), make_string(""));
    assert!(js_regexp_test(fancy, make_string("pre77")) != 0);

    // A RepeatMatcher header whose ECMAScript matcher must likewise outlive
    // its thread-local cache entry.
    let repeat_matcher = js_regexp_new(make_string(r"(a?b??)*"), make_string(""));
    assert!(js_regexp_test(repeat_matcher, make_string("ab")) != 0);

    // Flood the cache with distinct patterns — far past the cap.
    for i in 0..(REGEX_CACHE_MAX_ENTRIES * 2 + 10) {
        let _ = get_or_compile_regex(&format!("cachefill{i}[a-z]+"), "");
    }
    let std_len = REGEX_CACHE.with(|c| c.borrow().len());
    assert!(
        std_len <= REGEX_CACHE_MAX_ENTRIES,
        "REGEX_CACHE must stay capped at {REGEX_CACHE_MAX_ENTRIES} entries, got {std_len}"
    );

    // Flood the fancy cache as well (each pattern rejected by the std engine).
    for i in 0..(REGEX_CACHE_MAX_ENTRIES + 10) {
        let _ = get_or_compile_regex(&format!("(?<=fill{i})x"), "");
    }
    let fancy_len = FANCY_CACHE.with(|c| c.borrow().len());
    assert!(
        fancy_len <= REGEX_CACHE_MAX_ENTRIES,
        "FANCY_CACHE must stay capped at {REGEX_CACHE_MAX_ENTRIES} entries, got {fancy_len}"
    );

    // Quantified captures populate the ECMAScript RepeatMatcher cache.
    for i in 0..(REGEX_CACHE_MAX_ENTRIES + 10) {
        let _ = get_or_compile_regex(&format!("(repeat{i})*"), "");
    }
    let repeat_len = REPEAT_MATCHER_CACHE.with(|c| c.borrow().len());
    assert!(
        repeat_len <= REGEX_CACHE_MAX_ENTRIES,
        "REPEAT_MATCHER_CACHE must stay capped at {REGEX_CACHE_MAX_ENTRIES} entries, got {repeat_len}"
    );

    // The pre-flood headers still execute correctly: their compiled programs
    // are owned by the headers (leaked Arc refs), not borrowed from the
    // now-cleared caches.
    assert!(
        js_regexp_test(re, make_string("xx needle42 yy")) != 0,
        "std-engine header must keep matching after cache eviction"
    );
    assert!(
        js_regexp_test(re, make_string("no match here")) == 0,
        "std-engine header must keep REJECTING correctly after cache eviction"
    );
    assert!(
        js_regexp_test(fancy, make_string("pre77")) != 0,
        "fancy-fallback header must keep matching after cache eviction \
         (header-resident fancy_ptr, not the cleared FANCY_CACHE)"
    );
    assert!(
        js_regexp_test(fancy, make_string("nope77")) == 0,
        "fancy-fallback header must keep rejecting after cache eviction"
    );
    assert!(
        js_regexp_test(repeat_matcher, make_string("ab")) != 0,
        "RepeatMatcher header must keep matching after cache eviction"
    );
}

// ---------------------------------------------------------------------------
// UTF-16 code-unit indices (#5897)
//
// Every JS-observable string index is counted in UTF-16 code units — the same
// unit `str.length` (`StringHeader::utf16_len`) reports. The regex module used
// to count `chars()` (Unicode scalars) instead, which is only equivalent for
// BMP text: a non-BMP scalar is one `char` but TWO code units.
// ---------------------------------------------------------------------------

#[test]
fn byte_index_to_utf16_index_counts_surrogate_pairs_as_two() {
    // U+1D306 TETRAGRAM FOR CENTRE ("𝌆") is 4 UTF-8 bytes / 1 char / 2 UTF-16 units.
    let s = "𝌆a";
    assert_eq!(byte_index_to_utf16_index(s, 0), 0);
    // Past the astral scalar: 2 code units, NOT 1 (the old `chars().count()`).
    assert_eq!(byte_index_to_utf16_index(s, 4), 2);
    // Past the trailing ASCII 'a'.
    assert_eq!(byte_index_to_utf16_index(s, 5), 3);
    // Matches what `str.length` reports for the same string.
    assert_eq!(
        byte_index_to_utf16_index(s, s.len()),
        s.encode_utf16().count()
    );

    // Pure-BMP text is unchanged (code points == code units).
    let bmp = "héllo";
    assert_eq!(
        byte_index_to_utf16_index(bmp, bmp.len()),
        bmp.encode_utf16().count()
    );

    // Out-of-range byte index clamps to the end rather than panicking.
    assert_eq!(byte_index_to_utf16_index(s, 999), 3);
}

#[test]
fn utf16_index_to_byte_inverts_byte_index_to_utf16_index() {
    let s = "a𝌆b𝌆";
    // Walk every code-unit boundary and confirm the round trip.
    for (byte, ch) in s.char_indices() {
        let u16_idx = byte_index_to_utf16_index(s, byte);
        assert_eq!(
            utf16_index_to_byte(s, u16_idx),
            byte,
            "round trip at {byte}"
        );
        let _ = ch;
    }
    assert_eq!(utf16_index_to_byte(s, 0), 0);
    // Index 1 addresses the LOW surrogate of the first "𝌆" — no UTF-8 boundary
    // of its own, so it resolves just past that scalar.
    assert_eq!(utf16_index_to_byte(s, 2), 1 + 4);
    // At/beyond the end clamps to the buffer length.
    assert_eq!(utf16_index_to_byte(s, 99), s.len());
}

#[test]
fn exec_last_index_advances_by_utf16_code_units() {
    // test262 built-ins/RegExp/prototype/exec/u-lastindex-value:
    //   var r = /./ug; r.exec('𝌆'); assert.sameValue(r.lastIndex, 2);
    // A single astral match must leave `lastIndex` at 2 (the string's `.length`),
    // not 1 (its scalar count).
    let re = js_regexp_new(make_string("."), make_string("ug"));
    let arr = js_regexp_exec(re, make_string("𝌆"));
    assert!(!arr.is_null(), "/./ug must match the astral scalar");
    assert_eq!(
        regex_last_index_offset(re),
        2,
        "lastIndex must be in UTF-16 code units"
    );

    // A second exec finds nothing more and resets lastIndex — proving the
    // UTF-16 lastIndex maps back to a valid byte offset (the end of input).
    let arr2 = js_regexp_exec(re, make_string("𝌆"));
    assert!(
        arr2.is_null(),
        "second exec must not re-match past the input"
    );
    assert_eq!(regex_last_index_offset(re), 0, "no-match resets lastIndex");
}

#[test]
fn global_exec_walks_astral_string_by_code_units() {
    // Two astral scalars: `.` with `u` matches each whole scalar, so lastIndex
    // must land on 2 then 4 — the same indices `str.length` / `charAt` use.
    let re = js_regexp_new(make_string("."), make_string("ug"));
    let subject = "𝌆𝌆";
    assert!(!js_regexp_exec(re, make_string(subject)).is_null());
    assert_eq!(regex_last_index_offset(re), 2);
    assert!(!js_regexp_exec(re, make_string(subject)).is_null());
    assert_eq!(regex_last_index_offset(re), 4);
    // Exhausted → null, lastIndex reset.
    assert!(js_regexp_exec(re, make_string(subject)).is_null());
    assert_eq!(regex_last_index_offset(re), 0);
}

#[test]
fn search_returns_utf16_index() {
    // `"𝌆x".search(/x/)` is 2 (the astral scalar occupies indices 0 and 1),
    // matching `"𝌆x".indexOf("x")`.
    let re = js_regexp_new(make_string("x"), make_string(""));
    assert_eq!(js_string_search_regex(make_string("𝌆x"), re), 2);
}

/// The eager syntax check must accept EXACTLY what the full build accepts.
///
/// `js_regexp_new` no longer answers "is this a `SyntaxError`?" by building the
/// automaton — it asks the standard engine's parser alone
/// (`lazy::std_engine_syntax_ok`) and only falls through to the both-engines
/// path when the parser refuses. That is sound only while parser-acceptance and
/// builder-acceptance agree; if a future `regex` release moves a diagnostic out
/// of the parser and into the NFA build, a pattern would silently stop throwing
/// at construction. This is the gate for that: it disagrees loudly rather than
/// letting the divergence ship.
///
/// Both directions matter, so the corpus deliberately contains patterns the
/// linear engine ACCEPTS, ones it rejects for lack of a feature (lookbehind,
/// backreferences — the fancy-regex fallback's territory) and ones that are
/// genuinely malformed.
#[test]
fn syntax_check_agrees_with_full_build() {
    let corpus: &[(&str, &str)] = &[
        // Ordinary shapes.
        ("abc", ""),
        ("^v?(\\d+)\\.(\\d+)\\.(\\d+)$", ""),
        ("[A-Za-z0-9_.+-]+@[\\w-]+\\.[\\w.-]+", "i"),
        ("(?:https?|ftp)://[^\\s]+", "gi"),
        ("\\s+", "gm"),
        ("a.b", "s"),
        ("(foo|bar|baz){2,4}", "i"),
        ("x{0,250}", ""),
        ("\\d{1,256}", ""),
        // Unicode classes / properties / astral — the case-folding shapes.
        ("[A-Za-zÀ-ɏ]+", "i"),
        ("[Ѐ-ӿͰ-Ͽ]*", "giu"),
        ("\\p{L}+", "u"),
        ("\\p{Script=Greek}", "u"),
        ("[\\u{1F600}-\\u{1F64F}]", "u"),
        ("[←-⇿☀-⛿]", "u"),
        ("\\w+\\b", "iu"),
        // Fancy-only (the linear engine refuses; fancy-regex accepts).
        ("(?<=pre)\\d+", ""),
        ("(?<!x)y", ""),
        ("(?=abc)a", ""),
        ("(a)\\1", ""),
        // Malformed.
        ("(", ""),
        ("[z-a]", ""),
        ("a{2,1}", ""),
        ("[", ""),
        (")", ""),
        ("\\p{Bogus}", "u"),
        ("\\p{Script=Nonsense}", "u"),
        ("[\\p{Bogus}]", "u"),
        ("(?<", ""),
        ("*", ""),
    ];
    let mut disagreements = Vec::new();
    for (pattern, flags) in corpus {
        let cheap = lazy::std_engine_syntax_ok(pattern, flags);
        let full = build_std_regex(&lazy::flag_prefixed_pattern(pattern, flags)).is_ok();
        if cheap != full {
            disagreements.push(format!(
                "/{pattern}/{flags}: parser says {cheap}, full build says {full}"
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "the cheap construction-time syntax check diverged from the full build \
         — construction would throw (or stop throwing) SyntaxError for:\n  {}",
        disagreements.join("\n  ")
    );

    // The corpus above is the committed, readable one. It was developed
    // against a much larger throwaway corpus: every distinct regex literal in
    // the claude-code (2,378), pi and kimi bundles — 3,402 in total — plus
    // 6,297 mutations of the claude-code set (truncations, single-character
    // deletions, an injected `{2,1}`) to load the REJECT direction, since the
    // real-world patterns are all valid by construction. 9,899 patterns, zero
    // disagreements. Point this at such a file to re-run that sweep.
    if let Ok(path) = std::env::var("PERRY_REGEX_CORPUS") {
        let text = std::fs::read_to_string(&path).expect("PERRY_REGEX_CORPUS is unreadable");
        let mut wide = Vec::new();
        let mut n = 0usize;
        for line in text.lines().filter(|l| !l.is_empty()) {
            let mut fields = line.splitn(2, '\t');
            let pattern = fields.next().unwrap();
            let flags = fields.next().unwrap_or("");
            n += 1;
            if lazy::std_engine_syntax_ok(pattern, flags)
                != build_std_regex(&lazy::flag_prefixed_pattern(pattern, flags)).is_ok()
            {
                wide.push(format!("/{pattern}/{flags}"));
            }
        }
        assert!(
            wide.is_empty(),
            "{} of {n} patterns in {path} disagree:\n  {}",
            wide.len(),
            wide.join("\n  ")
        );
    }
}

/// Construction must NOT build the automaton; the first operation that needs a
/// matcher must.
///
/// This is the structural half of the perf fix — the wall-clock half is a
/// fixture whose 200 literals cost 73 ms to construct before and ~0 after. A
/// regression here (something re-introducing an eager build) would not fail any
/// behavioural test, only make every program slower, so assert the state
/// directly: `regex_ptr` is the built/not-built flag.
#[test]
fn construction_defers_the_program_build_until_first_use() {
    let re = js_regexp_new(
        make_string("[A-Za-z]+(?:foo|bar)[0-9]{1,4}"),
        make_string("i"),
    );
    assert!(
        unsafe { (*re).regex_ptr.is_null() },
        "constructing a RegExp must not build its program"
    );
    // Everything observable without matching stays available.
    assert_eq!(
        string_payload(js_regexp_get_source(re)),
        b"[A-Za-z]+(?:foo|bar)[0-9]{1,4}".to_vec()
    );
    assert_eq!(string_payload(js_regexp_get_flags(re)), b"i".to_vec());
    assert!(unsafe { (*re).case_insensitive });
    assert!(
        unsafe { (*re).regex_ptr.is_null() },
        "reading .source/.flags must not build the program either"
    );

    assert!(js_regexp_test(re, make_string("XFOO12")) != 0);
    assert!(
        !unsafe { (*re).regex_ptr.is_null() },
        "the first match must build and install the program"
    );
}

/// The deferred build installs the fancy-regex and RepeatMatcher programs too,
/// not just the linear one — they live on the same publish point, so a header
/// whose pattern needs one must still get it on first use.
#[test]
fn deferred_build_installs_the_fancy_and_repeat_matcher_fallbacks() {
    let fancy = js_regexp_new(make_string(r"(?<=pre)\d+"), make_string(""));
    assert!(unsafe { (*fancy).fancy_ptr.is_null() });
    assert!(js_regexp_test(fancy, make_string("pre77")) != 0);
    assert!(
        !unsafe { (*fancy).fancy_ptr.is_null() },
        "first use must install the fancy-regex fallback"
    );
    assert!(js_regexp_test(fancy, make_string("nope77")) == 0);

    let repeat = js_regexp_new(make_string(r"(a?b??)*"), make_string(""));
    assert!(unsafe { (*repeat).repeat_matcher_ptr.is_null() });
    assert!(js_regexp_test(repeat, make_string("ab")) != 0);
    assert!(
        !unsafe { (*repeat).repeat_matcher_ptr.is_null() },
        "first use must install the ECMAScript RepeatMatcher"
    );
}

/// Two evaluations of the same pattern are still distinct objects with
/// independent `lastIndex`, and deferring the build does not let them share a
/// header (ECMA-262 requires a fresh object per evaluation — the same
/// invariant the closure-literal singleton fix restored for functions).
#[test]
fn deferred_build_keeps_per_object_identity_and_last_index() {
    let a = js_regexp_new(make_string("x"), make_string("g"));
    let b = js_regexp_new(make_string("x"), make_string("g"));
    assert_ne!(
        a as usize, b as usize,
        "each evaluation is a distinct object"
    );
    assert!(!js_regexp_exec(a, make_string("xx")).is_null());
    assert_eq!(regex_last_index_offset(a), 1);
    assert_eq!(
        regex_last_index_offset(b),
        0,
        "a sibling regex must not inherit lastIndex through the shared program"
    );
}

/// The validated-pattern set is capped like the program caches: it holds owned
/// pattern text (`emoji-regex` is ~12,807 chars) and is fed by `new
/// RegExp(userInput)`, so an uncapped one would be the same attacker-driven
/// growth the compiled-program caches were capped for.
#[test]
fn validated_pattern_set_is_capped() {
    for i in 0..(REGEX_CACHE_MAX_ENTRIES * 2 + 10) {
        lazy::mark_pattern_validated(&format!("validfill{i}[a-z]+"), "");
    }
    let len = VALIDATED_PATTERNS.with(|c| c.borrow().len());
    assert!(
        len <= REGEX_CACHE_MAX_ENTRIES,
        "VALIDATED_PATTERNS must stay capped at {REGEX_CACHE_MAX_ENTRIES} entries, got {len}"
    );
}

/// The `[\s\S]` → `(?s:.)` rewrite must not move a single match result.
///
/// The rewrite exists purely to dodge a 1.1-million-iteration case fold in
/// `regex_syntax` (see `grammar::push_any_char`), so the only thing that may
/// change is how long construction takes. Everything a program can observe —
/// what matches, what a capture group holds, which group number it is, and
/// that the NEGATED forms still match nothing — is pinned here, because a
/// silently widened character class produces no error anywhere: only a wrong
/// answer, on inputs a syntax test never looks at.
#[test]
fn any_char_rewrite_preserves_match_behaviour() {
    // Matches every code point, newlines included, with and without `i`.
    for pattern in ["[\\s\\S]", "[^]", "[\\d\\D]", "[\\w\\W]", "[\\S\\s]"] {
        for flags in ["", "i", "u", "iu", "m"] {
            let re = js_regexp_new(make_string(pattern), make_string(flags));
            for subject in ["a", "\n", " ", "\u{1F600}", "Ω", "\r"] {
                assert!(
                    js_regexp_test(re, make_string(subject)) != 0,
                    "/{pattern}/{flags} must match {subject:?}"
                );
            }
        }
    }

    // The negated forms are the exact opposite and must still match NOTHING.
    for pattern in ["[^\\s\\S]", "[^\\w\\W]", "[]"] {
        let re = js_regexp_new(make_string(pattern), make_string("i"));
        for subject in ["a", "\n", "Ω"] {
            assert!(
                js_regexp_test(re, make_string(subject)) == 0,
                "/{pattern}/i must not match {subject:?}"
            );
        }
    }

    // A class that is NOT a complementary pair keeps its narrow meaning.
    let narrow = js_regexp_new(make_string("[\\d\\s]"), make_string("i"));
    assert!(js_regexp_test(narrow, make_string("7")) != 0);
    assert!(js_regexp_test(narrow, make_string("a")) == 0);

    // The rewrite emits a NON-capturing group, so group numbering is
    // unchanged: `$1` is still `b`, not the any-char.
    let re = js_regexp_new(make_string("a[\\s\\S](b)"), make_string(""));
    let m = js_regexp_exec(re, make_string("a\nb"));
    assert!(!m.is_null(), "a[\\s\\S](b) must match \"a\\nb\"");

    // Quantifiers still bind to the any-char, lazily and greedily.
    let lazy = js_regexp_new(make_string("<x>([\\s\\S]*?)</x>"), make_string("i"));
    assert!(js_regexp_test(lazy, make_string("<x>one\ntwo</x>")) != 0);
    let greedy = js_regexp_new(make_string("^[\\s\\S]{3}$"), make_string(""));
    assert!(js_regexp_test(greedy, make_string("a\nb")) != 0);
    assert!(js_regexp_test(greedy, make_string("a\nbc")) == 0);

    // `.source` still reports what the author wrote, not the translation.
    let re = js_regexp_new(make_string("[\\s\\S]+"), make_string("gi"));
    assert_eq!(
        string_payload(js_regexp_get_source(re)),
        b"[\\s\\S]+".to_vec()
    );
}

/// #9305 fallout: the translator spells ECMAScript's ASCII `\b`/`\B` as
/// `(?-iu:\b)`, which fancy-regex's parser rejects (`NonUnicodeUnsupported`).
/// Any lookaround/backreference pattern containing a word boundary therefore
/// raised a bogus SyntaxError — cli.js's `marked` html-block regex among
/// them, whose throw-in-a-microtask the setjmp miscompile then turned into
/// a segfault. `build_fancy_regex` now rewrites the marker into one-char
/// lookarounds.
#[test]
fn fancy_engine_accepts_ascii_word_boundary_markers() {
    // Lookahead + \b: std engine refuses (lookaround), fancy must accept.
    let translated = js_regex_to_rust(r"(?!foo\b)\w+");
    let fancy = crate::regex::build_fancy_regex(&translated).expect("fancy build");
    assert_eq!(
        fancy.find("foobar").unwrap().map(|m| m.as_str()),
        Some("foobar")
    );
    assert!(fancy.find("foo bar").unwrap().map(|m| m.as_str()) != Some("foo"));

    // \B variant.
    let translated = js_regex_to_rust(r"(?=x)x\Ba");
    let fancy = crate::regex::build_fancy_regex(&translated).expect("fancy \\B build");
    assert!(fancy.is_match("xa").unwrap());

    // Boundary semantics stay ASCII on the fancy engine: é is NOT a word
    // char, so /(?=.)\bé/ must treat the position before é as a boundary
    // only when the preceding char is a word char... spec: \b before é
    // (non-word) requires previous to be word.
    let translated = js_regex_to_rust(r"(?=.)a\b\u00e9");
    let fancy = crate::regex::build_fancy_regex(&translated).expect("fancy ascii build");
    assert!(fancy.is_match("a\u{e9}").unwrap());

    // The real-world shape: marked's html-block regex from cli_2.1.112.js.
    let marked = concat!(
        r"^ *(?:<!--(?:-?>|[\s\S]*?(?:-->|$)) *(?:\n|\s*$)",
        r"|<((?!(?:a|em|strong|small|s|cite|q|dfn|abbr|data|time|code|var|samp|kbd",
        r"|sub|sup|i|b|u|mark|ruby|rt|rp|bdi|bdo|span|br|wbr|ins|del|img)\b)",
        r"\w+(?!:|[^\w\s@]*@)\b)[\s\S]+?</\1> *(?:\n{2,}|\s*$)",
        r"|<(?!(?:a|em|strong|small|s|cite|q|dfn|abbr|data|time|code|var|samp|kbd",
        r"|sub|sup|i|b|u|mark|ruby|rt|rp|bdi|bdo|span|br|wbr|ins|del|img)\b)",
        r"\w+(?!:|[^\w\s@]*@)\b(?:\x22[^\x22]*\x22|'[^']*'|\s[^'\x22/>\s]*)*?/?> *(?:\n{2,}|\s*$))",
    );
    let translated = js_regex_to_rust(marked);
    let fancy = crate::regex::build_fancy_regex(&translated).expect("marked html regex build");
    assert_eq!(
        fancy
            .find("<div>\nhello\n</div>\n\n")
            .unwrap()
            .map(|m| m.as_str()),
        Some("<div>\nhello\n</div>\n\n")
    );
}

// ---- #9429: exec/test at a non-zero lastIndex see the WHOLE subject ------

/// One `exec` at `last_index`, as `(matched text, .index, lastIndex after)`.
/// `None` also asserts the spec's reset-to-0 on a failed stateful exec, so a
/// row that stops matching cannot quietly leave `lastIndex` behind.
fn exec_from(
    pattern: &str,
    flags: &str,
    subject: &str,
    last_index: usize,
) -> Option<(String, f64, usize)> {
    let re = js_regexp_new(make_string(pattern), make_string(flags));
    store_last_index_number(re, last_index);
    let arr = js_regexp_exec(re, make_string(subject));
    if arr.is_null() {
        assert_eq!(
            regex_last_index_offset(re),
            0,
            "{pattern}/{flags} @{last_index}: a failed stateful exec resets lastIndex"
        );
        return None;
    }
    let text = match_capture_text(arr, 0).expect("capture zero always participates");
    Some((
        text,
        js_regexp_exec_get_index(),
        regex_last_index_offset(re),
    ))
}

fn hit(text: &str, index: f64, last_index: usize) -> Option<(String, f64, usize)> {
    Some((text.to_string(), index, last_index))
}

#[test]
fn exec_at_last_index_holds_anchors_against_the_subject_not_a_slice() {
    // Every row is a position where the SLICE and the SUBJECT disagree.
    // `^` is start-of-subject: at lastIndex 1 of "ab" it must not hold, even
    // though it would hold at offset 0 of the slice "b".
    assert_eq!(exec_from("^b", "g", "ab", 1), None);
    assert_eq!(exec_from("^b", "g", "ab", 0), None);
    assert_eq!(exec_from("^a", "g", "ab", 0), hit("a", 0.0, 1));
    assert_eq!(exec_from("^a", "g", "ab", 1), None);
    // Under `m` it holds after a LineTerminator IN THE SUBJECT — index 2 of
    // "a\nb" regardless of where the scan was told to start.
    assert_eq!(exec_from("^b", "gm", "a\nb", 0), hit("b", 2.0, 3));
    assert_eq!(exec_from("^b", "gm", "a\nb", 1), hit("b", 2.0, 3));
    assert_eq!(exec_from("^b", "gm", "a\nb", 2), hit("b", 2.0, 3));
    // `\b`/`\B` read the character BEFORE the start position.
    assert_eq!(exec_from(r"\bb", "g", "ab", 1), None);
    assert_eq!(exec_from(r"\Bb", "g", "ab", 1), hit("b", 1.0, 2));
    assert_eq!(exec_from(r"\bb", "g", "a b", 1), hit("b", 2.0, 3));
    assert_eq!(exec_from(r"\Bb", "g", "a b", 1), None);
    // `$` at the very end still matches the empty string there.
    assert_eq!(exec_from("$", "g", "ab", 2), hit("", 2.0, 2));
}

#[test]
fn exec_at_last_index_keeps_lookaround_context() {
    // The `regex` crate has no lookaround, so these run on the fancy-regex
    // fallback — assert the lane, or the rows below could pass on a different
    // engine than the one this fix touches.
    let looky = js_regexp_new(make_string("(?<=a)b"), make_string("g"));
    assert!(
        lookup_fancy_regex(looky).is_some(),
        "lookbehind must select the fancy-regex lane"
    );

    // Lookbehind is destroyed by a slice: the `a` is to the LEFT of the start.
    assert_eq!(exec_from("(?<=a)b", "g", "ab", 0), hit("b", 1.0, 2));
    assert_eq!(exec_from("(?<=a)b", "g", "ab", 1), hit("b", 1.0, 2));
    assert_eq!(exec_from("(?<=a)b", "g", "ab", 2), None);
    assert_eq!(exec_from("(?<=ab)c", "g", "abc", 2), hit("c", 2.0, 3));
    // …and a NEGATIVE lookbehind is wrong the other way: a slice makes it hold.
    assert_eq!(exec_from("(?<!a)b", "g", "ab", 1), None);
    assert_eq!(exec_from("(?<!a)b", "g", "xb", 1), hit("b", 1.0, 2));
    // A zero-width lookbehind at the end of the subject still matches.
    assert_eq!(exec_from("(?<=b)", "g", "ab", 2), hit("", 2.0, 2));
    // Lookahead scans rightwards from the found position, unaffected by the
    // start but covered so a future rewrite can't drop it.
    assert_eq!(exec_from("a(?=b)", "g", "abab", 1), hit("a", 2.0, 3));
    assert_eq!(exec_from("a(?=b)", "g", "abab", 3), None);
}

#[test]
fn sticky_exec_anchors_at_last_index_not_at_offset_zero() {
    // Sticky means "the match must START at lastIndex" — of the subject.
    assert_eq!(exec_from("b", "y", "ab", 1), hit("b", 1.0, 2));
    assert_eq!(exec_from("b", "y", "ab", 0), None);
    assert_eq!(exec_from("^b", "y", "ab", 1), None);
    assert_eq!(exec_from(r"\bb", "y", "ab", 1), None);
    assert_eq!(exec_from(r"\bb", "y", "a b", 2), hit("b", 2.0, 3));
    assert_eq!(exec_from("(?<=a)b", "y", "ab", 1), hit("b", 1.0, 2));
    assert_eq!(exec_from("(?<=ab)c", "y", "abc", 2), hit("c", 2.0, 3));
}

#[test]
fn exec_from_last_index_on_the_regress_lane() {
    // A quantified capture group routes to `regress` — the third engine, and
    // the only one whose positional entry point is an iterator.
    let re = js_regexp_new(make_string("(?<=a)(b)*"), make_string("g"));
    assert!(
        lookup_repeat_matcher(re).is_some(),
        "a quantified capture must select the regress lane"
    );
    assert_eq!(exec_from("(?<=a)(b)*", "g", "ab", 1), hit("b", 1.0, 2));
    assert_eq!(exec_from("(?<=a)(b)*", "g", "xb", 1), None);
    assert_eq!(exec_from("(a)*", "g", "xa", 1), hit("a", 1.0, 2));
}

#[test]
fn exec_past_the_end_is_no_match_not_a_search_clamped_to_the_end() {
    // RegExpBuiltinExec step 12.a. `utf16_index_to_byte` saturates at the
    // payload length, so a byte-offset bound cannot see this at all: without
    // the UTF-16 bound, `/a*/g` with lastIndex 5 reports an empty match at 2.
    assert_eq!(exec_from("a*", "g", "ab", 5), None);
    assert_eq!(exec_from("a*", "y", "ab", 5), None);
    assert_eq!(exec_from("a*", "g", "ab", 3), None);
    // Exactly at the end is still in range.
    assert_eq!(exec_from("a*", "g", "ab", 2), hit("", 2.0, 2));
    // Astral: "𝌆" is ONE scalar but TWO code units, so lastIndex 2 is the end
    // and 3 is past it — a scalar-count bound would get both wrong.
    assert_eq!(exec_from("x*", "g", "𝌆", 2), hit("", 2.0, 2));
    assert_eq!(exec_from("x*", "g", "𝌆", 3), None);
}

#[test]
fn stateful_test_reports_the_same_answer_as_exec() {
    // `test` routes global/sticky through `exec`; these are the rows where a
    // sliced haystack flipped the boolean.
    let sticky_anchor = js_regexp_new(make_string("^b"), make_string("y"));
    store_last_index_number(sticky_anchor, 1);
    assert_eq!(js_regexp_test(sticky_anchor, make_string("ab")), 0);

    let global_anchor = js_regexp_new(make_string("^b"), make_string("g"));
    store_last_index_number(global_anchor, 1);
    assert_eq!(js_regexp_test(global_anchor, make_string("ab")), 0);

    let behind = js_regexp_new(make_string("(?<=a)b"), make_string("g"));
    store_last_index_number(behind, 1);
    assert_eq!(js_regexp_test(behind, make_string("ab")), 1);
    assert_eq!(regex_last_index_offset(behind), 2);

    let past_end = js_regexp_new(make_string("a*"), make_string("g"));
    store_last_index_number(past_end, 5);
    assert_eq!(js_regexp_test(past_end, make_string("ab")), 0);

    // A non-global, non-sticky regex ignores lastIndex entirely.
    let plain = js_regexp_new(make_string("^b"), make_string(""));
    store_last_index_number(plain, 1);
    assert_eq!(js_regexp_test(plain, make_string("ab")), 0);
    assert_eq!(regex_last_index_offset(plain), 1, "plain test leaves it be");
}
