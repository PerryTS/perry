//! Cross-call rebinding (#10166): a program cell keeps a plain-data witness of
//! its validated words, and a string header remembers that its payload
//! validated, so a search per JavaScript call binds both in constant work.
use super::*;
use crate::regex::perex_api as api;
use crate::regex::perex_owner::{cell_witness, set_cell_witness, GcProgram};
use crate::regex::perex_runtime::EngineError;
use crate::regex::RegExpHeader;
use crate::string::{StringHeader, STRING_FLAG_WTF8_VALIDATED};
use crate::value::js_nanbox_string;
use perex::binding::ImmutableProgram;

fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ))
}

fn regex<'s>(scope: &'s RuntimeHandleScope, pattern: &str) -> RuntimeHandle<'s> {
    let pattern = text(scope, pattern.as_bytes());
    let flags = text(scope, b"");
    scope.root_raw_mut_ptr(pattern.with_const_ptr::<StringHeader, _>(|pattern| {
        flags.with_const_ptr::<StringHeader, _>(|flags| crate::regex::js_regexp_new(pattern, flags))
    }))
}

/// One non-global search: the full match's UTF-16 span, or None.
fn search(
    re: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
) -> Result<Option<(usize, usize)>, EngineError> {
    re.with_mut_ptr::<RegExpHeader, _>(|re| {
        input.with_const_ptr::<StringHeader, _>(|input| {
            api::execute(re, input, false, &mut || Ok(()))
                .map(|found| found.map(|m| (m.full.start(), m.full.end())))
        })
    })
}

/// The witness in the RegExp's current program cell.
fn witness(re: &RuntimeHandle<'_>) -> Option<perex::binding::ProgramWitness> {
    re.with_const_ptr::<RegExpHeader, _>(|re| unsafe { cell_witness((*re).perex_program) })
}

fn set_witness(re: &RuntimeHandle<'_>, value: Option<perex::binding::ProgramWitness>) {
    re.with_const_ptr::<RegExpHeader, _>(|re| unsafe {
        set_cell_witness((*re).perex_program, value)
    });
}

fn recompile(scope: &RuntimeHandleScope, re: &RuntimeHandle<'_>, pattern: &str) {
    let pattern = text(scope, pattern.as_bytes());
    let flags = text(scope, b"");
    let p = pattern.with_const_ptr::<StringHeader, _>(|p| js_nanbox_string(p as i64));
    let f = flags.with_const_ptr::<StringHeader, _>(|f| js_nanbox_string(f as i64));
    re.with_mut_ptr::<RegExpHeader, _>(|re| crate::regex::js_regexp_compile_value(re, p, f));
}

fn validated(s: &RuntimeHandle<'_>) -> bool {
    s.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).flags & STRING_FLAG_WTF8_VALIDATED != 0 })
}

#[test]
fn perex_program_witness_lives_with_its_cell_and_a_recompile_starts_without_one() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"xb xa");

    let re = regex(&scope, "x(a)");
    assert_eq!(
        witness(&re),
        None,
        "a freshly compiled program has no witness"
    );
    assert_eq!(search(&re, &input).unwrap(), Some((3, 5)));
    let wa = witness(&re).expect("the first validating bind records the witness");

    recompile(&scope, &re, "x(b)");
    assert_eq!(
        witness(&re),
        None,
        "a recompile emits a new cell, which starts without a witness"
    );
    assert_eq!(search(&re, &input).unwrap(), Some((0, 2)));
    let wb = witness(&re).expect("the new cell records its own witness");

    // Precondition for the next step: these programs share length and header.
    assert_eq!(
        wa, wb,
        "precondition: x(a) and x(b) must share length and header"
    );
    // Even a foreign witness over an equal header binds the cell's CURRENT
    // words, which Perex emitted: the answer is this program's.
    recompile(&scope, &re, "x(a)");
    set_witness(&re, Some(wb));
    assert_eq!(search(&re, &input).unwrap(), Some((3, 5)));
}

#[test]
fn perex_program_witness_that_does_not_match_falls_back_to_validation() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"ba");
    let a = regex(&scope, "a");
    let b = regex(&scope, "b");
    assert_eq!(search(&a, &input).unwrap(), Some((1, 2)));
    assert_eq!(search(&b, &input).unwrap(), Some((0, 1)));
    let (wa, wb) = (witness(&a).unwrap(), witness(&b).unwrap());
    assert_ne!(
        wa, wb,
        "precondition: a and b must differ in length or header"
    );

    set_witness(&a, Some(wb));
    assert_eq!(search(&a, &input).unwrap(), Some((1, 2)));
    assert_eq!(
        witness(&a),
        Some(wa),
        "a mismatched witness is replaced by validation"
    );
}

#[test]
fn perex_subject_is_marked_only_after_it_validates_with_its_exact_length() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let digits = regex(&scope, r"\d+");

    let good = text(&scope, "ä1 b22 c333 longer than inline".as_bytes());
    assert!(!validated(&good));
    assert_eq!(search(&digits, &good).unwrap(), Some((1, 2)));
    assert!(
        validated(&good),
        "a valid payload with an exact length is marked"
    );
    assert_eq!(
        search(&digits, &good).unwrap(),
        Some((1, 2)),
        "the counted bind answers the same"
    );

    let wrong_length = text(&scope, b"abcdef1 and longer than inline");
    wrong_length.with_const_ptr::<StringHeader, _>(|s| unsafe {
        (*(s as *mut StringHeader)).utf16_len = 3;
    });
    let _ = search(&digits, &wrong_length);
    assert!(
        !validated(&wrong_length),
        "a header whose length disagrees with its payload must never be trusted"
    );

    let malformed = text(&scope, b"\xff\xfe 1 malformed and longer than inline");
    let result = search(&digits, &malformed);
    assert!(
        result.is_err(),
        "malformed WTF-8 is rejected as before: {result:?}"
    );
    assert!(
        !validated(&malformed),
        "a payload that failed to validate is never marked"
    );
}

#[test]
fn perex_cross_call_marks_survive_moving_collections() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, r"\d+");
    let input = text(
        &scope,
        "ö7 and some text after it, beyond inline".as_bytes(),
    );
    assert_eq!(search(&re, &input).unwrap(), Some((1, 2)));
    let (w, marked) = (witness(&re), validated(&input));
    assert!(w.is_some() && marked);
    let (re_before, input_before) = (
        re.with_const_ptr::<RegExpHeader, _>(|p| p as usize),
        input.with_const_ptr::<StringHeader, _>(|p| p as usize),
    );
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);
    assert_ne!(
        input.with_const_ptr::<StringHeader, _>(|p| p as usize),
        input_before,
        "the string must move"
    );
    assert_ne!(
        re.with_const_ptr::<RegExpHeader, _>(|p| p as usize),
        re_before,
        "the RegExp must move"
    );
    assert_eq!(witness(&re), w, "the witness moves with its RegExp");
    assert!(validated(&input), "the mark moves with its string");
    assert_eq!(search(&re, &input).unwrap(), Some((1, 2)));
}

/// Debug builds prove a witness is never written under a live view of the
/// words it describes (#10166).
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "must not be written while a view of its words is live")]
fn perex_program_witness_write_under_a_live_view_is_caught() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, "q+");
    let input = text(&scope, b"qq");
    assert_eq!(search(&re, &input).unwrap(), Some((0, 2)));
    let witness = witness(&re).expect("validated");
    let owner = unsafe { GcProgram::from_receiver(&scope, &re) }.unwrap();
    let root = owner.root();
    let _ = owner.with_words(|_words| GcProgram::record_witness(&root, witness));
}
