use super::*;

fn make_string(s: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

fn make_wtf8(bytes: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn string_payload(s: *const StringHeader) -> Vec<u8> {
    unsafe {
        std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize).to_vec()
    }
}

fn regex_is_built(re: *const RegExpHeader) -> bool {
    !unsafe { (*re).programs_ptr.is_null() }
}

fn regex_has_fancy_program(re: *const RegExpHeader) -> bool {
    regex_is_built(re) && unsafe { (*(*re).programs_ptr).fancy.is_some() }
}

fn regex_has_repeat_program(re: *const RegExpHeader) -> bool {
    regex_is_built(re) && unsafe { (*(*re).programs_ptr).repeat.is_some() }
}

/// Construction must NOT build the automaton; the first operation that needs a
/// matcher must.
///
/// This is the structural half of the perf fix — the wall-clock half is a
/// fixture whose 200 literals cost 73 ms to construct before and ~0 after. A
/// regression here (something re-introducing an eager build) would not fail any
/// behavioural test, only make every program slower, so assert the state
/// directly: `programs_ptr` is the built/not-built flag.
#[test]
fn construction_defers_the_program_build_until_first_use() {
    let re = js_regexp_new(
        make_string("[A-Za-z]+(?:foo|bar)[0-9]{1,4}"),
        make_string("i"),
    );
    assert!(
        !regex_is_built(re),
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
        !regex_is_built(re),
        "reading .source/.flags must not build the program either"
    );

    assert!(js_regexp_test(re, make_string("XFOO12")) != 0);
    assert!(
        regex_is_built(re),
        "the first match must build and install the program"
    );
}

/// Removing the address-keyed source table must not turn the RegExp-pattern
/// constructor arm into an empty-pattern fallback. This calls the exported
/// constructor entry point, so deleting its direct header read fails both the
/// source and inherited-flags assertions.
#[test]
fn regexp_construct_reads_source_and_flags_from_the_pattern_header() {
    let original = js_regexp_new(make_string("left/right"), make_string("ig"));
    let pattern = crate::value::js_nanbox_pointer(original as i64);
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

    let copy = js_regexp_construct(pattern, undefined);
    assert_ne!(
        copy, original,
        "construction must still allocate a fresh object"
    );
    assert_eq!(string_payload(js_regexp_get_source(copy)), b"left\\/right");
    assert_eq!(string_payload(js_regexp_get_flags(copy)), b"gi");

    let override_flags = crate::value::js_nanbox_string(make_string("m") as i64);
    let overridden = js_regexp_construct(pattern, override_flags);
    assert_eq!(
        string_payload(js_regexp_get_source(overridden)),
        b"left\\/right"
    );
    assert_eq!(string_payload(js_regexp_get_flags(overridden)), b"m");
}

/// `RegExp.prototype.compile` rewrites the header in place. The source table
/// used to mask a stale header slot here; after its removal both observable
/// strings must come from the newly stored, traced edges.
#[test]
fn regexp_compile_replaces_the_header_source_and_flags() {
    let receiver = js_regexp_new(make_string("old"), make_string("m"));
    js_regexp_set_last_index(receiver, 9.0);
    let pattern = crate::value::js_nanbox_string(make_string("new/source") as i64);
    let flags = crate::value::js_nanbox_string(make_string("ig") as i64);
    let result = js_regexp_compile_value(receiver, pattern, flags);
    let receiver = crate::value::JSValue::from_bits(result.to_bits()).as_pointer::<RegExpHeader>();

    assert_eq!(
        string_payload(js_regexp_get_source(receiver)),
        b"new\\/source"
    );
    assert_eq!(string_payload(js_regexp_get_flags(receiver)), b"gi");
    assert_eq!(js_regexp_get_last_index(receiver), 0.0);
    assert_eq!(js_regexp_test(receiver, make_string("NEW/source")), 1);
}

/// Perry stores lone JavaScript surrogates as WTF-8. `.source` must copy those
/// exact bytes from the traced pattern slot; routing them through Rust's UTF-8
/// scalar iterator either replaces the surrogate or invokes undefined
/// behaviour.
#[test]
fn regexp_source_round_trips_wtf8_lone_surrogates_from_the_header() {
    let lone_high = [b'a', 0xED, 0xA0, 0x80, b'/', b'b'];
    let re = js_regexp_new(make_string("placeholder"), make_string(""));
    let pattern = make_wtf8(&lone_high);
    unsafe {
        (*re).pattern_ptr = pattern;
    }
    let expected = [b'a', 0xED, 0xA0, 0x80, b'\\', b'/', b'b'];
    assert_eq!(string_payload(js_regexp_get_source(re)), expected);
}

/// The deferred build installs the fancy-regex and RepeatMatcher programs too,
/// not just the linear one — they live on the same publish point, so a header
/// whose pattern needs one must still get it on first use.
#[test]
fn deferred_build_installs_the_fancy_and_repeat_matcher_fallbacks() {
    let fancy = js_regexp_new(make_string(r"(?<=pre)\d+"), make_string(""));
    assert!(!regex_is_built(fancy));
    assert!(js_regexp_test(fancy, make_string("pre77")) != 0);
    assert!(
        regex_has_fancy_program(fancy),
        "first use must install the fancy-regex fallback"
    );
    assert!(js_regexp_test(fancy, make_string("nope77")) == 0);

    let repeat = js_regexp_new(make_string(r"(a?b??)*"), make_string(""));
    assert!(!regex_is_built(repeat));
    assert!(js_regexp_test(repeat, make_string("ab")) != 0);
    assert!(
        regex_has_repeat_program(repeat),
        "first use must install the ECMAScript RepeatMatcher"
    );
}

/// Sabotage for the bounded Segmenter lane: this pattern's standard program
/// is the never-match placeholder, so a wrong `Standard` tag returns false.
/// Exercise all three installation routes that must publish the tag beside the
/// program handle: lazy build, born-built cache hit, and `compile`.
#[test]
fn bounded_test_matcher_tag_routes_fancy_patterns_to_fancy_regex() {
    let _lock = crate::gc::global_side_table_test_lock();
    site_cache::test_reset();
    let pattern = r"(?<=left)right";

    let cold = js_regexp_new(make_string(pattern), make_string(""));
    assert_eq!(unsafe { (*cold).matcher_kind }, MatcherKind::Unbuilt);
    assert_eq!(regexp_test_str_bounded(cold, "leftright"), Some(true));
    assert_eq!(unsafe { (*cold).matcher_kind }, MatcherKind::Fancy);

    let born_built = js_regexp_new(make_string(pattern), make_string(""));
    assert!(regex_is_built(born_built));
    assert_eq!(unsafe { (*born_built).matcher_kind }, MatcherKind::Fancy);
    assert_eq!(
        regexp_test_str_bounded(born_built, "leftwrong"),
        Some(false)
    );

    let compiled = js_regexp_new(make_string("plain"), make_string(""));
    let pattern_value = crate::value::js_nanbox_string(make_string(pattern) as i64);
    let flags_value = crate::value::js_nanbox_string(make_string("") as i64);
    let result = js_regexp_compile_value(compiled, pattern_value, flags_value);
    let compiled = crate::value::JSValue::from_bits(result.to_bits()).as_pointer::<RegExpHeader>();
    assert_eq!(unsafe { (*compiled).matcher_kind }, MatcherKind::Fancy);
    assert_eq!(regexp_test_str_bounded(compiled, "leftright"), Some(true));
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
