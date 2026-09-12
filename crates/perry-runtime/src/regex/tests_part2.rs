//! Second half of the `regex` test module, split for the 2000-line file cap.
//! A sibling child of `regex`, so `use super::*` resolves exactly as it does
//! in `tests.rs`; the shared fixtures come from there.

use super::tests::{first_match, make_string, match_capture_text, string_payload};
use super::*;

#[test]
fn search_returns_utf16_index() {
    // `"𝌆x".search(/x/)` is 2 (the astral scalar occupies indices 0 and 1),
    // matching `"𝌆x".indexOf("x")`.
    let re = js_regexp_new(make_string("x"), make_string(""));
    assert_eq!(js_string_search_regex(make_string("𝌆x"), re), 2);
}

/// Construction must validate original JavaScript syntax before returning a receiver.
#[test]
fn constructor_accepts_valid_js_and_rejects_malformed_patterns() {
    let corpus: &[(&str, &str, bool)] = &[
        // Ordinary shapes.
        ("abc", "", true),
        ("^v?(\\d+)\\.(\\d+)\\.(\\d+)$", "", true),
        ("[A-Za-z0-9_.+-]+@[\\w-]+\\.[\\w.-]+", "i", true),
        ("(?:https?|ftp)://[^\\s]+", "gi", true),
        ("\\s+", "gm", true),
        ("a.b", "s", true),
        ("(foo|bar|baz){2,4}", "i", true),
        ("x{0,250}", "", true),
        ("\\d{1,256}", "", true),
        // Unicode classes / properties / astral — the case-folding shapes.
        ("[A-Za-zÀ-ɏ]+", "i", true),
        ("[Ѐ-ӿͰ-Ͽ]*", "giu", true),
        ("\\p{L}+", "u", true),
        ("\\p{Script=Greek}", "u", true),
        ("[\\u{1F600}-\\u{1F64F}]", "u", true),
        ("[←-⇿☀-⛿]", "u", true),
        ("\\w+\\b", "iu", true),
        // Lookarounds and backreferences are valid JavaScript syntax.
        ("(?<=pre)\\d+", "", true),
        ("(?<!x)y", "", true),
        ("(?=abc)a", "", true),
        ("(a)\\1", "", true),
        // Malformed.
        ("(", "", false),
        ("[z-a]", "", false),
        ("a{2,1}", "", false),
        ("[", "", false),
        (")", "", false),
        ("\\p{Bogus}", "u", false),
        ("\\p{Script=Nonsense}", "u", false),
        ("[\\p{Bogus}]", "u", false),
        ("(?<", "", false),
        ("*", "", false),
    ];
    for &(pattern, flags, expected) in corpus {
        let result = crate::exception::catch_js_throw(|| {
            js_regexp_new(make_string(pattern), make_string(flags))
        });
        assert_eq!(result.is_ok(), expected, "/{pattern}/{flags}");
    }
}

/// Two evaluations of the same pattern are still distinct objects with
/// independent `lastIndex`, and deferring the build does not let them share a
/// header (ECMA-262 requires a fresh object per evaluation — the same
/// invariant the closure-literal singleton fix restored for functions).
#[test]
fn construction_keeps_per_object_identity_and_last_index() {
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

/// Retain the marked HTML and ASCII word-boundary cases that formerly needed fallback translation.
#[test]
fn lookarounds_and_backreferences_use_javascript_word_boundaries() {
    // Lookahead + \b: std engine refuses (lookaround), fancy must accept.
    let pattern = r"(?!foo\b)\w+";
    assert_eq!(
        first_match(pattern, "", "foobar").as_deref(),
        Some("foobar")
    );
    assert!(first_match(pattern, "", "foo bar").as_deref() != Some("foo"));

    // \B variant.
    assert!(first_match(r"(?=x)x\Ba", "", "xa").is_some());

    // Boundary semantics stay ASCII on the fancy engine: é is NOT a word
    // char, so /(?=.)\bé/ must treat the position before é as a boundary
    // only when the preceding char is a word char... spec: \b before é
    // (non-word) requires previous to be word.
    assert!(first_match(r"(?=.)a\b\u00e9", "", "a\u{e9}").is_some());

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
    assert_eq!(
        first_match(marked, "", "<div>\nhello\n</div>\n\n").as_deref(),
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
        test_original_strings_and_program(looky).2,
        "the matcher must be a GC-owned Perex program"
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
fn exec_from_last_index_with_quantified_captures() {
    // A quantified capture group routes to `regress` — the third engine, and
    // the only one whose positional entry point is an iterator.
    let re = js_regexp_new(make_string("(?<=a)(b)*"), make_string("g"));
    assert!(
        test_original_strings_and_program(re).2,
        "the matcher must be a GC-owned Perex program"
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
    assert_eq!(global_match_list("a*", "g", "aXa"), vec!["a", "", "a", ""]);
    assert_eq!(global_match_list("(?:)", "g", "ab"), vec!["", "", ""]);
    assert_eq!(global_match_list("(?:)", "gu", "a𝌆b"), vec!["", "", "", ""]);
    assert_eq!(
        global_match_list("(?:)", "g", "a𝌆b"),
        vec!["", "", "", "", ""]
    );
}

#[test]
fn global_match_keeps_the_trailing_and_interior_empty_matches() {
    // The linear `regex` lane.
    let plain = js_regexp_new(make_string("a*"), make_string("g"));
    assert!(
        test_original_strings_and_program(plain).2,
        "the matcher must be a GC-owned Perex program"
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
fn global_match_keeps_empty_matches_with_lookarounds() {
    // A possibly-empty pattern the linear engine cannot compile.
    let looky = js_regexp_new(make_string("a*(?!x)"), make_string("g"));
    assert!(
        test_original_strings_and_program(looky).2,
        "the matcher must be a GC-owned Perex program"
    );
    assert_eq!(global_match_list("a*(?!x)", "g", "a"), vec!["a", ""]);
    assert_eq!(
        global_match_list("a*(?!x)", "g", "aXa"),
        vec!["a", "", "a", ""]
    );
    assert_eq!(global_match_list("(?<=,)", "g", "a,b,"), vec!["", ""]);
}

#[test]
fn global_match_keeps_empty_matches_with_quantified_captures() {
    // `regress`'s iterator already implements the ECMAScript rule; this is the
    // control that says so, and that nothing routed it elsewhere.
    let quantified = js_regexp_new(make_string("(a)*"), make_string("g"));
    assert!(
        test_original_strings_and_program(quantified).2,
        "the matcher must be a GC-owned Perex program"
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
    let re = scope.root_raw_mut_ptr(re);
    // Assert the actual single-engine owner before exercising the hard case.
    assert!(
        test_original_strings_and_program(re.get_raw_const_ptr()).2,
        "the matcher must be a GC-owned Perex program"
    );

    let hay = format!("{}!", "a".repeat(40));
    let subject = scope.root_string_ptr(make_string(&hay));
    let started = std::time::Instant::now();
    let outcome = crate::exception::catch_js_throw(|| {
        js_regexp_test(re.get_raw_const_ptr(), subject.get_raw_const_ptr())
    });
    let answer = outcome.unwrap_or_else(|error| {
        let error = scope.root_nanbox_f64(error);
        let message = super::perex_api::finish(super::perex_dispatch::get(&error, b"message"));
        let message = crate::value::js_get_string_pointer_unified(message) as *const StringHeader;
        panic!(
            "expected no match, received JS error: {}",
            string_as_str(message)
        );
    });
    assert_eq!(answer, 0, "no match: the subject ends in '!'");
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
        good.with_const_ptr::<StringHeader, _>(|s| js_regexp_test(re.get_raw_const_ptr(), s)),
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
}
