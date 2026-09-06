//! Second half of the `regex` test module, split for the 2000-line file cap.
//! A sibling child of `regex`, so `use super::*` resolves exactly as it does
//! in `tests.rs`; the shared fixtures come from there.

use super::tests::{make_string, match_capture_text, string_payload};
use super::*;

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

// ---- #9430: a global scan keeps the empty match at a match's end ---------

/// `subject.match(/pattern/flags)` for a global regex, as plain strings.
fn global_match_list(pattern: &str, flags: &str, subject: &str) -> Vec<String> {
    let re = js_regexp_new(make_string(pattern), make_string(flags));
    let arr = js_string_match(make_string(subject), re);
    if arr.is_null() {
        return Vec::new();
    }
    let len = unsafe { (*arr).length };
    (0..len)
        .map(|index| match_capture_text(arr, index).expect("a match list holds only strings"))
        .collect()
}

fn replace_all_with(pattern: &str, flags: &str, subject: &str, repl: &str) -> String {
    let re = js_regexp_new(make_string(pattern), make_string(flags));
    let out = js_string_replace_regex(make_string(subject), re, make_string(repl));
    string_as_str(out).to_string()
}

#[test]
fn ecmascript_scan_keeps_an_empty_match_where_the_previous_one_ended() {
    // The scan loop's contract, pinned without an engine: an empty match at
    // the previous match's end is KEPT, and the cursor then advances one
    // position — Rust's iterators drop it and advance instead.
    //
    // The finder below is `/a*/` over "aXa" written out by hand.
    let subject = "aXa";
    let seen = super::global_scan::scan(subject, 0, |cursor| {
        // `a*` matches the empty string anywhere, so its leftmost match from
        // `cursor` always STARTS at `cursor` and runs over the `a`s there.
        let mut end = cursor;
        while subject.as_bytes().get(end) == Some(&b'a') {
            end += 1;
        }
        Some((cursor, end, (cursor, end)))
    });
    assert_eq!(seen, vec![(0, 1), (1, 1), (2, 3), (3, 3)]);

    // The bound is what terminates the walk: without `cursor > len` ending it,
    // the trailing empty match would repeat forever.
    let empties = super::global_scan::scan("ab", 0, |cursor| Some((cursor, cursor, cursor)));
    assert_eq!(empties, vec![0, 1, 2]);

    // A zero-width step never lands inside a scalar.
    assert_eq!(super::global_scan::advance_past_empty("a𝌆b", 0), 1);
    assert_eq!(super::global_scan::advance_past_empty("a𝌆b", 1), 5);
    assert_eq!(super::global_scan::advance_past_empty("a𝌆b", 5), 6);
    assert_eq!(super::global_scan::advance_past_empty("ab", 2), 3);
}

#[test]
fn global_match_keeps_the_trailing_and_interior_empty_matches() {
    // The linear `regex` lane.
    let plain = js_regexp_new(make_string("a*"), make_string("g"));
    assert!(
        lookup_fancy_regex(plain).is_none() && lookup_repeat_matcher(plain).is_none(),
        "`a*` must stay on the linear engine"
    );
    assert_eq!(global_match_list("a*", "g", "a"), vec!["a", ""]);
    assert_eq!(global_match_list("a*", "g", "aa"), vec!["aa", ""]);
    assert_eq!(global_match_list("b*", "g", "ab"), vec!["", "b", ""]);
    // Not only the trailing one: the empty match at index 1 is interior.
    assert_eq!(global_match_list("a*", "g", "aXa"), vec!["a", "", "a", ""]);
    assert_eq!(global_match_list("x*", "g", "abc"), vec!["", "", "", ""]);
    assert_eq!(global_match_list("a*", "g", ""), vec![""]);
    // A pattern that cannot match empty is unchanged.
    assert_eq!(global_match_list("a+", "g", "aXa"), vec!["a", "a"]);
}

#[test]
fn global_match_keeps_empty_matches_on_the_fancy_lane() {
    // A possibly-empty pattern the linear engine cannot compile.
    let looky = js_regexp_new(make_string("a*(?!x)"), make_string("g"));
    assert!(
        lookup_fancy_regex(looky).is_some(),
        "a lookahead must select the fancy-regex lane"
    );
    assert_eq!(global_match_list("a*(?!x)", "g", "a"), vec!["a", ""]);
    assert_eq!(
        global_match_list("a*(?!x)", "g", "aXa"),
        vec!["a", "", "a", ""]
    );
    assert_eq!(global_match_list("(?<=,)", "g", "a,b,"), vec!["", ""]);
}

#[test]
fn global_match_on_the_regress_lane_is_unchanged() {
    // `regress`'s iterator already implements the ECMAScript rule; this is the
    // control that says so, and that nothing routed it elsewhere.
    let quantified = js_regexp_new(make_string("(a)*"), make_string("g"));
    assert!(
        lookup_repeat_matcher(quantified).is_some(),
        "a quantified capture must select the regress lane"
    );
    assert_eq!(global_match_list("(a)*", "g", "a"), vec!["a", ""]);
    assert_eq!(
        global_match_list("(a)*", "g", "aXa"),
        vec!["a", "", "a", ""]
    );
}

#[test]
fn global_replace_substitutes_at_every_empty_match() {
    assert_eq!(replace_all_with("a*", "g", "a", "<>"), "<><>");
    assert_eq!(replace_all_with("a*", "g", "aXa", "-"), "--X--");
    assert_eq!(replace_all_with("b*", "g", "ab", "-"), "-a--");
    assert_eq!(replace_all_with("x*", "g", "abc", "-"), "-a-b-c-");
    assert_eq!(replace_all_with("a*", "g", "aXa", "[$&]"), "[a][]X[a][]");
    // The non-global form still replaces exactly one match.
    assert_eq!(replace_all_with("a*", "", "aXa", "-"), "-Xa");
    // Fancy lane.
    assert_eq!(replace_all_with("a*(?!x)", "g", "a", "<>"), "<><>");
    assert_eq!(replace_all_with("(?<=a)", "g", "aba", "!"), "a!ba!");
    // Named-group substitution takes its own scan path.
    let named = js_regexp_new(make_string("(?<n>a)*"), make_string("g"));
    let out = js_string_replace_regex_named(make_string("a"), named, make_string("[$<n>]"));
    assert_eq!(string_as_str(out), "[a][]");
}

/// The construction cache (`regex::site_cache`): once a header built from
/// some `(pattern, flags)` has been executed, the next construction of the
/// same text is born built — it shares the executed header's program and
/// never runs the lazy build. Fails on a runtime without the cache (the
/// second header stays lazy).
#[test]
fn site_cache_reconstruction_is_born_built() {
    let _lock = crate::gc::global_side_table_test_lock();
    site_cache::test_reset();
    let re1 = js_regexp_new(make_string("born[0-9]+built"), make_string("g"));
    assert!(
        unsafe { (*re1).regex_ptr.is_null() },
        "construction stays lazy"
    );
    assert_eq!(
        site_cache::test_has_programs("born[0-9]+built", "g"),
        Some(false),
        "construction records the validated text without programs"
    );
    assert!(js_regexp_test(re1, make_string("xx born42built")) != 0);
    assert_eq!(
        site_cache::test_has_programs("born[0-9]+built", "g"),
        Some(true),
        "the first execution's build is remembered against the text"
    );
    let re2 = js_regexp_new(make_string("born[0-9]+built"), make_string("g"));
    assert!(
        !unsafe { (*re2).regex_ptr.is_null() },
        "the second construction installs the programs eagerly"
    );
    assert!(
        std::ptr::eq(unsafe { (*re1).regex_ptr }, unsafe { (*re2).regex_ptr }),
        "both headers share one compiled program"
    );
    // The owned source copies are shared too (two refcount bumps per header,
    // not two `String`s).
    let (p1, p2) = REGEX_SOURCE_TABLE.with(|t| {
        let t = t.borrow();
        (
            t.get(&(re1 as usize)).map(|(p, _)| p.clone()).unwrap(),
            t.get(&(re2 as usize)).map(|(p, _)| p.clone()).unwrap(),
        )
    });
    assert!(Arc::ptr_eq(&p1, &p2), "source text is shared, not copied");
    assert_eq!(js_regexp_test(re2, make_string("born7built")), 1);
    assert_eq!(js_regexp_test(re2, make_string("nothing")), 0);
    // Different flags are a different entry.
    let re3 = js_regexp_new(make_string("born[0-9]+built"), make_string("i"));
    assert!(unsafe { (*re3).regex_ptr.is_null() });
}

/// `test` on a global/sticky receiver advances `lastIndex` exactly like
/// `exec` and resets it on failure, through the find-only engine phase (no
/// exec array). Pinned against node for every branch of that bookkeeping.
#[test]
fn global_test_advances_and_resets_last_index() {
    let _lock = crate::gc::global_side_table_test_lock();
    let re = js_regexp_new(make_string("a"), make_string("g"));
    let s = make_string("aXa");
    assert_eq!(js_regexp_test(re, s), 1);
    assert_eq!(js_regexp_get_last_index(re), 1.0);
    assert_eq!(js_regexp_test(re, s), 1);
    assert_eq!(js_regexp_get_last_index(re), 3.0);
    assert_eq!(js_regexp_test(re, s), 0);
    assert_eq!(js_regexp_get_last_index(re), 0.0);

    // `lastIndex > length` is "no match" and resets.
    js_regexp_set_last_index(re, 10.0);
    assert_eq!(js_regexp_test(re, s), 0);
    assert_eq!(js_regexp_get_last_index(re), 0.0);

    // sticky anchors at lastIndex.
    let sticky = js_regexp_new(make_string("a"), make_string("y"));
    let t = make_string("ba");
    assert_eq!(js_regexp_test(sticky, t), 0);
    assert_eq!(js_regexp_get_last_index(sticky), 0.0);
    js_regexp_set_last_index(sticky, 1.0);
    assert_eq!(js_regexp_test(sticky, t), 1);
    assert_eq!(js_regexp_get_last_index(sticky), 2.0);

    // lastIndex counts UTF-16 code units, not bytes.
    let astral = js_regexp_new(make_string("b"), make_string("g"));
    let u = make_string("😀b😀b");
    assert_eq!(js_regexp_test(astral, u), 1);
    assert_eq!(js_regexp_get_last_index(astral), 3.0);
    assert_eq!(js_regexp_test(astral, u), 1);
    assert_eq!(js_regexp_get_last_index(astral), 6.0);
    assert_eq!(js_regexp_test(astral, u), 0);

    // The fancy-regex fallback (lookbehind) takes the same path.
    let fancy = js_regexp_new(make_string("(?<=x)a"), make_string("g"));
    let f = make_string("xa xa a");
    assert_eq!(js_regexp_test(fancy, f), 1);
    assert_eq!(js_regexp_get_last_index(fancy), 2.0);
    assert_eq!(js_regexp_test(fancy, f), 1);
    assert_eq!(js_regexp_get_last_index(fancy), 5.0);
    assert_eq!(js_regexp_test(fancy, f), 0);
    assert_eq!(js_regexp_get_last_index(fancy), 0.0);

    // The backtracking matcher (quantified capture) likewise.
    let repeat = js_regexp_new(make_string("(a?b??)*c"), make_string("g"));
    let r = make_string("abc c");
    assert_eq!(js_regexp_test(repeat, r), 1);
    assert_eq!(js_regexp_get_last_index(repeat), 3.0);
    assert_eq!(js_regexp_test(repeat, r), 1);
    assert_eq!(js_regexp_get_last_index(repeat), 5.0);
    assert_eq!(js_regexp_test(repeat, r), 0);
}

/// A capacity event in one compiled-program cache must not leave a pattern
/// whose real program lives in ANOTHER of them permanently non-matching.
///
/// `compile_and_cache_regex_checked` returns early when `REGEX_CACHE` already
/// holds the pattern, so it never re-runs the fancy build; for a lookbehind
/// pattern that `REGEX_CACHE` entry is the never-match placeholder and the
/// real program is the one in `FANCY_CACHE`. Clear `FANCY_CACHE` on its own —
/// which is exactly what its independent 512-entry overflow used to do — and
/// `get_or_compile_regex` hands back a program that matches nothing while
/// nothing rebuilds the fallback. Since `lookup_fancy_regex` now treats a
/// built header as authoritative and `site_cache::install_programs` memoizes
/// the triple against the pattern text, that is not one bad header: every
/// later construction of the same literal is born with it.
///
/// The fix is that `lazy::build_and_install_programs` REPAIRS the header
/// before publishing it and before memoizing the triple: a standard program
/// that is the never-match placeholder with no fancy program beside it means
/// the fancy program is missing, so it is rebuilt. (Clearing the three caches
/// as a group was built first and dropped: it closes the route into the bad
/// state but cannot repair a header already in it, so this test still failed.)
#[test]
fn a_single_program_cache_clear_cannot_disarm_a_lookbehind_literal() {
    let _lock = crate::gc::global_side_table_test_lock();
    let source = "(?<=foo)bar";
    let scope = crate::gc::RuntimeHandleScope::new();
    site_cache::test_reset();

    let build = || {
        let pattern = scope.root_string_ptr(make_string(source));
        let flags = scope.root_string_ptr(make_string(""));
        pattern.with_mut_ptr::<StringHeader, _>(|pattern| {
            flags.with_mut_ptr::<StringHeader, _>(|flags| js_regexp_new(pattern, flags))
        })
    };
    let subject = scope.root_string_ptr(make_string("foobar"));

    let warm = build();
    assert_eq!(
        subject.with_const_ptr::<StringHeader, _>(|s| js_regexp_test(warm, s)),
        1,
        "a lookbehind pattern must match through the fancy fallback"
    );

    // The state a `FANCY_CACHE` overflow produces: its programs are gone, the
    // never-match placeholder for this pattern survives in `REGEX_CACHE`.
    FANCY_CACHE.with(|fc| fc.borrow_mut().clear());
    assert!(
        REGEX_CACHE.with(|c| c
            .borrow()
            .contains_key(&(std::sync::Arc::from(source), std::sync::Arc::from("")))),
        "the placeholder must survive, or this test exercises nothing"
    );
    // A fresh literal site, so the construction cache cannot answer from the
    // programs the first header built.
    site_cache::test_reset();

    let cold = build();
    unsafe {
        lazy::ensure_regex_compiled(cold);
        assert!(
            !(*cold).fancy_ptr.is_null(),
            "a built header must carry every program its pattern needs — a null \
             fancy_ptr here is memoized by site_cache::install_programs and makes \
             the breakage permanent for this literal"
        );
    }
    assert_eq!(
        subject.with_const_ptr::<StringHeader, _>(|s| js_regexp_test(cold, s)),
        1,
        "the literal must still match after an unrelated cache reached capacity"
    );
}

/// The backtracking cliff: a capture group under a quantifier takes a pattern
/// off the linear engine, and the ECMAScript backtracker has no step budget.
/// `/^(a+)+$/.test("a"*28 + "!")` measured 16.5 s against 4.8 s for node and
/// 0 ms for the identical-language `/^(?:a+)+$/`.
///
/// The linear program proves the answer in O(n) — the two engines accept the
/// same language and disagree only about capture ASSIGNMENT — so the
/// backtracker must not be entered for a subject the linear engine has already
/// ruled out. This test would take minutes without that gate.
#[test]
fn quantified_capture_pattern_does_not_backtrack_on_a_non_matching_subject() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string("^(a+)+$"));
    let flags = scope.root_string_ptr(make_string(""));
    let re = pattern.with_mut_ptr::<StringHeader, _>(|pattern| {
        flags.with_mut_ptr::<StringHeader, _>(|flags| js_regexp_new(pattern, flags))
    });
    // The pattern really is on the backtracker — that is the premise.
    assert!(
        lookup_repeat_matcher(re).is_some(),
        "a capture under a quantifier must route to the ECMAScript matcher"
    );

    let hay = format!("{}!", "a".repeat(40));
    let subject = scope.root_string_ptr(make_string(&hay));
    let started = std::time::Instant::now();
    assert_eq!(
        subject.with_const_ptr::<StringHeader, _>(|s| js_regexp_test(re, s)),
        0,
        "no match: the subject ends in '!'"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "a non-matching subject must not be handed to the backtracker \
         (took {:?} for 40 characters)",
        started.elapsed()
    );

    // A subject that DOES match still goes through the backtracker and still
    // reports the spec's captures.
    let good = scope.root_string_ptr(make_string("aaaa"));
    assert_eq!(
        good.with_const_ptr::<StringHeader, _>(|s| js_regexp_test(re, s)),
        1
    );
}

/// #6759 phase 1 follow-up: a `RegExp` receiver can now answer the
/// descriptor-summary probe. Before the meta edge was wired for
/// `GC_TYPE_REGEXP`, `may_have_descriptor_entry` answered the conservative
/// `true` for every RegExp, so `set_last_index_throwing` built a `String` and
/// SipHashed `(usize, String)` on every global/sticky `test()`/`exec()`.
#[test]
fn a_fresh_regexp_proves_lastindex_absent_without_probing_the_tables() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string("x"));
    let flags = scope.root_string_ptr(make_string("g"));
    let re = pattern.with_mut_ptr::<StringHeader, _>(|pattern| {
        flags.with_mut_ptr::<StringHeader, _>(|flags| js_regexp_new(pattern, flags))
    });
    // Premise: this really is the dedicated RegExp cell, not a shaped object
    // that would have answered through the ordinary `GC_TYPE_OBJECT` path.
    let gc = unsafe { crate::value::addr_class::try_read_gc_header(re as usize) }
        .expect("RegExp must be a GC allocation");
    assert_eq!(gc.obj_type, crate::gc::GC_TYPE_REGEXP);

    assert!(
        !crate::object::test_may_have_descriptor_entry(re as usize, "lastIndex", false),
        "a fresh RegExp has no descriptors, so the meta summary must prove \
         `lastIndex` absent instead of sending the caller to the table"
    );
    assert!(
        crate::object::get_property_attrs(re as usize, "lastIndex").is_none(),
        "and the answer the fast path skips must be the same one"
    );
}

/// The other half, and the one that makes the fast negative safe: an owner
/// that DOES have a descriptor must still be found. Install and probe share
/// one predicate, so a probe widened without its install would answer
/// "proven absent" here and `set_last_index_throwing` would silently stop
/// throwing (test262 prototype/{exec,test}/y-fail-lastindex-no-write).
#[test]
fn a_regexp_with_a_non_writable_lastindex_is_still_found_by_the_probe() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_string_ptr(make_string("x"));
    let flags = scope.root_string_ptr(make_string("g"));
    let re = pattern.with_mut_ptr::<StringHeader, _>(|pattern| {
        flags.with_mut_ptr::<StringHeader, _>(|flags| js_regexp_new(pattern, flags))
    });
    let attrs = crate::object::PropertyAttrs::new(false, true, true);
    crate::object::set_property_attrs(re as usize, "lastIndex".to_string(), attrs);

    assert!(
        crate::object::test_may_have_descriptor_entry(re as usize, "lastIndex", false),
        "the install set the key bit, so the probe must send the caller to the table"
    );
    let found = crate::object::get_property_attrs(re as usize, "lastIndex")
        .expect("the descriptor the test installed must be readable back");
    assert!(!found.writable(), "and it must still read as non-writable");

    // A DIFFERENT key on the same owner stays proven-absent: the summary is
    // per key, not per owner, so widening it must not blunt it.
    assert!(
        !crate::object::test_may_have_descriptor_entry(re as usize, "source", false),
        "an unrelated key on the same RegExp must still take the fast negative"
    );
}

// ---------------------------------------------------------------------------
// Literal-site keyed construction (`js_regexp_new_site` + `regex::site_key`).
//
// The site key is the address of a private global the compiler emits once per
// regex literal. These statics stand in for two such globals: distinct
// addresses, 8-byte aligned, immortal — the same three properties the emitted
// ones have, and the reason the key is a sound identity where a `StringHeader`
// address is not.
// ---------------------------------------------------------------------------

// Distinct initialisers so nothing may merge them: the test uses only their
// ADDRESSES, and identical zero-valued statics are exactly the shape a
// constant-merging pass is allowed to collapse. `assert_ne!` on the two keys
// keeps that from being a silent assumption.
static SITE_SLOT_A: u64 = 0xA;
static SITE_SLOT_B: u64 = 0xB;
static SITE_SLOT_C: u64 = 0xC;

fn site_key_of(slot: &'static u64) -> i64 {
    slot as *const u64 as i64
}

/// **The sabotage the coordinator named: key the table by pattern length.**
///
/// Two literals at two sites, same flags, same pattern LENGTH, different text.
/// A table that verifies a probe by anything weaker than the site key — a
/// length, a prefix, a fingerprint without the exactness check — hands the
/// second site the first site's entry, and `.source` then reports a pattern
/// this literal never contained while `test` matches the wrong language.
///
/// Each site is constructed twice: the first construction records, the second
/// is the one that must come back from the table, which is the case a weak key
/// breaks. Asserting only on a first construction would pass under every
/// sabotage, because a miss always takes the content-keyed path.
#[test]
fn two_literal_sites_with_equal_length_patterns_never_answer_for_each_other() {
    let _lock = crate::gc::global_side_table_test_lock();
    site_key::test_reset();

    let key_a = site_key_of(&SITE_SLOT_A);
    let key_b = site_key_of(&SITE_SLOT_B);
    assert_ne!(key_a, key_b, "two sites must have two addresses");

    for round in 0..2 {
        let a = js_regexp_new_site(make_string("a.c"), make_string("g"), key_a);
        let b = js_regexp_new_site(make_string("x.z"), make_string("g"), key_b);
        assert_eq!(
            string_payload(js_regexp_get_source(a)),
            b"a.c".to_vec(),
            "round {round}: site A must report its own pattern"
        );
        assert_eq!(
            string_payload(js_regexp_get_source(b)),
            b"x.z".to_vec(),
            "round {round}: site B must report its own pattern — a table keyed by anything \
             weaker than the site address hands B the entry A recorded, and both patterns are \
             three bytes long"
        );
        assert!(
            js_regexp_test(a, make_string("abc")) != 0
                && js_regexp_test(a, make_string("xyz")) == 0,
            "round {round}: site A must match its own language"
        );
        assert!(
            js_regexp_test(b, make_string("xyz")) != 0
                && js_regexp_test(b, make_string("abc")) == 0,
            "round {round}: site B must match its own language"
        );
    }

    assert_eq!(
        site_key::test_recorded_pattern(key_a, "g").as_deref(),
        Some("a.c")
    );
    assert_eq!(
        site_key::test_recorded_pattern(key_b, "g").as_deref(),
        Some("x.z")
    );
}

/// A dynamic `new RegExp(str)` must never reach the site table.
///
/// The two-argument entry point is what every non-literal construction uses —
/// `js_regexp_construct`, `RegExp.prototype.compile`, the runtime's own
/// callers — and it has no site to be keyed by. If it recorded under some
/// stand-in key, a later literal whose key collided would inherit a pattern
/// that a *variable* produced, which is the one thing the site key's
/// compile-time-constant argument is supposed to guarantee against.
#[test]
fn a_dynamic_construction_records_nothing_in_the_site_table() {
    let _lock = crate::gc::global_side_table_test_lock();
    site_key::test_reset();

    for _ in 0..4 {
        let re = js_regexp_new(make_string("dyn(amic)"), make_string("g"));
        assert!(js_regexp_test(re, make_string("dynamic")) != 0);
    }
    assert_eq!(
        site_key::test_occupied_slots(),
        0,
        "the two-argument entry point has no site key and must record nothing"
    );

    // The same TEXT through the site entry does record — so the zero above is
    // a property of the entry point, not of a table that never works.
    let key = site_key_of(&SITE_SLOT_C);
    let re = js_regexp_new_site(make_string("dyn(amic)"), make_string("g"), key);
    assert!(js_regexp_test(re, make_string("dynamic")) != 0);
    assert_eq!(
        site_key::test_occupied_slots(),
        1,
        "the site entry point must record — otherwise the assertion above proves nothing"
    );
    assert_eq!(
        site_key::test_recorded_pattern(key, "g").as_deref(),
        Some("dyn(amic)")
    );
}

/// A site hit must be born built: the second construction at a site whose
/// first header has already executed installs the compiled programs eagerly,
/// so `regex_ptr` is non-null before any match runs.
///
/// This is what makes the fast path complete — a hit that skipped the content
/// cache but arrived unbuilt would push the pattern's hash back onto the first
/// `test()` and give the site key nothing.
#[test]
fn a_site_hit_after_the_first_execution_is_born_built() {
    let _lock = crate::gc::global_side_table_test_lock();
    site_key::test_reset();
    let key = site_key_of(&SITE_SLOT_A);

    let first = js_regexp_new_site(make_string("bo+rn"), make_string(""), key);
    assert!(
        unsafe { (*first).regex_ptr }.is_null(),
        "construction must not build the program (that is #5777's deferred build)"
    );
    assert!(js_regexp_test(first, make_string("boorn")) != 0);
    assert!(
        !unsafe { (*first).regex_ptr }.is_null(),
        "the first execution installs the programs"
    );

    // Second construction at the SAME site.
    let second = js_regexp_new_site(make_string("bo+rn"), make_string(""), key);
    assert!(
        !unsafe { (*second).regex_ptr }.is_null(),
        "a site hit must install the programs the site already compiled, so the header is born \
         built and the first match pays no lookup"
    );
    assert_ne!(first, second, "each evaluation is still a distinct object");
    assert!(js_regexp_test(second, make_string("born")) != 0);
}

/// The kill switch has to remove the lane, not just its answers: with
/// `PERRY_REGEX_SITE_KEY=0` the table records nothing, so the OFF arm is the
/// content-keyed path exactly rather than a control still paying the
/// bookkeeping.
///
/// Read once per process through a `OnceLock`, so this asserts the DEFAULT is
/// on rather than flipping the variable mid-run (which would only test the
/// cache of the first read).
#[test]
fn the_site_key_lane_is_on_by_default() {
    assert!(
        crate::gc::env_default_on_from_value(None),
        "the site-key lane defaults ON; `PERRY_REGEX_SITE_KEY=0` is the kill switch"
    );
    assert!(!crate::gc::env_default_on_from_value(Some("0")));
}
