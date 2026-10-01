//! #11549: a regex operation's own scratch is not collector pressure.
//!
//! Operation scratch (`perex_memory::Buffer`, the owned search path's match
//! buffers, the lent cell) is freed by the operation, never by a collection.
//! Reporting it as external side bytes put every per-call buffer's release into
//! `GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL`, which old-reclaim holds as pressure
//! until the next full: dotenv's `LINE` (42 registers, past the lent cell's old
//! fixed 32) ran a budgeted full mark-sweep every ~250 parses on phantom bytes.
use super::*;
use crate::gc::policy::{external_side_live_bytes, GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL};
use crate::regex::perex_api as api;
use crate::regex::perex_memory::{Buffer, MemoryBudget};
use crate::regex::perex_runtime::{EngineError, OWNED_SEARCHES};
use crate::regex::RegExpHeader;
use crate::string::StringHeader;

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

/// `groups` capturing groups of one `a` each: `(a)(a)...`. A program's register
/// count is at least twice its capture count (group zero included), so this
/// needs at least `2 * (groups + 1)` registers.
fn groups_pattern(groups: usize) -> String {
    "(a)".repeat(groups)
}

fn external_readings() -> (usize, usize) {
    (
        external_side_live_bytes(),
        GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(TriggerInput::get),
    )
}

fn owned_searches() -> usize {
    OWNED_SEARCHES.with(std::cell::Cell::get)
}

#[test]
fn perex_operation_buffer_is_not_reported_as_external_side_bytes() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let before = external_readings();
    let memory = MemoryBudget::new(1 << 20);
    {
        let buffer = Buffer::<usize>::new(&memory, 4096).expect("fits the budget");
        assert_eq!(buffer.len(), 4096);
        assert_eq!(memory.live_bytes(), 4096 * 8, "the budget still sees it");
        assert_eq!(external_readings(), before, "allocation noted nothing");
    }
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(memory.peak_bytes(), 4096 * 8);
    assert_eq!(
        external_readings(),
        before,
        "a released operation buffer is not drained external pressure (#11549)"
    );
    // The budget remains the bound: past it, nothing is allocated.
    assert!(Buffer::<usize>::new(&memory, (1 << 20) / 8 + 1).is_err());
}

#[test]
fn perex_search_past_32_registers_uses_the_lent_cell_and_notes_nothing() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    // 20 groups: 42+ registers, which the lent cell's old fixed 32 could not
    // hold, so every call built and dropped owned match buffers.
    let re = regex(&scope, &groups_pattern(20));
    let input = text(&scope, "a".repeat(25).as_bytes());
    // The first call may grow the thread's cell; every later one must not
    // build anything.
    assert_eq!(search(&re, &input).unwrap(), Some((0, 20)));
    let before = external_readings();
    let owned = owned_searches();
    for _ in 0..64 {
        assert_eq!(search(&re, &input).unwrap(), Some((0, 20)));
    }
    assert_eq!(
        owned_searches(),
        owned,
        "a program within the lent bound searches on the lent cell"
    );
    assert_eq!(
        external_readings(),
        before,
        "and reports no scratch to the collector"
    );
}

#[test]
fn perex_search_past_the_lent_bound_takes_the_owned_path_and_notes_nothing() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    // 600 groups: 1,202+ registers, past what the lent cell may retain. The
    // cell must not grow to it; the operation builds, and frees, its own.
    let re = regex(&scope, &groups_pattern(600));
    let input = text(&scope, "a".repeat(600).as_bytes());
    let before = external_readings();
    let owned = owned_searches();
    for _ in 0..4 {
        assert_eq!(search(&re, &input).unwrap(), Some((0, 600)));
    }
    assert_eq!(
        owned_searches(),
        owned + 4,
        "a program past the lent bound runs the owned path every call"
    );
    assert_eq!(
        external_readings(),
        before,
        "owned match buffers are released by the search, not drained into GC pressure"
    );
}
