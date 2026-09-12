use super::*;
use crate::string::js_string_from_bytes;

pub(super) fn make_string(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

fn make_wtf8(bytes: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
}

pub(super) fn string_payload(s: *const StringHeader) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize).to_vec()
    }
}

pub(super) fn regex_is_built(re: *const RegExpHeader) -> bool {
    !unsafe { (*re).programs_ptr.is_null() }
}

pub(super) fn regex_has_fancy_program(re: *const RegExpHeader) -> bool {
    regex_is_built(re) && unsafe { (*(*re).programs_ptr).fancy.is_some() }
}

pub(super) fn regex_has_repeat_program(re: *const RegExpHeader) -> bool {
    regex_is_built(re) && unsafe { (*(*re).programs_ptr).repeat.is_some() }
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
#[cfg(target_pointer_width = "64")]
fn regexp_header_is_one_56_byte_per_object_record() {
    assert_eq!(
        std::mem::size_of::<RegExpHeader>(),
        56,
        "the three per-program matcher pointers must stay collapsed into one handle"
    );
}

#[test]
fn malloc_finalize_clears_regexp_address_owned_state() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string("finalize"));
    let flags = scope.root_string_ptr(make_string("g"));
    let re = pattern.with_mut_ptr::<StringHeader, _>(|pattern| {
        flags.with_mut_ptr::<StringHeader, _>(|flags| js_regexp_new(pattern, flags))
    });
    let addr = re as usize;
    assert!(test_regex_pointer_entry_exists(addr));
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
    assert!(!crate::object::exotic_expando::test_exotic_expando_entry_exists(addr));
}

// Program lifetime and compilation-churn reclamation are exercised with the
// real collector in gc::tests::runtime_roots::perex_lifecycle.

#[test]
fn js_replacement_expands_special_patterns() {
    for (pattern, subject, replacement, expected) in [
        (r"(\w+)\s(\w+)", "John Smith", "$2 $1", "Smith John"),
        (r"(\w+)\s(\w+)", "John Smith", "[$&]", "[John Smith]"),
        ("b", "abc", "$`", "aac"),
        ("b", "abc", "$'", "acc"),
        ("b", "abc", "$&", "abc"),
        ("b", "abc", "$$", "a$c"),
        ("b", "abc", "$z", "a$zc"),
        ("b", "abc", "end$", "aend$c"),
        ("(a)(x)?(b)", "ab", "$1$2$3", "ab"),
        ("(a)(x)?(b)", "ab", "$10", "a0"),
    ] {
        assert_eq!(replace_case(pattern, subject, replacement), expected);
    }
}

fn replace_case(pattern: &str, subject: &str, replacement: &str) -> String {
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string(pattern));
    let flags = scope.root_string_ptr(make_string(""));
    let re = scope.root_raw_mut_ptr(js_regexp_new(
        pattern.get_raw_const_ptr(),
        flags.get_raw_const_ptr(),
    ));
    let subject = scope.root_string_ptr(make_string(subject));
    let replacement = scope.root_string_ptr(make_string(replacement));
    let out = js_string_replace_regex_named(
        subject.get_raw_const_ptr(),
        re.get_raw_const_ptr(),
        replacement.get_raw_const_ptr(),
    );
    string_as_str(out).to_owned()
}

pub(super) fn first_match(pattern: &str, flags: &str, subject: &str) -> Option<String> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string(pattern));
    let flags = scope.root_string_ptr(make_string(flags));
    let re = scope.root_raw_mut_ptr(js_regexp_new(
        pattern.get_raw_const_ptr(),
        flags.get_raw_const_ptr(),
    ));
    let subject = scope.root_string_ptr(make_string(subject));
    let result = js_regexp_exec(re.get_raw_mut_ptr(), subject.get_raw_const_ptr());
    if result.is_null() {
        None
    } else {
        match_capture_text(result, 0)
    }
}

#[test]
fn js_replacement_named_group_gate() {
    assert_eq!(replace_case("n", "end", "$<bad>"), "e$<bad>d");
    assert_eq!(replace_case("n", "end", "[$<bad>]"), "e[$<bad>]d");
    let pattern = r"(?<first>\w+)\s(?<last>\w+)";
    assert_eq!(
        replace_case(pattern, "John Smith", "$<last>, $<first>"),
        "Smith, John"
    );
    assert_eq!(replace_case(pattern, "John Smith", "[$<missing>]"), "[]");
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
    // RegExp.prototype[@@split] never visits q == size, so a zero-width match
    // at the end does not open a trailing empty chunk.
    let re = js_regexp_new(make_string(r"(?<=\d)"), make_string(""));
    let arr = js_string_split_regex(make_string("a1b2c3"), re);
    unsafe {
        assert_eq!((*arr).length, 3);
    }
    assert_eq!(
        (0..3)
            .map(|index| match_capture_text(arr, index))
            .collect::<Vec<_>>(),
        vec![
            Some("a1".to_string()),
            Some("b2".to_string()),
            Some("c3".to_string()),
        ]
    );

    // Separator captures are interleaved into the result.
    let re = js_regexp_new(make_string(r"((?<=a)X)"), make_string(""));
    let arr = js_string_split_regex(make_string("aXbXc"), re);
    unsafe {
        assert_eq!((*arr).length, 3);
    }
    assert_eq!(
        (0..3)
            .map(|index| match_capture_text(arr, index))
            .collect::<Vec<_>>(),
        vec![
            Some("a".to_string()),
            Some("X".to_string()),
            Some("bXc".to_string()),
        ]
    );
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

pub(super) fn match_capture_text(arr: *const ArrayHeader, index: u32) -> Option<String> {
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
    for pattern in [r"[a\- ]", r"[:\- ]", r"[\-]"] {
        assert_eq!(first_match(pattern, "", "-"), Some("-".into()));
    }
    assert_eq!(first_match(r"a\-b", "", "a-b"), Some("a-b".into()));
    for pattern in [r"[a\- ]", r"[:\- ]", r" {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n"] {
        let _ = js_regexp_new(make_string(pattern), make_string(""));
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
    for (pattern, subject, matched) in [
        (r"\1", "\x01", "\x01"),
        (r"\b(\w+) \2\b", "name \x02x", "name \x02"),
        (r"[\12-\14]", "\n", "\n"),
        (r"[\12-\14]", "\x0c", "\x0c"),
        (r"(a)[\1]", "a\x01", "a\x01"),
        (r"(a)\1", "aa", "aa"),
        (r"\8", "8", "8"),
        (r"\0", "\0", "\0"),
        (r"\012", "\n", "\n"),
    ] {
        assert_eq!(
            first_match(pattern, "", subject),
            Some(matched.into()),
            "{pattern}"
        );
    }
    for pattern in [r"\1", r"\b(\w+) \2\b", r"[\d][\12-\14]{1,}[^\d]"] {
        let _ = js_regexp_new(make_string(pattern), make_string(""));
    }
}

#[test]
fn annexb_invalid_control_escape_is_literal_backslash_c() {
    for (pattern, subject) in [
        (r"\cА", r"\cА"),
        (r"\c ", r"\c "),
        (r"\c", r"\c"),
        (r"\c!", r"\c!"),
        (r"\cA", "\x01"),
    ] {
        assert_eq!(first_match(pattern, "", subject), Some(subject.into()));
    }
    for subject in ["\\", "c", " "] {
        assert_eq!(first_match(r"[\c ]", "", subject), Some(subject.into()));
    }
}

#[test]
fn surrogate_pairs_match_the_original_utf16_units() {
    for (pattern, subject) in [
        (r"\uD800[\uDC00-\uDC0B]", "\u{10000}"),
        (r"\uD800[\uDC00-\uDC0B]", "\u{1000b}"),
        (r"\uD83D\uDE00", "😀"),
        (r"[\uD80C\uD81C-\uD820][\uDC00-\uDFFF]", "\u{13000}"),
        (r"[\uD80C\uD81C-\uD820][\uDC00-\uDFFF]", "\u{183ff}"),
        (r"[ˁ\xAA]", "ˁ"),
        (r"[ˁ\xAA]", "ª"),
        (r"[A-Za-z]", "Z"),
    ] {
        assert_eq!(first_match(pattern, "", subject), Some(subject.into()));
    }
    assert_eq!(first_match(r"\uD800[\uDC00-\uDC0B]", "", "\u{1000c}"), None);
    let scope = crate::gc::RuntimeHandleScope::new();
    let re = scope.root_raw_mut_ptr(js_regexp_new(make_string(r"\uD800x"), make_string("")));
    let subject = scope.root_string_ptr(make_wtf8(b"\xed\xa0\x80x"));
    assert_eq!(
        js_regexp_test(re.get_raw_const_ptr(), subject.get_raw_const_ptr()),
        1
    );
    let pat = r"(?:[A-Za-z\xAA]|\uD800[\uDC00-\uDC0B\uDC0D-\uDC26]|\uD801[\uDC00-\uDC9D])";
    assert!(!js_regexp_new(make_string(pat), make_string("")).is_null());
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

#[test]
fn high_surrogate_distributes_over_group() {
    for pattern in [
        r"\uD83C(?:\uDDE6\uD83C[\uDDE8-\uDDEC]|\uDDE7🇴)",
        r"\uD83E(?:[\uDD0C\uDD0F]️?|[\uDD18-\uDD1F])",
        r"\uD83D(?:\uDC8B‍\uD83D)?[\uDC68\uDC69]",
    ] {
        assert!(!js_regexp_new(make_string(pattern), make_string("")).is_null());
    }
    let pattern = r"\uD83D(?:\uDC8B‍\uD83D)?[\uDC68\uDC69]";
    for subject in ["\u{1F468}", "\u{1F469}", "\u{1F48B}\u{200D}\u{1F469}"] {
        assert_eq!(first_match(pattern, "", subject), Some(subject.into()));
    }
    assert_eq!(first_match(pattern, "", "AB"), None);
}

/// Unicode 17.0 scripts (`Beria_Erfe`, `Sidetic`, `Tai_Yo`, `Tolong_Siki`) are
/// absent from `regex-syntax`'s bundled Unicode-16 UCD. Instead of throwing a
/// `SyntaxError` or compiling to a never-matching class, Perry expands them to
/// the explicit code-point ranges Unicode 17 assigns — so `built-ins/RegExp/`
/// `property-escapes` Test262 cases that expect real matches pass. Covers every
/// alias form (`Script`/`sc`/`Script_Extensions`/`scx`) and long + short names.
#[test]
fn unicode17_scripts_expand_to_codepoint_ranges() {
    for (property, member) in [
        ("Script=Beria_Erfe", '\u{16EA0}'),
        ("sc=Berf", '\u{16EA0}'),
        ("scx=Beria_Erfe", '\u{16EA0}'),
        ("Script_Extensions=Berf", '\u{16EA0}'),
        ("sc=Sidetic", '\u{10940}'),
        ("sc=Tai_Yo", '\u{1E6C0}'),
        ("sc=Tolong_Siki", '\u{11DB0}'),
    ] {
        let subject = member.to_string();
        assert_eq!(
            first_match(&format!(r"^\p{{{property}}}$"), "u", &subject),
            Some(subject)
        );
    }

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
