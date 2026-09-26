//! String.split and generic RegExp @@split using original UTF-16 spans.
//! Species/exec hooks retain ordinary traced values; no input conversion buffer.
use super::perex_api as api;
use super::perex_dispatch as dispatch;
use super::perex_match_search::subject;
use super::perex_memory::MemoryBudget;
use super::perex_owner::GcProgram;
use super::perex_owner::HeapSubject;
use super::perex_replace::{callable, index_property};
use super::perex_replace_storage::{boxed, call, length, text, List, Pieces, Units};
use super::perex_runtime::{self as host, CaptureMode, EngineError};
use super::perex_strings::SpanCopies;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_NULL, TAG_UNDEFINED};
use perex::binding::{BoundProgram, BoundSubject, SubjectError};
use perex::input::Position;
use perex::Budget;

/// Literal String operations also accept Perry's raw Buffer/FFI payloads.
/// An encoding error selects byte coordinates for that host operation only;
/// resource failures still propagate and regex input validation stays strict.
fn literal_subject(
    input: RuntimeHandle<'_>,
) -> Result<Option<BoundSubject<HeapSubject<'_>>>, EngineError> {
    match subject(input) {
        Ok(bound) => Ok(Some(bound)),
        Err(EngineError::Subject(SubjectError::Encoding(_))) => Ok(None),
        Err(error) => Err(error),
    }
}

fn limit(value: &RuntimeHandle<'_>) -> Result<usize, EngineError> {
    if value.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        return Ok(u32::MAX as usize);
    }
    let n = dispatch::to_number(value)?;
    Ok(if !n.is_finite() || n == 0.0 {
        0
    } else {
        n.trunc().rem_euclid(4_294_967_296.0) as usize
    })
}

fn advance(
    units: &mut Units<'_, '_>,
    index: usize,
    size: usize,
    unicode: bool,
    budget: &mut Budget,
) -> Result<usize, EngineError> {
    if unicode
        && index + 1 < size
        && (0xd800..=0xdbff).contains(&units.at(index, budget)?)
        && (0xdc00..=0xdfff).contains(&units.at(index + 1, budget)?)
    {
        return Ok(index + 2);
    }
    Ok(index + 1)
}

// Counts forward splits taken, and the work the last one charged, so tests can
// tell which path ran and how its cost scales.
#[cfg(test)]
thread_local! {
    pub(crate) static FORWARD_SPLITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static LAST_FORWARD_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The program for split's forward search, when it is admissible (#10165).
///
/// The specification tries a sticky match at every position `q`. A non-sticky
/// search from `q` returns the leftmost position `s >= q` where the pattern
/// matches, with the same match a sticky attempt at `s` finds, so the attempts
/// at `q..s` can be skipped without changing any piece or capture, and empty
/// matches and Unicode advancement line up. The skipped attempts are
/// unobservable only when nothing can see a RegExpExec happen:
/// - the splitter came from the intrinsic `RegExp` (absent or intrinsic
///   species), so it is a fresh object no user code holds, and its skipped
///   `lastIndex` writes cannot be seen;
/// - its `exec` resolves, without running a getter, to the builtin data
///   property, so the skipped `Get(exec)` calls cannot be seen either.
///
/// The program is compiled from the splitter's own internal source and flags
/// without `y`. Anything else keeps the per-position sticky loop.
fn forward_program<'s>(
    scope: &'s RuntimeHandleScope,
    constructor: Option<&RuntimeHandle<'_>>,
    splitter: &RuntimeHandle<'_>,
    budget: &mut Budget,
) -> Option<BoundProgram<GcProgram<'s>>> {
    if constructor.is_some_and(|c| {
        !crate::object::regex_proto_thunks::is_intrinsic_regexp_constructor(c.get_nanbox_f64())
    }) {
        return None;
    }
    let value = splitter.get_nanbox_f64();
    let re = crate::value::js_nanbox_get_pointer(value) as *const super::RegExpHeader;
    if !super::is_valid_regex_ptr(re)
        || !crate::object::regex_proto_thunks::regexp_view_uses_builtin(value)
    {
        return None;
    }
    let splitter = scope.root_raw_const_ptr(re);
    let program = super::perex_construct::nonsticky_program(scope, &splitter).ok()?;
    BoundProgram::new(program, budget).ok()
}

fn push_span(
    output: &mut List<'_>,
    copies: &mut SpanCopies<'_, '_>,
    start: usize,
    end: usize,
    budget: &mut Budget,
) -> Result<(), EngineError> {
    let result = copies.copy(start, end, budget)?;
    output.push(js_nanbox_string(result as i64), budget)
}

pub(crate) fn regexp(receiver: f64, argument: f64, limit_value: f64) -> Result<f64, EngineError> {
    dispatch::require_object(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let argument = scope.root_nanbox_f64(argument);
    let limit_value = scope.root_nanbox_f64(limit_value);
    let input = text(&scope, &argument)?;
    let constructor = super::match_all::species(&receiver)?.map(|v| scope.root_nanbox_f64(v));
    let flags = scope.root_nanbox_f64(dispatch::get(&receiver, b"flags")?);
    let flags = text(&scope, &flags)?;
    let mut budget = Budget::new(api::WORK);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let mut unicode = false;
    let mut sticky = false;
    {
        let bound = subject(flags)?;
        let mut units = Units::new(&bound)?;
        for index in 0..length(&flags) {
            match units.at(index, &mut budget)? {
                117 | 118 => unicode = true,
                121 => sticky = true,
                _ => (),
            }
        }
    }
    let new_flags = if sticky {
        flags
    } else {
        let y = scope.root_string_ptr(api::caught(|| {
            crate::string::js_string_from_bytes(b"y".as_ptr(), 1)
        })?);
        let mut pieces = Pieces::new(&scope)?;
        pieces.whole(&flags, &mut budget)?;
        pieces.whole(&y, &mut budget)?;
        scope.root_string_ptr(pieces.finish(&flags, Some(&y), &mut budget)?)
    };
    let splitter = scope.root_nanbox_f64(api::caught(|| match &constructor {
        None => js_nanbox_pointer(super::js_regexp_construct(
            receiver.get_nanbox_f64(),
            boxed(&new_flags),
        ) as i64),
        Some(constructor) => crate::object::construct_two_rooted(
            constructor.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
            boxed(&new_flags),
        ),
    })?);
    let mut output = List::new(&scope)?;
    let lim = limit(&limit_value)?;
    if lim == 0 {
        return Ok(output.value());
    }
    let size = length(&input);
    if size == 0 {
        if dispatch::execute(
            &splitter,
            &input,
            true,
            &mut budget,
            &memory,
            &mut host::poll,
            None,
        )?
        .is_none()
        {
            output.push(boxed(&input), &mut budget)?;
        }
        return Ok(output.value());
    }
    let bound = subject(input)?;
    let reuse = api::Reuse::new(&scope, &splitter, input, &bound, &mut budget);
    let mut units = Units::new(&bound)?;
    let mut copies = SpanCopies::new(&bound)?;
    let (mut p, mut q) = (0, 0);
    if let Some(forward) = forward_program(&scope, constructor.as_ref(), &splitter, &mut budget) {
        #[cfg(test)]
        FORWARD_SPLITS.with(|n| n.set(n.get() + 1));
        let charged = |budget: &Budget| {
            #[cfg(test)]
            LAST_FORWARD_WORK.with(|w| w.set(api::WORK - budget.remaining()));
            let _ = budget;
        };
        // Each search starts where the previous one stood, so on non-ASCII
        // storage it does not seek from an end of the subject (#10164).
        let mut near: Option<Position> = None;
        while q < size {
            let local = RuntimeHandleScope::new();
            let (found, position) = host::find_near(
                &forward,
                &bound,
                q,
                near,
                CaptureMode::All,
                &mut budget,
                &memory,
                api::QUANTUM,
                &mut host::poll,
            )?;
            near = Some(position);
            let Some(found) = found else {
                break;
            };
            let start = found.full.start();
            // The sticky loop never tries the end of the input.
            if start >= size {
                break;
            }
            let end = found.full.end().min(size);
            if end == p {
                // Only an empty match at `p` itself: step past it, as the
                // sticky loop does.
                q = advance(&mut units, start, size, unicode, &mut budget)?;
                host::poll()?;
                continue;
            }
            push_span(&mut output, &mut copies, p, start, &mut budget)?;
            if output.len() == lim {
                charged(&budget);
                return Ok(output.value());
            }
            p = end;
            let count = found.captures.as_ref().map_or(0, |captures| captures.len());
            if count > 1 {
                let (array, _) = api::caught(|| {
                    super::perex_results::materialize(
                        &input,
                        &bound,
                        &forward,
                        &found,
                        // Captures lie within this match, just behind the search's end.
                        Some(position),
                        false,
                        &mut budget,
                        &mut host::poll,
                    )
                })??;
                let array = local.root_raw_mut_ptr(array);
                for capture in 1..count {
                    let value = array.with_const_ptr::<crate::array::ArrayHeader, _>(|array| {
                        crate::array::js_array_get_f64(array, capture as u32)
                    });
                    output.push(value, &mut budget)?;
                    if output.len() == lim {
                        charged(&budget);
                        return Ok(output.value());
                    }
                }
            }
            q = p;
            host::poll()?;
        }
        push_span(&mut output, &mut copies, p, size, &mut budget)?;
        charged(&budget);
        return Ok(output.value());
    }
    while q < size {
        let local = RuntimeHandleScope::new();
        dispatch::set_last_index(&splitter, q as f64)?;
        let found = dispatch::execute(
            &splitter,
            &input,
            true,
            &mut budget,
            &memory,
            &mut host::poll,
            Some(&reuse),
        )?;
        if let Some(found) = found {
            let found = local.root_nanbox_f64(found.object());
            let end = local.root_nanbox_f64(dispatch::get(&splitter, b"lastIndex")?);
            let end = dispatch::to_length(&end)?.min(size as f64) as usize;
            if end != p {
                push_span(&mut output, &mut copies, p, q, &mut budget)?;
                if output.len() == lim {
                    return Ok(output.value());
                }
                p = end;
                let count = local.root_nanbox_f64(dispatch::get(&found, b"length")?);
                let count = dispatch::to_length(&count)?;
                let mut capture = 1usize;
                while (capture as f64) < count {
                    // Captures are arbitrary JS values. No Get(0), index,
                    // groups or ToString(capture) occurs in this operation.
                    let value = index_property(&found, capture)?;
                    output.push(value, &mut budget)?;
                    if output.len() == lim {
                        return Ok(output.value());
                    }
                    capture += 1;
                }
                q = p;
            } else {
                q = advance(&mut units, q, size, unicode, &mut budget)?;
            }
        } else {
            q = advance(&mut units, q, size, unicode, &mut budget)?;
        }
        host::poll()?;
    }
    push_span(&mut output, &mut copies, p, size, &mut budget)?;
    Ok(output.value())
}

/// Is this `limit` free of a coercion that can run user code or throw?
///
/// The plain algorithm raises by throwing where this module returns `Err`, so
/// any input whose coercion can fail is left to the engine path rather than
/// having its exception translated.
fn limit_is_plain(limit_value: &RuntimeHandle<'_>) -> bool {
    let bits = limit_value.get_nanbox_f64().to_bits();
    bits == TAG_UNDEFINED || crate::value::JSValue::from_bits(bits).is_number()
}

/// Is this separator already a string with no lone surrogate in it?
///
/// The plain algorithm scans WTF-8 bytes where the engine reads UTF-16 units,
/// so it cannot match a separator that is one half of a valid pair --
/// `"\u{1F600}\u{1F600}".split(lowHalf)` is three parts to the engine and one
/// to a byte scan. WTF-8 spells a surrogate `ED A0..BF xx`, so this test is
/// exact rather than conservative. A separator that is not already a string is
/// excluded too: its `ToString` can run user code, and a Symbol must throw.
fn separator_is_plain(
    scope: &RuntimeHandleScope,
    separator: &RuntimeHandle<'_>,
) -> Result<bool, EngineError> {
    let jv = crate::value::JSValue::from_bits(separator.get_nanbox_f64().to_bits());
    if !jv.is_string() && !jv.is_short_string() {
        return Ok(false);
    }
    // Already a string, so this coercion runs no user code; it only puts the
    // value in the one representation whose bytes can be read.
    let sep = text(scope, separator)?;
    // SAFETY: a rooted string handle; the borrow spans no allocation or call.
    Ok(unsafe {
        sep.with_string_bytes(|bytes| {
            !bytes
                .windows(2)
                .any(|w| w[0] == 0xED && (0xA0..=0xBF).contains(&w[1]))
        })
    })
}

pub(crate) fn string(receiver: f64, separator: f64, limit_value: f64) -> Result<f64, EngineError> {
    // A string separator on a string receiver has no `@@split` and no
    // coercion that can run user code or throw: answer it before any of the
    // setup below, which is most of the cost of a short split (#10519).
    if let Some(parts) = crate::string::split_string_by_string(receiver, separator, limit_value) {
        return Ok(parts);
    }
    if matches!(receiver.to_bits(), TAG_NULL | TAG_UNDEFINED) {
        return Err(EngineError::Type(
            "String.split requires a non-null receiver",
        ));
    }
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let separator = scope.root_nanbox_f64(separator);
    let limit_value = scope.root_nanbox_f64(limit_value);
    let mut budget = Budget::new(api::WORK);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    if crate::proxy::reflect_value_is_object(separator.get_nanbox_f64()) {
        let method = scope.root_nanbox_f64(dispatch::get_symbol(&separator, "split")?);
        if !matches!(method.get_nanbox_f64().to_bits(), TAG_NULL | TAG_UNDEFINED) {
            if !callable(&method)? {
                return Err(EngineError::Type("Symbol.split is not callable"));
            }
            let mut args = List::new(&scope)?;
            args.push(receiver.get_nanbox_f64(), &mut budget)?;
            args.push(limit_value.get_nanbox_f64(), &mut budget)?;
            return call(&method, &separator, &args, &memory);
        }
    }
    // No `@@split`, so the plain string algorithm applies and the engine has
    // nothing to contribute. Hand it to the implementation a build without the
    // engine uses.
    //
    // Linking `regex-engine` replaces `String.prototype.split` with this module
    // wholesale, so a program using a regex *anywhere* ran every split through
    // the engine's per-unit subject reader: 35,826 instructions for
    // `"alpha beta gamma delta eps0".split(" ")` against 3,662 without the
    // engine, and 2,729 in Node 26.5.1. Both arms auto-optimized, so that is the
    // implementation swap rather than the build mode.
    //
    // The plain algorithm agrees with Node on 27 cases where a byte scan and a
    // UTF-16 unit scan can disagree -- empty separator, separator longer than
    // the subject, every `limit` form, lone surrogates, an astral pair split by
    // units, a separator that is a prefix of itself at the tail -- and on every
    // non-string separator form. The one thing it does not implement is
    // `@@split`, which is why this sits below that check.
    // The two implementations report failure differently: this one returns
    // `Err(EngineError)` for `api::finish` to raise at the ABI boundary, while
    // the plain algorithm throws directly (its own boundary is the ABI). A
    // coercion that throws -- `ToNumber` on a BigInt `limit`, say -- would
    // otherwise escape as an uncaught exception, so the throw is captured here
    // and re-raised by `finish` like any other engine error.
    if limit_is_plain(&limit_value) && separator_is_plain(&scope, &separator)? {
        // The plain algorithm reports failure by throwing, where this one
        // returns `Err` for `api::finish` to raise; `delegable` has already
        // excluded every input whose coercion can throw, so nothing escapes.
        return api::caught(|| {
            crate::string::js_string_split_plain(
                receiver.get_nanbox_f64(),
                separator.get_nanbox_f64(),
                limit_value.get_nanbox_f64(),
            )
        });
    }
    string_via_engine(
        receiver.get_nanbox_f64(),
        separator.get_nanbox_f64(),
        limit_value.get_nanbox_f64(),
    )
}

/// The engine's split, for inputs `delegable` excludes.
fn string_via_engine(receiver: f64, separator: f64, limit_value: f64) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let separator = scope.root_nanbox_f64(separator);
    let limit_value = scope.root_nanbox_f64(limit_value);
    let mut budget = Budget::new(api::WORK);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    let input = text(&scope, &receiver)?;
    let lim = limit(&limit_value)?;
    let needle = text(&scope, &separator)?;
    let mut output = List::new(&scope)?;
    if lim == 0 {
        return Ok(output.value());
    }
    if separator.get_nanbox_f64().to_bits() == TAG_UNDEFINED {
        output.push(boxed(&input), &mut budget)?;
        return Ok(output.value());
    }
    let bound = literal_subject(input)?;
    let pattern = literal_subject(needle)?;
    let (Some(bound), Some(pattern)) = (bound, pattern) else {
        super::perex_literal_bytes::split(&input, &needle, lim, &mut output, &mut budget, &memory)?;
        return Ok(output.value());
    };
    let mut copies = SpanCopies::new(&bound)?;
    let n = bound
        .with_view(|s| s.len_utf16())
        .map_err(EngineError::Subject)?;
    let m = pattern
        .with_view(|s| s.len_utf16())
        .map_err(EngineError::Subject)?;
    if m == 0 {
        for i in 0..n.min(lim) {
            push_span(&mut output, &mut copies, i, i + 1, &mut budget)?;
        }
        return Ok(output.value());
    }
    let mut end = 0;
    super::perex_literal_search::each_bound(
        &bound,
        &pattern,
        &mut budget,
        &memory,
        |position, budget| {
            push_span(&mut output, &mut copies, end, position, budget)?;
            end = position + m;
            Ok(output.len() == lim)
        },
    )?;
    if output.len() < lim {
        push_span(&mut output, &mut copies, end, n, &mut budget)?;
    }
    Ok(output.value())
}

pub(crate) extern "C" fn regexp_thunk(
    _: *const crate::closure::ClosureHeader,
    input: f64,
    limit: f64,
) -> f64 {
    api::finish(regexp(crate::object::js_implicit_this_get(), input, limit))
}

#[no_mangle]
pub extern "C" fn js_string_split_js(receiver: f64, separator: f64, limit: f64) -> f64 {
    api::finish(string(receiver, separator, limit))
}
