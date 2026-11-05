//! RegExp `@@replace` without exec result objects, when every step the loop
//! would observe is the builtin one (#10165).
//!
//! The ordinary loop materializes a full exec result array per match, then
//! reads `length`, `0`, `index`, each capture and `groups` back through generic
//! property gets. For a receiver whose `exec` is the builtin, whose program
//! has no named groups and whose `lastIndex` is a Number, those objects and
//! reads are invisible: the arrays are fresh, own-data-property objects that
//! no user code can reach, and RegExpBuiltinExec's `lastIndex` read runs no
//! code. This path collects each match's spans natively instead and builds
//! the output from spans of the input, in one `Output`.
//!
//! The specification's order is kept: every match is collected before the
//! first replacer call, so a replacer that changes `lastIndex`, `exec` or the
//! pattern cannot change which matches are replaced. The observable steps
//! before the loop (`flags`, the `lastIndex` reset) still run in the caller,
//! and admission is decided after them because either can run user code.
//!
//! Inside the collection loop no user code can run, so the searches skip
//! RegExpBuiltinExec's `lastIndex` traffic: each starts where the previous
//! one left `lastIndex` (its end, advanced past an empty match), and only the
//! final value is written. A global loop's final search fails, writing 0,
//! which is what the caller's reset already stored; a sticky non-global
//! search writes its end or 0 once. No intermediate value is observable.
//! A template replacement runs no user code at all, so it arms no trap.
use super::perex_api::Reuse;
use super::perex_match_search::advance;
use super::perex_memory::{MemoryBudget, StorageError};
use super::perex_output::{Offsets, Output, Text};
use super::perex_owner::HeapSubject;
use super::perex_replace_storage::{boxed, call, call_native, length, List, NativeArgs};
use super::perex_runtime::{self as host, CaptureMode, EngineError};
use super::perex_strings::SpanCopies;
use super::perex_substitution::{emit, parse, Parts, Token};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{JSValue, TAG_UNDEFINED};
use perex::binding::BoundSubject;
use perex::Budget;

#[cfg(test)]
crate::perry_thread_local! {
    static DIRECT_REPLACES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DIRECT_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static UNMATCHED_EXITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn direct_replaces() -> usize {
    DIRECT_REPLACES.with(std::cell::Cell::get)
}

/// Replacements answered by the leading-unit exit (`perex_replace::unmatched`).
#[cfg(test)]
pub(crate) fn unmatched_exits() -> usize {
    UNMATCHED_EXITS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(super) fn note_unmatched_exit() {
    UNMATCHED_EXITS.with(|n| n.set(n.get() + 1));
}

/// Whether a test holds [`DisableDirectReplaceForTest`]; the leading-unit
/// exit steps aside as well, so the ordinary loop is what runs.
#[cfg(test)]
pub(super) fn direct_disabled() -> bool {
    DIRECT_DISABLED.with(std::cell::Cell::get)
}

/// The ordinary loop runs while this is held, so a test can compare the two.
#[cfg(test)]
pub(crate) struct DisableDirectReplaceForTest(bool);

#[cfg(test)]
impl DisableDirectReplaceForTest {
    pub(crate) fn new() -> Self {
        Self(DIRECT_DISABLED.with(|d| d.replace(true)))
    }
}

#[cfg(test)]
impl Drop for DisableDirectReplaceForTest {
    fn drop(&mut self) {
        DIRECT_DISABLED.with(|d| d.set(self.0));
    }
}

/// Whether this replacement may skip exec result objects and `lastIndex`
/// traffic. Non-observable: the builtin RegExp fact proves the brand, `exec`
/// and an own data `lastIndex` slot; the name count is the program's.
pub(super) fn admissible(receiver: &RuntimeHandle<'_>, reuse: &Reuse<'_, '_>) -> bool {
    #[cfg(test)]
    if DIRECT_DISABLED.with(std::cell::Cell::get) {
        return false;
    }
    let Some(re) = crate::regex::instance::builtin(receiver.get_nanbox_f64()) else {
        return false;
    };
    reuse.name_count(re) == Some(0)
        // SAFETY: admitted just now; nothing collected since.
        && JSValue::from_bits(unsafe { crate::regex::instance::builtin_last_index(re) }.to_bits())
            .is_number()
}

/// One collection-loop round in this many runs the GC safepoint poll.
///
/// The value matches `PRE_SEARCH_POLL_STRIDE`, and deliberately so: both count
/// one search, so they are parallel on the same unit rather than nested. Note
/// that 64 is a chosen margin in #10494, not a derived one -- the evidence
/// there (removing the poll left `cycle_starts`, `completions` and `steps`
/// identical across 48,000,000 calls) argues for removal and does not pick a
/// stride. Nothing here relies on 64 being the right number; see the call site.
const COLLECT_POLL_STRIDE: usize = 64;

/// A collected match's spans as GetSubstitution's parts: the matched text and
/// every capture are spans of the subject.
struct Spans<'r> {
    record: &'r [u32],
}

impl Parts for Spans<'_> {
    fn matched_len(&self) -> usize {
        (self.record[1] - self.record[0]) as usize
    }
    fn matched(
        &mut self,
        input: &mut Text<'_, '_>,
        output: &mut Output,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        output.span(
            input,
            self.record[0] as usize,
            self.record[1] as usize,
            budget,
        )
    }
    fn capture(
        &mut self,
        index: usize,
        input: &mut Text<'_, '_>,
        output: &mut Output,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        let (a, b) = (self.record[2 * index], self.record[2 * index + 1]);
        if a == u32::MAX {
            return Ok(());
        }
        output.span(input, a as usize, b as usize, budget)
    }
}

fn index(n: usize) -> Result<u32, EngineError> {
    u32::try_from(n).map_err(|_| StorageError::Limit.into())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn replace(
    scope: &RuntimeHandleScope,
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    bound: &BoundSubject<HeapSubject<'_>>,
    reuse: &Reuse<'_, '_>,
    global: bool,
    unicode: bool,
    replacement: &RuntimeHandle<'_>,
    template: Option<&RuntimeHandle<'_>>,
    budget: &mut Budget,
    memory: &MemoryBudget,
) -> Result<f64, EngineError> {
    #[cfg(test)]
    DIRECT_REPLACES.with(|n| n.set(n.get() + 1));
    // The ordinary loop's RegExpExec adds a reference to the input per search.
    input.with_mut_ptr::<StringHeader, _>(|input| crate::string::js_string_addref(input));
    let input_length = length(input);
    let groups = reuse
        .capture_count()
        .ok_or(EngineError::InvalidSpan)?
        .saturating_sub(1);
    // A capture-bearing template must be inspected to choose the search's
    // capture mode. Without captures, GetSubstitution needs no preparation
    // until a match exists.
    let mut tokens = if groups > 0 {
        template.map(|t| parse(t, groups, false)).transpose()?
    } else {
        None
    };
    // Captures are produced only when something reads them: a replacer
    // receives them all, a template only those it names. Otherwise the
    // search needs the match bounds alone, which the automaton answers.
    let captures = groups > 0
        && tokens
            .as_ref()
            .is_none_or(|t| t.iter().any(|t| matches!(t, Token::Capture(_))));
    let (mode, width) = if captures {
        (CaptureMode::All, 2 * (groups + 1))
    } else {
        (CaptureMode::Full, 2)
    };
    let sticky =
        unsafe { (*crate::regex::regexp_data_ptr(super::perex_api::regexp(receiver))).sticky };
    // RegExpBuiltinExec's start: a global receiver's `lastIndex` was just
    // reset to 0; admission proved it a Number otherwise, which only a
    // sticky receiver reads.
    let mut start = if global || !sticky {
        0
    } else {
        let stored = crate::regex::get_last_index(super::perex_api::regexp(receiver));
        stored.max(0.0).floor().min(9_007_199_254_740_991.0) as usize
    };
    let mut spans = Offsets::new();
    let mut slots = None;
    let mut searches = 0usize;
    let mut last = None;
    loop {
        host::charge(budget, 1)?;
        // Each match ends at or after the next search's start, and an empty one
        // advances, so a global loop runs at most once per position plus the
        // final failing search. More means it stopped advancing.
        searches += 1;
        debug_assert!(
            searches <= input_length + 2,
            "a global replace searched more often than its input has positions"
        );
        let found = if start > input_length {
            None
        } else {
            reuse.find(start, mode, budget, memory, &mut slots)?
        };
        let Some(full) = found else {
            break;
        };
        spans.reserve(width)?;
        if captures {
            let slots = slots.as_ref().ok_or(EngineError::InvalidSpan)?;
            for capture in slots.iter() {
                let (a, b) = match capture {
                    Some(span) => (index(span.start())?, index(span.end())?),
                    None => (u32::MAX, u32::MAX),
                };
                spans.values.push(a);
                spans.values.push(b);
            }
        } else {
            spans.values.push(index(full.start())?);
            spans.values.push(index(full.end())?);
        }
        if !global {
            last = Some(full.end());
            break;
        }
        start = if full.is_empty() {
            advance(bound, full.end() as f64, input_length, unicode, budget)? as usize
        } else {
            full.end()
        };
        // One collection-loop round in COLLECT_POLL_STRIDE runs the safepoint.
        //
        // This loop writes each match's spans into a native buffer and creates
        // no JS garbage, so its poll enables no collection: nulling it moves
        // peak RSS by +0.0% median over nine interleaved rounds of an
        // allocating replace at n=1,000,000.
        //
        // Worst-case work between executed polls does not grow. Every search
        // ticks `PRE_SEARCH_POLL_TICK` and polls on one search in 64 (#10494),
        // once per iteration of this loop, so the two strides run in parallel
        // on the same unit rather than composing.
        if searches.is_multiple_of(COLLECT_POLL_STRIDE) {
            host::poll()?;
        }
    }
    if sticky && !global {
        super::perex_dispatch::set_last_index(receiver, last.unwrap_or(0) as f64)?;
    }
    if spans.values.is_empty() {
        return Ok(boxed(input));
    }
    if tokens.is_none() {
        tokens = template.map(|t| parse(t, groups, false)).transpose()?;
    }
    let mut subject_text = Text::new(bound)?;
    let bytes = input.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).byte_len as usize });
    let mut output = Output::with_capacity(bytes)?;
    let mut next_source = 0;
    if let (Some(tokens), Some(template)) = (tokens.as_ref(), template) {
        // No user code runs here: every piece is a span of the subject or
        // of the template.
        for record in spans.values.chunks_exact(width) {
            let (start, end) = (record[0] as usize, record[1] as usize);
            let position = start.min(input_length);
            if position < next_source {
                continue;
            }
            output.span(&mut subject_text, next_source, position, budget)?;
            emit(
                tokens,
                template,
                &mut subject_text,
                position,
                &mut Spans { record },
                &mut output,
                budget,
            )?;
            next_source = end;
        }
    } else {
        let mut copies = SpanCopies::new(bound)?;
        // An ordinary replacer's arguments never reach user code as an array, so
        // they are produced straight into shadow-stack slots. A proxy replacer's do
        // reach it, through the `apply` trap, and keep the JS array. Sized once:
        // the program's capture count fixes the argument count for every match.
        let mut native_args =
            if crate::proxy_hooks::proxy_is_proxy(replacement.get_nanbox_f64()) == 1 {
                None
            } else {
                Some(NativeArgs::new(width / 2 + 2)?)
            };
        let this = scope.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
        for record in spans.values.chunks_exact(width) {
            let local = RuntimeHandleScope::new();
            let (start, end) = (record[0] as usize, record[1] as usize);
            let position = start.min(input_length);
            let value = if let Some(args) = native_args.as_mut() {
                let copies = &mut copies;
                call_native(replacement, &this, args, memory, |set| {
                    let matched = copies.copy_value(start, end, budget)?;
                    set(0, matched);
                    let mut slot = 1;
                    for pair in record[2..].as_chunks::<2>().0 {
                        // An unset capture is the `undefined` the slot already holds.
                        if pair[0] != u32::MAX {
                            let capture =
                                copies.copy_value(pair[0] as usize, pair[1] as usize, budget)?;
                            set(slot, capture);
                        }
                        slot += 1;
                    }
                    set(slot, position as f64);
                    set(slot + 1, boxed(input));
                    Ok(())
                })?
            } else {
                let mut args = List::new(&local)?;
                let matched = copies.copy_value(start, end, budget)?;
                args.push(matched, budget)?;
                for pair in record[2..].as_chunks::<2>().0 {
                    if pair[0] == u32::MAX {
                        args.push(f64::from_bits(TAG_UNDEFINED), budget)?;
                    } else {
                        let capture =
                            copies.copy_value(pair[0] as usize, pair[1] as usize, budget)?;
                        args.push(capture, budget)?;
                    }
                }
                args.push(position as f64, budget)?;
                args.push(boxed(input), budget)?;
                call(replacement, &this, &args, memory)?
            };
            let value = local.root_nanbox_f64(value);
            // ToString runs for every result, replaced or not.
            if position >= next_source {
                output.span(&mut subject_text, next_source, position, budget)?;
                output.value(&value, budget)?;
                next_source = end;
            } else {
                super::perex_dispatch::to_string(&value)?;
            }
        }
    }
    if next_source < input_length {
        output.span(&mut subject_text, next_source, input_length, budget)?;
    }
    output.finish()
}

#[cfg(test)]
pub(crate) fn template_parse_calls() -> usize {
    super::perex_substitution::parse_calls()
}

#[cfg(test)]
pub(crate) fn literal_heap_buffers() -> usize {
    super::perex_literal_search::heap_buffers()
}
