//! RegExp operations are not capped by work (#10164): a valid program whose
//! matching charges more than the former 100,000,000-unit allowance completes.
use super::*;
use crate::regex::perex_api as api;
use crate::regex::perex_memory::MemoryBudget;
use crate::regex::RegExpHeader;
use crate::string::StringHeader;
use perex::Budget;

const FORMER_WORK_LIMIT: usize = 100_000_000;

fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ))
}

#[test]
fn perex_operation_allowance_admits_linear_work_beyond_the_former_limit() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    // One search that is linear but heavy: at every position the lookahead
    // reads 32 units and then fails on a character class the subject never
    // contains, so it matches nothing and allocates no results. The pattern has
    // no required literal on purpose; a literal lets admission reject the whole
    // search in one pass without doing the per-position work. Measured at about
    // 220 work units per subject unit, so 540,000 units charge about 1.2e8.
    let pattern = text(&scope, br"\w(?=[\w,; ]{32}[^\w,; ])");
    let flags = text(&scope, b"");
    let receiver = scope.root_raw_mut_ptr(pattern.with_const_ptr::<StringHeader, _>(|pattern| {
        flags.with_const_ptr::<StringHeader, _>(|flags| crate::regex::js_regexp_new(pattern, flags))
    }));
    let input = text(&scope, "ab12,cd345;ef6 ".repeat(36_000).as_bytes());
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut budget = Budget::new(api::WORK);
    let found = receiver.with_mut_ptr::<RegExpHeader, _>(|receiver| {
        input.with_const_ptr::<StringHeader, _>(|input| {
            api::execute_with_resources(
                receiver,
                input,
                false,
                &mut budget,
                &memory,
                &mut || Ok(()),
                None,
            )
        })
    });
    let charged = api::WORK - budget.remaining();
    assert!(
        matches!(found, Ok(None)),
        "a valid search must complete without a work-limit error"
    );
    assert!(
        charged > FORMER_WORK_LIMIT,
        "the witness must charge more than the former limit to prove anything; \
         charged {charged}, so a Perex change may have made this pattern cheaper"
    );
}
