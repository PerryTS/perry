//! RegExp `@@replace` without exec result objects, when every step the loop
//! would observe is the builtin one (#10165).
//!
//! The ordinary loop materializes a full exec result array per match, then
//! reads `length`, `0`, `index`, each capture and `groups` back through generic
//! property gets. For a receiver whose `exec` is the builtin and whose program
//! has no named groups, those objects and reads are invisible: the arrays are
//! fresh, own-data-property objects that no user code can reach. This path
//! collects each match's capture spans natively instead and builds the output
//! from spans of the input.
//!
//! The specification's order is kept: every match is collected before the
//! first replacer call, so a replacer that changes `lastIndex`, `exec` or the
//! pattern cannot change which matches are replaced. The observable steps
//! before the loop (`flags`, the `lastIndex` reset) still run in the caller,
//! and admission is decided after them because either can run user code.
//! Inside the collection loop no user code can run: `lastIndex` was reset to a
//! Number and the builtin search only writes Numbers.
use super::perex_api::{self as api, ExecOutput, Reuse};
use super::perex_match_search::advance;
use super::perex_memory::{MemoryBudget, StorageError};
use super::perex_owner::HeapSubject;
use super::perex_replace_storage::{
    boxed, call, call_native, length, text, List, NativeArgs, Pieces, Units,
};
use super::perex_runtime::{self as host, EngineError};
use super::perex_strings::SpanCopies;
use super::RegExpHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{js_nanbox_string, TAG_UNDEFINED};
use perex::binding::BoundSubject;
use perex::Budget;

#[cfg(test)]
crate::perry_thread_local! {
    static DIRECT_REPLACES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DIRECT_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn direct_replaces() -> usize {
    DIRECT_REPLACES.with(std::cell::Cell::get)
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

/// Whether this replacement may skip exec result objects. Non-observable.
pub(super) fn admissible(receiver: &RuntimeHandle<'_>, reuse: &Reuse<'_, '_>) -> bool {
    #[cfg(test)]
    if DIRECT_DISABLED.with(std::cell::Cell::get) {
        return false;
    }
    let value = receiver.get_nanbox_f64();
    let re = crate::value::js_nanbox_get_pointer(value) as *const RegExpHeader;
    super::is_valid_regex_ptr(re)
        && crate::object::regex_proto_thunks::regexp_view_uses_builtin(value)
        && reuse.name_count(re) == Some(0)
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

/// One piece of a template: a span of the template itself, or a part of the
/// current match. Parsed once; the capture count is the program's.
#[derive(Clone, Copy)]
enum Token {
    Template(usize, usize),
    Matched,
    Before,
    After,
    Capture(usize),
}

/// GetSubstitution's scan (as `perex_substitution` performs it) with no named
/// groups, recorded once instead of per match.
fn parse(
    template: &RuntimeHandle<'_>,
    captures: usize,
    budget: &mut Budget,
) -> Result<Vec<Token>, EngineError> {
    let bound = super::perex_match_search::subject(*template)?;
    let mut reader = Units::new(&bound)?;
    let n = length(template);
    let mut tokens = Vec::new();
    let (mut i, mut literal) = (0, 0);
    while i < n {
        if reader.at(i, budget)? != b'$' as u16 || i + 1 == n {
            i += 1;
            continue;
        }
        let marker = reader.at(i + 1, budget)?;
        let mut next = i + 2;
        let token = match marker {
            0x24 => Token::Template(i, i + 1),
            0x26 => Token::Matched,
            0x60 => Token::Before,
            0x27 => Token::After,
            0x30..=0x39 => {
                let mut index = (marker - 0x30) as usize;
                if next < n {
                    let second = reader.at(next, budget)?;
                    if (0x30..=0x39).contains(&second) {
                        let two = index * 10 + (second - 0x30) as usize;
                        if two > 0 && two <= captures {
                            index = two;
                            next += 1;
                        }
                    }
                }
                if index == 0 || index > captures {
                    i += 1;
                    continue;
                }
                Token::Capture(index)
            }
            _ => {
                i += 1;
                continue;
            }
        };
        tokens
            .try_reserve(2)
            .map_err(|_| StorageError::Allocation)?;
        tokens.push(Token::Template(literal, i));
        tokens.push(token);
        i = next;
        literal = i;
    }
    tokens.push(Token::Template(literal, n));
    Ok(tokens)
}

/// Every match's capture spans for one replacement. Their size follows the
/// subject (matches times captures), not a fixed operation limit, so they are
/// not charged to the `MemoryBudget`: a cap there made large replacements
/// throw where Node completes (#10164). The collector is told about them as
/// external bytes, like other runtime side storage.
struct Spans {
    values: Vec<u32>,
    noted: usize,
}

impl Spans {
    fn note_growth(&mut self) -> Result<(), EngineError> {
        let bytes = self.values.capacity() * std::mem::size_of::<u32>();
        if bytes > self.noted {
            let grown = bytes - self.noted;
            self.noted = bytes;
            api::caught(|| crate::gc::gc_note_external_side_alloc(grown))?;
        }
        Ok(())
    }
}

impl Drop for Spans {
    fn drop(&mut self) {
        crate::gc::gc_note_external_side_free(self.noted);
    }
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
    let mut spans = Spans {
        values: Vec::new(),
        noted: 0,
    };
    let mut width = 0;
    let mut searches = 0usize;
    loop {
        host::charge(budget, 1)?;
        // Each match ends at or after the next search's start, and an empty one
        // advances `lastIndex`, so a global loop runs at most once per position
        // plus the final failing search. More means it stopped advancing.
        searches += 1;
        debug_assert!(
            searches <= input_length + 2,
            "a global replace searched more often than its input has positions"
        );
        let before = spans.values.len();
        // Re-read the receiver every search: the previous one may have collected.
        let re =
            crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *mut RegExpHeader;
        let found = input.with_const_ptr::<StringHeader, _>(|input| {
            api::execute_output(
                re,
                input,
                ExecOutput::Spans(&mut spans.values),
                budget,
                memory,
                &mut host::poll,
                Some(reuse),
            )
        })?;
        spans.note_growth()?;
        let Some(found) = found else {
            break;
        };
        width = spans.values.len() - before;
        if !global {
            break;
        }
        if found.full.is_empty() {
            let local = RuntimeHandleScope::new();
            let index = local.root_nanbox_f64(super::perex_dispatch::get(receiver, b"lastIndex")?);
            let index = super::perex_dispatch::to_length(&index)?;
            let next = advance(bound, index, input_length, unicode, budget)?;
            super::perex_dispatch::set_last_index(receiver, next)?;
        }
        // One collection-loop round in COLLECT_POLL_STRIDE runs the safepoint.
        //
        // This loop writes each match's spans into a native buffer and creates
        // no JS garbage, so its poll enables no collection: nulling it moves
        // peak RSS by +0.0% median over nine interleaved rounds of an
        // allocating replace at n=1,000,000. (For contrast, polling 8x less
        // often in `Pieces::finish`, which does produce garbage, moved the same
        // figure +13.2%.)
        //
        // Worst-case work between executed polls does not grow. Every search
        // this loop performs goes through `find_near`, which either polls
        // unconditionally (the owned path, and any lent fallback) or ticks
        // `PRE_SEARCH_POLL_TICK` and polls on one search in 64 (#10494). That
        // tick advances once per search, which is once per iteration of this
        // loop, so the two strides run in parallel on the same unit rather than
        // composing: the bound stays 64 searches whether this poll is strided
        // or not. Striding a site whose own counter advanced on a *different*
        // unit would not be safe on this argument.
        if searches % COLLECT_POLL_STRIDE == 0 {
            host::poll()?;
        }
    }
    if spans.values.is_empty() {
        return Ok(boxed(input));
    }
    let captures = (width / 2).saturating_sub(1);
    let tokens = template.map(|t| parse(t, captures, budget)).transpose()?;
    let mut copies = SpanCopies::new(bound)?;
    // A string template's pieces are all spans of the subject or the template,
    // so they need no traced heap entry (#10411). A callback's do: user code
    // produces the replacement string.
    let mut output = if tokens.is_some() {
        Pieces::new_native(scope)?
    } else {
        Pieces::new(scope)?
    };
    // An ordinary replacer's arguments never reach user code as an array, so
    // they are produced straight into shadow-stack slots. A proxy replacer's do
    // reach it, through the `apply` trap, and keep the JS array. Sized once:
    // the program's capture count fixes the argument count for every match.
    let mut native_args =
        if tokens.is_some() || crate::proxy::js_proxy_is_proxy(replacement.get_nanbox_f64()) == 1 {
            None
        } else {
            Some(NativeArgs::new(captures + 3)?)
        };
    let mut next_source = 0;
    for record in spans.values.chunks_exact(width) {
        let local = RuntimeHandleScope::new();
        let (start, end) = (record[0] as usize, record[1] as usize);
        let position = start.min(input_length);
        let accepted = position >= next_source;
        if accepted {
            output.append_original(input, next_source, position, budget)?;
        }
        if let Some(tokens) = tokens.as_ref() {
            if accepted {
                for token in tokens {
                    match *token {
                        Token::Template(a, b) => {
                            output.append_template(template.unwrap(), a, b, budget)?
                        }
                        Token::Matched => output.append_original(input, start, end, budget)?,
                        Token::Before => output.append_original(input, 0, position, budget)?,
                        Token::After => output.append_original(
                            input,
                            end.min(input_length),
                            input_length,
                            budget,
                        )?,
                        Token::Capture(index) => {
                            let (a, b) = (record[2 * index], record[2 * index + 1]);
                            if a != u32::MAX {
                                output.append_original(input, a as usize, b as usize, budget)?;
                            }
                        }
                    }
                }
            }
        } else if let Some(args) = native_args.as_mut() {
            let this = local.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            let copies = &mut copies;
            let value = call_native(replacement, &this, args, memory, |set| {
                let matched = copies.copy(start, end, budget)?;
                set(0, js_nanbox_string(matched as i64));
                let mut slot = 1;
                for pair in record[2..].as_chunks::<2>().0 {
                    // An unset capture is the `undefined` the slot already holds.
                    if pair[0] != u32::MAX {
                        let capture = copies.copy(pair[0] as usize, pair[1] as usize, budget)?;
                        set(slot, js_nanbox_string(capture as i64));
                    }
                    slot += 1;
                }
                set(slot, position as f64);
                set(slot + 1, boxed(input));
                Ok(())
            })?;
            let value = local.root_nanbox_f64(value);
            let value = text(&local, &value)?;
            if accepted {
                output.whole(&value, budget)?;
            }
        } else {
            let mut args = List::new(&local)?;
            let matched = copies.copy(start, end, budget)?;
            args.push(js_nanbox_string(matched as i64), budget)?;
            for pair in record[2..].as_chunks::<2>().0 {
                if pair[0] == u32::MAX {
                    args.push(f64::from_bits(TAG_UNDEFINED), budget)?;
                } else {
                    let capture = copies.copy(pair[0] as usize, pair[1] as usize, budget)?;
                    args.push(js_nanbox_string(capture as i64), budget)?;
                }
            }
            args.push(position as f64, budget)?;
            args.push(boxed(input), budget)?;
            let this = local.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            let value = local.root_nanbox_f64(call(replacement, &this, &args, memory)?);
            let value = text(&local, &value)?;
            if accepted {
                output.whole(&value, budget)?;
            }
        }
        if accepted {
            next_source = end;
        }
        host::poll()?;
    }
    if next_source < input_length {
        output.append_original(input, next_source, input_length, budget)?;
    }
    output
        .finish(input, template, budget)
        .map(|s| js_nanbox_string(s as i64))
}
