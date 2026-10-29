//! The lazy automaton and the evaluator are two exact readings of one
//! program: every search the automaton answers must give the evaluator's match.

use super::*;
use crate::regex::tests::make_string;
use crate::regex::{js_regexp_new, RegExpHeader};

const WORK: usize = usize::MAX;

/// A RegExp's program, compiled through the production path.
fn regexp(scope: &RuntimeHandleScope, pattern: &str, flags: &str) -> *mut RegExpHeader {
    let pattern = scope.root_string_ptr(make_string(pattern));
    let flags = scope.root_string_ptr(make_string(flags));
    pattern.with_const_ptr(|pattern| flags.with_const_ptr(|flags| js_regexp_new(pattern, flags)))
}

/// The match and every capture span, from the automaton-first search and from
/// the evaluator alone, for one search of `pattern` over `subject` at `start`.
type Answer = Option<Vec<Option<Span>>>;

fn both(
    pattern: &str,
    flags: &str,
    subject: &str,
    start: usize,
    mode: CaptureMode,
) -> (Answer, Answer) {
    let scope = RuntimeHandleScope::new();
    let re = scope.root_raw_mut_ptr(regexp(&scope, pattern, flags));
    let input = scope.root_string_ptr(make_string(subject));
    let mut budget = Budget::new(WORK);
    let memory = MemoryBudget::new(super::super::perex_api::SCRATCH_BYTES);
    let owner = re
        .with_const_ptr::<RegExpHeader, _>(|re| unsafe { GcProgram::from_regexp(&scope, re) })
        .unwrap();
    let program = super::super::perex_api::bind_program(owner, &mut budget).unwrap();
    let subject = super::super::perex_api::bind_heap_subject(input).unwrap();
    let registers = program.with_view(|p| p.register_count()).unwrap();
    let read = |found: Option<Span>, captures: Option<Captures<'_>>| {
        found.map(|full| match captures {
            Some(captures) => captures.to_vec(),
            None => vec![Some(full)],
        })
    };
    let (found, _) = find_near(
        &program,
        &subject,
        start,
        None,
        mode,
        &mut budget,
        &memory,
        64,
        &mut poll,
    )
    .unwrap();
    let first = match found {
        Some(m) => read(Some(m.full), m.captures),
        None => None,
    };
    let mut captures = None;
    let (found, _) = evaluate(
        &BoundResources {
            program: &program,
            subject: &subject,
        },
        &(),
        registers,
        start,
        None,
        mode,
        &mut budget,
        &memory,
        64,
        &mut captures,
        &mut poll,
    )
    .unwrap();
    (first, read(found, captures))
}

const CASES: &[(&str, &str, &str)] = &[
    ("[\\0 ]", "g", "0000644\0 "),
    ("[\\0 ]", "g", "12345670"),
    ("\\s+", "", "sha512-abc  sha1-def"),
    ("(foo|bar|baz)=(\\d+)", "", "a=1&baz=42&foo=7"),
    ("\\w+", "g", "  foo bar"),
    ("a*?", "", "aaa"),
    ("(?:)", "", "abc"),
    ("x*", "g", "axxb"),
    ("\\bfo", "", "afo fo"),
    ("\\Bo", "", "o foo"),
    ("^b", "m", "a\nb\nb"),
    ("b$", "m", "ab\nb"),
    ("[a-z]+_[0-9]+", "", "--record_12345--"),
    ("%[0-9a-f]{2}", "gi", "a%2Fb%2fc%zz"),
    ("\\.([^.[]+)", "g", "a.b[c].d"),
    ("é+", "u", "café éé"),
    (".", "u", "😀x"),
    (".", "", "😀x"),
    ("\\u{1F600}", "u", "a😀b😀"),
    ("[^a]", "u", "a😀"),
    ("k", "iu", "\u{212A}k"),
    ("(a)|b", "", "cb"),
    ("a{2,3}", "", "aaaaa"),
    ("(?:ab|a)(?:c|bcd)", "", "xabcd"),
    ("q", "", "no match here"),
    ("a", "y", "ba"),
];

/// Every case, at every start, in both modes: the automaton-first search
/// gives the evaluator's match and captures, and the automaton really
/// answered a share of them (an ineligible-only run would prove nothing).
#[test]
fn dfa_first_search_matches_the_evaluator() {
    let _lock = crate::gc::global_side_table_test_lock();
    let before = DFA_ANSWERS.with(std::cell::Cell::get);
    let mut searches = 0;
    for &(pattern, flags, subject) in CASES {
        let units = subject.encode_utf16().count();
        for start in 0..=units {
            for mode in [CaptureMode::Full, CaptureMode::All] {
                let (first, reference) = both(pattern, flags, subject, start, mode);
                assert_eq!(
                    first, reference,
                    "/{pattern}/{flags} on {subject:?} from {start}"
                );
                searches += 1;
            }
        }
    }
    let answered = DFA_ANSWERS.with(std::cell::Cell::get) - before;
    assert!(
        answered * 2 > searches,
        "the automaton answered {answered} of {searches} searches"
    );
}

/// A program the automaton cannot read carries no cache, and one it can
/// carries at least its minimum.
#[test]
fn only_eligible_programs_carry_a_cache() {
    let _lock = crate::gc::global_side_table_test_lock();
    for (pattern, eligible) in [
        ("a+b", true),
        ("(a)\\1", false),
        ("a(?=b)", false),
        ("(?<=a)b", false),
    ] {
        let scope = RuntimeHandleScope::new();
        let re = scope.root_raw_mut_ptr(regexp(&scope, pattern, ""));
        let mut budget = Budget::new(WORK);
        let owner = re
            .with_const_ptr::<RegExpHeader, _>(|re| unsafe { GcProgram::from_regexp(&scope, re) })
            .unwrap();
        let program = super::super::perex_api::bind_program(owner, &mut budget).unwrap();
        let cache = program
            .with_view(|p| {
                let len = unsafe { super::super::perex_owner::cell_dfa_cache(p) }
                    .map(|cache| cache.len());
                (len, dfa::minimum_words(p))
            })
            .unwrap();
        match cache {
            (Some(len), Some(minimum)) => {
                assert!(eligible, "/{pattern}/ carries a cache");
                assert!(
                    len >= minimum,
                    "/{pattern}/: {len} words < minimum {minimum}"
                );
            }
            (None, None) => assert!(!eligible, "/{pattern}/ carries no cache"),
            other => panic!("/{pattern}/: cache and eligibility disagree: {other:?}"),
        }
    }
}

#[test]
fn dfa_decisions_do_not_acquire_evaluator_metadata() {
    let _lock = crate::gc::global_side_table_test_lock();
    let before = EVALUATOR_METADATA_READS.with(std::cell::Cell::get);
    let answered = DFA_ANSWERS.with(std::cell::Cell::get);
    for subject in ["abcdefgh", "abc+def"] {
        let (automaton, evaluator) = both("\\+", "g", subject, 0, CaptureMode::Full);
        assert_eq!(automaton, evaluator);
    }
    assert!(DFA_ANSWERS.with(std::cell::Cell::get) > answered);
    assert_eq!(EVALUATOR_METADATA_READS.with(std::cell::Cell::get), before);
    let (automaton, evaluator) = both("a(?=b)", "", "ab", 0, CaptureMode::Full);
    assert_eq!(automaton, evaluator);
    assert!(EVALUATOR_METADATA_READS.with(std::cell::Cell::get) > before);
}

/// Prefix programs are answered with no automaton storage or evaluator setup.
#[test]
fn literal_prefixes_need_neither_dfa_cache_nor_evaluator_scratch() {
    let _lock = crate::gc::global_side_table_test_lock();
    let prefix_before = LITERAL_PREFIX_ANSWERS.with(std::cell::Cell::get);
    let dfa_before = DFA_ANSWERS.with(std::cell::Cell::get);
    let metadata_before = EVALUATOR_METADATA_READS.with(std::cell::Cell::get);
    for (pattern, flags, subject) in [
        ("^--", "", "--option"),
        ("^--", "g", "x--option"),
        ("^no-", "y", "no-color"),
        ("^é", "u", "éclair"),
        ("^😀", "gu", "😀tail"),
    ] {
        let scope = RuntimeHandleScope::new();
        let re = scope.root_raw_mut_ptr(regexp(&scope, pattern, flags));
        let owner = re
            .with_const_ptr::<RegExpHeader, _>(|re| unsafe { GcProgram::from_regexp(&scope, re) })
            .unwrap();
        let program = super::super::perex_api::bind_program(owner, &mut Budget::new(WORK)).unwrap();
        program
            .with_view(|p| {
                assert!(p.literal_prefix().is_some());
                assert_eq!(dfa::minimum_words(p), Some(0));
                assert!(unsafe { super::super::perex_owner::cell_dfa_cache(p) }.is_none());
            })
            .unwrap();
        for start in 0..=subject.encode_utf16().count() + 1 {
            for mode in [CaptureMode::Full, CaptureMode::All] {
                let (prefix, evaluator) = both(pattern, flags, subject, start, mode);
                assert_eq!(
                    prefix, evaluator,
                    "/{pattern}/{flags} {subject:?} start={start}"
                );
            }
        }
    }
    assert!(LITERAL_PREFIX_ANSWERS.with(std::cell::Cell::get) > prefix_before);
    assert_eq!(DFA_ANSWERS.with(std::cell::Cell::get), dfa_before);
    assert_eq!(
        EVALUATOR_METADATA_READS.with(std::cell::Cell::get),
        metadata_before
    );
}

const CAPTURE_CASES: &[(&str, &str, &str)] = &[
    ("(\\d+)-(\\d+)", "", "a 12-34 b"),
    ("(a)(b)", "y", "ab"),
    ("(b)", "y", "ab"),
    ("^(\\w+) (\\w+)$", "", "foo bar"),
    ("(?:(a)|b)+", "", "abab"),
    ("(a)?b", "", "b"),
    ("^(\\d{4})-(\\d{2})-(\\d{2})$", "", "2024-10-28"),
    ("(\\w+)@(\\w+)\\.com", "", "mail bob@example.com now"),
    ("(a*)(a*)b", "", "aaab"),
    ("(a|ab)(c|bcd)(d*)", "", "xabcd"),
    ("(x)*y", "", "xxxy"),
];

/// Searches whose groups the automaton's capture pass reads agree with the
/// evaluator's, from every start; a one-pass program is answered by the pass
/// and an ambiguous one is declined to the evaluator.
#[test]
fn dfa_capture_pass_matches_the_evaluator() {
    let _lock = crate::gc::global_side_table_test_lock();
    for &(pattern, flags, subject) in CAPTURE_CASES {
        let before = DFA_CAPTURE_ANSWERS.with(std::cell::Cell::get);
        for start in 0..=subject.encode_utf16().count() {
            let (first, reference) = both(pattern, flags, subject, start, CaptureMode::All);
            assert_eq!(
                first, reference,
                "/{pattern}/{flags} on {subject:?} from {start}"
            );
        }
        let answered = DFA_CAPTURE_ANSWERS.with(std::cell::Cell::get) - before;
        let ambiguous = matches!(pattern, "(a*)(a*)b" | "(a|ab)(c|bcd)(d*)");
        if ambiguous {
            assert!(
                answered < subject.encode_utf16().count() + 1,
                "/{pattern}/ is not one-pass"
            );
        } else {
            assert!(answered > 0, "/{pattern}/ is one-pass but never answered");
        }
    }
}

/// One program searched over many subjects: its storage starts at the
/// minimum plus a few states, thrashes, is reallocated once at the size
/// Perex reports, and the pass answers every later search.
#[test]
fn dfa_storage_grows_for_a_program_that_thrashes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let pattern = "^(\\d{4})-(\\d{2})-(\\d{2})T[\\d:]+ \\[(\\w+)\\] user=(\\d+) action=(\\w+) latency=(\\d+)ms$";
    let lines: Vec<String> = (0..120)
        .map(|k| {
            format!(
                "20{:02}-{:02}-{:02}T{:02}:{:02}:{:02} [{}] user={} action={} latency={}ms",
                10 + k % 15,
                1 + k % 12,
                1 + k % 28,
                k % 24,
                k % 60,
                (k * 7) % 60,
                ["info", "warn", "error", "debug"][k % 4],
                k * 31,
                ["login", "logout", "view", "buy_item"][k % 4],
                k * 3
            )
        })
        .collect();
    let scope = RuntimeHandleScope::new();
    let re = scope.root_raw_mut_ptr(regexp(&scope, pattern, ""));
    let mut budget = Budget::new(WORK);
    let memory = MemoryBudget::new(super::super::perex_api::SCRATCH_BYTES);
    let owner = re
        .with_const_ptr::<RegExpHeader, _>(|re| unsafe { GcProgram::from_regexp(&scope, re) })
        .unwrap();
    let program = super::super::perex_api::bind_program(owner, &mut budget).unwrap();
    let words = |program: &BoundProgram<GcProgram<'_>>| {
        program
            .with_view(|p| unsafe { super::super::perex_owner::cell_dfa_cache(p) }.map(|c| c.len()))
            .unwrap()
    };
    let before = DFA_CAPTURE_ANSWERS.with(std::cell::Cell::get);
    let mut first_len = None;
    for (n, line) in lines.iter().enumerate() {
        let (_, reference) = both(pattern, "", line, 0, CaptureMode::All);
        let input = scope.root_string_ptr(make_string(line));
        let subject = super::super::perex_api::bind_heap_subject(input).unwrap();
        let (found, _) = find_near(
            &program,
            &subject,
            0,
            None,
            CaptureMode::All,
            &mut budget,
            &memory,
            64,
            &mut poll,
        )
        .unwrap();
        let got = found.map(|m| m.captures.unwrap().to_vec());
        assert_eq!(got, reference, "line {n}");
        if n == 0 {
            first_len = words(&program);
        }
    }
    let grown = words(&program).unwrap();
    assert!(grown > first_len.unwrap(), "storage never grew");
    let answered = DFA_CAPTURE_ANSWERS.with(std::cell::Cell::get) - before;
    assert!(
        answered >= lines.len() - 12,
        "the pass answered {answered} of {} searches",
        lines.len()
    );
}
