//! RegExp `@@replace` without exec result objects (#10165): the same output
//! and final `lastIndex` as the ordinary loop, and only for receivers where
//! skipping the objects cannot be seen.
use super::perex_replace::{bytes, captured, function, get, put, regex, text};
use super::*;
use crate::regex::perex_api as api;
use crate::regex::perex_replace_direct::{direct_replaces, DisableDirectReplaceForTest};
use crate::value::{js_nanbox_string, TAG_UNDEFINED};

/// Replace through the public ABI; the output bytes and the receiver's final
/// `lastIndex`, and whether the direct path served it.
fn replace_all(
    source: &[u8],
    flags: &[u8],
    input: &[u8],
    replacement: impl Fn(&RuntimeHandleScope, &RuntimeHandle<'_>) -> f64,
) -> (Vec<u8>, f64, bool) {
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, source, flags);
    let input = text(&scope, input);
    let replacement = scope.root_nanbox_f64(replacement(&scope, &re));
    let before = direct_replaces();
    let result = scope.root_nanbox_f64(crate::regex::js_string_replace_js(
        input.get_nanbox_f64(),
        re.get_nanbox_f64(),
        replacement.get_nanbox_f64(),
    ));
    (
        bytes(result.get_nanbox_f64()),
        get(&re, b"lastIndex"),
        direct_replaces() > before,
    )
}

fn both(
    source: &[u8],
    flags: &[u8],
    input: &[u8],
    replacement: impl Fn(&RuntimeHandleScope, &RuntimeHandle<'_>) -> f64 + Copy,
) -> (Vec<u8>, f64) {
    let (direct, direct_last, served) = replace_all(source, flags, input, replacement);
    assert!(
        served,
        "{:?} /{:?}/ must take the direct path",
        std::str::from_utf8(source),
        flags
    );
    let (ordinary, ordinary_last, served) = {
        let _off = DisableDirectReplaceForTest::new();
        replace_all(source, flags, input, replacement)
    };
    assert!(!served);
    assert_eq!(
        (String::from_utf8_lossy(&direct), direct_last),
        (String::from_utf8_lossy(&ordinary), ordinary_last),
        "/{}/{} over {:?}",
        String::from_utf8_lossy(source),
        String::from_utf8_lossy(flags),
        String::from_utf8_lossy(input)
    );
    (direct, direct_last)
}

fn template(
    value: &'static [u8],
) -> impl Fn(&RuntimeHandleScope, &RuntimeHandle<'_>) -> f64 + Copy {
    move |scope, _| text(scope, value).get_nanbox_f64()
}

/// Pattern, flags, input, template, and the expected output when it is pinned.
type Case = (
    &'static [u8],
    &'static [u8],
    &'static [u8],
    &'static [u8],
    &'static str,
);

#[test]
fn direct_templates_match_the_ordinary_loop() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let cases: &[Case] = &[
        (
            b"(a)(z)?",
            b"g",
            b"abaz a",
            b"[$&|$1|$2|$`|$'|$$|$0|$3|$10|$<x>|$]",
            "",
        ),
        (b"(\\d)(\\d)?", b"g", b"1 23 4", b"<$2$1$11$01$12$00>", ""),
        (b"a", b"", b"banana", b"[$`|$']", "b[b|nana]nana"),
        (b"", b"g", "aé😀b".as_bytes(), b"<$&>", ""),
        (b"", b"gu", "aé😀b".as_bytes(), b"<$&>", "<>a<>é<>😀<>b<>"),
        (b"b", b"y", b"abba", b"X", "abba"),
        (b"b", b"gy", b"bba", b"X", "XXa"),
        (
            "[äö]+".as_bytes(),
            b"g",
            "xäöyö".as_bytes(),
            b"($&)",
            "x(äö)y(ö)",
        ),
        (b"x", b"g", b"no match here", b"Y", "no match here"),
    ];
    for (source, flags, input, replacement, expected) in cases {
        let (output, _) = both(source, flags, input, template(replacement));
        if !expected.is_empty() {
            assert_eq!(String::from_utf8_lossy(&output), *expected);
        }
    }
}

extern "C" fn describe(
    _: *const crate::closure::ClosureHeader,
    matched: f64,
    capture: f64,
    position: f64,
    input: f64,
) -> f64 {
    let capture = if capture.to_bits() == TAG_UNDEFINED {
        "undefined".to_string()
    } else {
        String::from_utf8_lossy(&bytes(capture)).into_owned()
    };
    let text = format!(
        "{{{}:{}@{}/{}}}",
        String::from_utf8_lossy(&bytes(matched)),
        capture,
        position,
        bytes(input).len()
    );
    gc_collect_minor();
    js_nanbox_string(crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32) as i64)
}

#[test]
fn direct_callbacks_receive_the_ordinary_arguments() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let callback = |scope: &RuntimeHandleScope, _: &RuntimeHandle<'_>| {
        function(scope, describe as *const u8, 4).get_nanbox_f64()
    };
    let (output, _) = both(b"(b)?a", b"g", "bä a ba".as_bytes(), callback);
    assert_eq!(
        String::from_utf8_lossy(&output),
        "bä {a:undefined@3/8} {ba:b@5/8}"
    );
}

extern "C" fn meddle(
    c: *const crate::closure::ClosureHeader,
    matched: f64,
    _position: f64,
    _input: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let matched = scope.root_nanbox_f64(matched);
    let state = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    // Rewind the receiver and give it an own exec that matches nothing. Every
    // match was collected before the first call, so neither can change them.
    api::finish(crate::regex::perex_dispatch::set_last_index(&state, 0.0));
    let never = function(&scope, never_exec as *const u8, 1);
    put(&state, b"exec", never.get_nanbox_f64());
    matched.get_nanbox_f64()
}

extern "C" fn never_exec(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

#[test]
fn a_replacer_cannot_change_which_matches_are_replaced() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let callback = |scope: &RuntimeHandleScope, re: &RuntimeHandle<'_>| {
        captured(scope, meddle as *const u8, 3, re).get_nanbox_f64()
    };
    let (output, last) = both(b"o", b"g", b"foo boo", callback);
    assert_eq!(output, b"foo boo");
    assert_eq!(last, 0.0);
}

extern "C" fn counting_exec(c: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(c, 0));
    put(&state, b"calls", get(&state, b"calls") + 1.0);
    f64::from_bits(crate::value::TAG_NULL)
}

#[test]
fn an_own_exec_or_named_groups_keep_the_ordinary_loop() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();

    let (_, _, served) = replace_all(b"a", b"g", b"banana", |scope, re| {
        put(re, b"calls", 0.0);
        let exec = captured(scope, counting_exec as *const u8, 1, re);
        put(re, b"exec", exec.get_nanbox_f64());
        text(scope, b"X").get_nanbox_f64()
    });
    assert!(
        !served,
        "an own exec must be called, so the direct path must decline"
    );

    let (output, _, served) = replace_all(b"(?<v>a)", b"g", b"banana", template(b"[$<v>]"));
    assert!(
        !served,
        "named groups need the groups object, so the direct path must decline"
    );
    assert_eq!(output, b"b[a]n[a]n[a]");
}

#[test]
fn direct_match_spans_are_not_capped_by_the_scratch_limit() {
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    // 63 groups: each match stores 64 span pairs, 128 entries. One match more
    // than a SCRATCH_BYTES / 8 entry cap (the limit #10207 removed from the
    // piece lists) allows, so a cap on span storage would throw here.
    let groups = 63;
    let entries_per_match = (groups + 1) * 2;
    let matches = api::SCRATCH_BYTES / 8 / entries_per_match + 1;
    let source = "(a)".repeat(groups) + "a";
    let input = "a".repeat((groups + 1) * matches);
    let (output, last, served) =
        replace_all(source.as_bytes(), b"g", input.as_bytes(), template(b"$1"));
    assert!(served, "the witness must run on the direct path");
    assert_eq!(output.len(), matches);
    assert!(output.iter().all(|&byte| byte == b'a'));
    assert_eq!(last, 0.0);
}
