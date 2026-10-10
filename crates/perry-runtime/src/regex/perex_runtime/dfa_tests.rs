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
                let len = unsafe { super::super::perex_owner::cell_dfa_cache(p.words()) }
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
