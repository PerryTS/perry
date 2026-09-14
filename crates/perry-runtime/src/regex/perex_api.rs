//! Perry's public RegExp execution boundary. JS throws are caught below native
//! owners and rethrown only after those owners have been released normally.
use super::perex_memory::{MemoryBudget, StorageError};
use super::perex_owner::{BuildError, GcProgram, HeapSubject};
use super::perex_runtime::{self as host, CaptureMode, EngineError};
use super::RegExpHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use perex::binding::{BoundProgram, BoundSubject};
use perex::compiler::CompileError;
use perex::executor::ExecError;
use perex::input::Position;
use perex::{span::Span, Budget};

// One explicit host policy; no retained scratch cache or alternate engine.
/// A RegExp operation's work allowance: effectively unlimited (#10164).
///
/// JavaScript engines never abort regex matching for doing too much work, and
/// no finite allowance separates valid programs from pathological ones: Perex's
/// charge per subject unit depends on the program (about 1 for `/x/`, 60 for
/// `/([a-z]+)([0-9]+)/g`, over 200 for a 32-unit lookahead), so any cap throws
/// on some large linear input Node completes. The former 100,000,000 did, as
/// `RangeError: Regular expression work limit exceeded`. Searches still run in
/// `QUANTUM` slices with a GC poll between them, so collection and cancellation
/// keep working; a catastrophic pattern runs as long as it does in Node. The
/// memory limits below are unchanged.
pub(crate) const WORK: usize = usize::MAX;
pub(crate) const SCRATCH_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const PROGRAM_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const QUANTUM: usize = 4096;
pub(crate) const OUTPUT_BYTES: usize = crate::string::MAX_STRING_LENGTH * 3;

/// Capture by reference when `f` can throw: its Rust frame can be abandoned by
/// longjmp. Native ownership belongs in the caller, above this local trap.
pub(crate) fn caught<T>(f: impl FnOnce() -> T) -> Result<T, EngineError> {
    crate::exception::catch_js_throw(f).map_err(EngineError::Abrupt)
}

pub(crate) fn finish<T>(result: Result<T, EngineError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => raise(error),
    }
}

fn raise(error: EngineError) -> ! {
    // No allocation/poll precedes rethrowing the returned exception bits.
    if let EngineError::Abrupt(value) = error {
        crate::exception::js_throw(value);
    }
    if let EngineError::Build(BuildError::Abrupt(bits))
    | EngineError::Storage(StorageError::Abrupt(bits)) = error
    {
        crate::exception::js_throw(f64::from_bits(bits));
    }
    let type_error = matches!(error, EngineError::Type(_));
    let (message, syntax) = match error {
        EngineError::Type(message) => (message, false),
        EngineError::InvalidFlags => ("Invalid flags supplied to RegExp constructor", true),
        EngineError::Compile(CompileError::Syntax { .. }) => ("Invalid regular expression", true),
        EngineError::Compile(CompileError::Unsupported { feature, .. }) => (feature, true),
        EngineError::Compile(CompileError::WorkLimit)
        | EngineError::Build(BuildError::Compile(CompileError::WorkLimit))
        | EngineError::Execution(ExecError::WorkLimit) => {
            ("Regular expression work limit exceeded", false)
        }
        EngineError::Storage(StorageError::Limit | StorageError::Allocation)
        | EngineError::Build(BuildError::SizeLimit | BuildError::Allocation) => {
            ("Regular expression memory limit exceeded", false)
        }
        EngineError::Cancelled => ("Regular expression operation cancelled", false),
        EngineError::Subject(error) => {
            // Preserve a resource/encoding failure as an error, never no-match.
            let _ = error;
            ("Invalid regular expression input storage", false)
        }
        EngineError::Program(error) => {
            let _ = error;
            ("Invalid regular expression program storage", false)
        }
        _ => ("Regular expression execution failed", false),
    };
    let scope = RuntimeHandleScope::new();
    let text = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let text = scope.root_string_ptr(text);
    let exception = text.with_mut_ptr::<StringHeader, _>(|text| {
        if type_error {
            crate::error::js_typeerror_new(text)
        } else if syntax {
            crate::error::js_syntaxerror_new(text)
        } else {
            crate::error::js_rangeerror_new(text)
        }
    });
    crate::exception::js_throw(crate::value::js_nanbox_pointer(exception as i64))
}

/// Construction and reinitialization publish the program. Each operation owns
/// an independent root so reentrant compilation cannot change its matcher.
pub(crate) fn program<'s>(
    scope: &'s RuntimeHandleScope,
    receiver: &RuntimeHandle<'_>,
    budget: &mut Budget,
    _memory: &MemoryBudget,
    _poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<BoundProgram<GcProgram<'s>>, EngineError> {
    let owner = unsafe { GcProgram::from_receiver(scope, receiver) }
        .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?;
    bind_program(owner, budget)
}

/// Bind a program, in constant work when a binding validated this same cell
/// before (#10166): the witness lives beside the words in the program cell, so
/// it cannot describe other words. A witness that does not match falls back to
/// validation, which records a fresh one. No allocation and nothing traced.
pub(crate) fn bind_program<'s>(
    owner: GcProgram<'s>,
    budget: &mut Budget,
) -> Result<BoundProgram<GcProgram<'s>>, EngineError> {
    let root = owner.root();
    let owner = match owner.witness() {
        Some(witness) => match BoundProgram::new_witnessed(owner, witness) {
            Ok(bound) => return Ok(bound),
            Err(failed) => failed.storage,
        },
        None => owner,
    };
    let bound = BoundProgram::new(owner, budget).map_err(|e| EngineError::Program(e.error))?;
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.perex_validations += 1);
    }
    GcProgram::record_witness(&root, bound.witness());
    Ok(bound)
}

/// Bind a whole heap string, in constant work when this header was validated
/// before (#10166). The first binding decodes it; if that succeeds and its
/// UTF-16 length matches the header, the header is marked
/// `STRING_FLAG_WTF8_VALIDATED` and later bindings use `new_counted`.
///
/// Perry strings are not all valid WTF-8 (raw Buffer and FFI payloads reach
/// here too), so validity is never assumed: a string that fails to validate is
/// never marked and keeps failing exactly as before. `HeapSubject::new` has
/// already marked the header shared, so a marked payload is never mutated in
/// place, and the mark is never copied to another string (see the flag).
pub(crate) fn bind_heap_subject(
    input: RuntimeHandle<'_>,
) -> Result<BoundSubject<HeapSubject<'_>>, EngineError> {
    bind_heap_subject_observed(input).map(|(bound, _)| bound)
}

/// [`bind_heap_subject`], also returning the string's cross-call identity when
/// it is non-ASCII (#10164), read from the same header access.
pub(crate) fn bind_heap_subject_observed(
    input: RuntimeHandle<'_>,
) -> Result<
    (
        BoundSubject<HeapSubject<'_>>,
        Option<super::perex_position_hint::StringIdentity>,
    ),
    EngineError,
> {
    use crate::string::STRING_FLAG_WTF8_VALIDATED;
    let (utf16_len, validated, identity) = input.with_const_ptr::<StringHeader, _>(|s| unsafe {
        (
            (*s).utf16_len as usize,
            (*s).flags & STRING_FLAG_WTF8_VALIDATED != 0,
            super::perex_position_hint::identity_of_header(s),
        )
    });
    let owner = unsafe { HeapSubject::new(input) }
        .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?;
    let owner = if validated {
        match BoundSubject::new_counted(owner, utf16_len) {
            Ok(bound) => return Ok((bound, identity)),
            Err(failed) => failed.storage,
        }
    } else {
        owner
    };
    let bound = BoundSubject::new(owner).map_err(|e| EngineError::Subject(e.error))?;
    let decoded = bound
        .with_view(|view| view.len_utf16())
        .map_err(EngineError::Subject)?;
    // An empty string has nothing to decode. `HeapSubject::new` already wrote
    // this header's refcount, so it is writable.
    if decoded == utf16_len && utf16_len > 0 {
        input.with_const_ptr::<StringHeader, _>(|s| unsafe {
            (*(s as *mut StringHeader)).flags |= STRING_FLAG_WTF8_VALIDATED;
        });
    }
    Ok((bound, identity))
}

/// Bindings one compound operation reuses across its searches (#10165).
///
/// Split, replace and global match run many searches over one string with one
/// matcher. Binding per search decodes the entire subject and revalidates the
/// entire program every time, which made those loops quadratic in the input.
/// Perex's binding contract lets a binding outlive allocation, collection and
/// JS callbacks: both owners hold registered roots and reacquire their base on
/// every view, so no search needs to rebind because the collector moved them.
///
/// Build it before the operation's loop. Runtime handle scopes are a stack, so
/// its roots must sit below every per-iteration scope; nothing here roots
/// lazily. A search uses a binding only while it is provably the same object:
/// the same string, and the same receiver still holding the same program cell.
/// Anything else (an `exec` override, a recompiled receiver, another string)
/// binds afresh for that search exactly as before.
///
/// `near` is where the previous search over the reused subject stood (#10164),
/// so the next search seeks from there instead of from an end of the subject.
/// It is only ever set from, and only ever used with, the reused binding.
pub(crate) struct Reuse<'b, 's> {
    input: RuntimeHandle<'s>,
    subject: &'b BoundSubject<HeapSubject<'s>>,
    program: Option<ReusedProgram<'s>>,
    near: std::cell::Cell<Option<Position>>,
}

struct ReusedProgram<'s> {
    receiver: RuntimeHandle<'s>,
    cell: RuntimeHandle<'s>,
    bound: BoundProgram<GcProgram<'s>>,
}

impl<'b, 's> Reuse<'b, 's> {
    /// `subject` must bind the whole of `input` (not a window), as the
    /// operations' own `subject(input)` bindings do.
    pub(crate) fn new(
        scope: &'s RuntimeHandleScope,
        receiver: &RuntimeHandle<'_>,
        input: RuntimeHandle<'s>,
        subject: &'b BoundSubject<HeapSubject<'s>>,
        budget: &mut Budget,
    ) -> Self {
        let re =
            crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *const RegExpHeader;
        // A receiver that is not a RegExp with a published program runs no
        // builtin search here; its failure belongs to the ordinary path.
        let program = (super::is_valid_regex_ptr(re) && unsafe { !(*re).perex_program.is_null() })
            .then(|| {
                // Rooting pushes a handle slot and never collects, so `re` and
                // its program edge are still current for every read below.
                let receiver = scope.root_raw_const_ptr(re);
                let cell = scope.root_raw_const_ptr(unsafe { (*re).perex_program });
                let owner = unsafe { GcProgram::from_receiver(scope, &receiver) }.ok()?;
                let bound = bind_program(owner, budget).ok()?;
                Some(ReusedProgram {
                    receiver,
                    cell,
                    bound,
                })
            })
            .flatten();
        Self {
            input,
            subject,
            program,
            near: std::cell::Cell::new(None),
        }
    }

    /// Where the last search over the reused subject stood, if any.
    pub(crate) fn near(&self) -> Option<Position> {
        self.near.get()
    }

    fn subject_for(&self, input: &RuntimeHandle<'_>) -> Option<&BoundSubject<HeapSubject<'s>>> {
        let current = input.with_const_ptr::<StringHeader, _>(|p| p);
        let bound = self.input.with_const_ptr::<StringHeader, _>(|p| p);
        (current == bound).then_some(self.subject)
    }

    /// Both roots are live, so equal addresses name the same objects even after
    /// either moved; a replaced program cannot reuse a cell this root retains.
    /// How many named groups the program of the RegExp at `receiver` declares,
    /// when this binding is for that receiver's current program.
    pub(crate) fn name_count(&self, receiver: *const RegExpHeader) -> Option<usize> {
        let reused = self.program.as_ref()?;
        let bound = reused.receiver.with_const_ptr::<RegExpHeader, _>(|p| p);
        let cell = reused.cell.with_const_ptr::<u8, _>(|p| p);
        (receiver == bound && unsafe { (*receiver).perex_program } == cell)
            .then(|| reused.bound.with_view(|program| program.name_count()).ok())
            .flatten()
    }

    fn program_for(&self, receiver: &RuntimeHandle<'_>) -> Option<&BoundProgram<GcProgram<'s>>> {
        let reused = self.program.as_ref()?;
        let current = receiver.with_const_ptr::<RegExpHeader, _>(|p| p);
        let bound = reused.receiver.with_const_ptr::<RegExpHeader, _>(|p| p);
        let cell = reused.cell.with_const_ptr::<u8, _>(|p| p);
        (current == bound && unsafe { (*current).perex_program } == cell).then_some(&reused.bound)
    }
}

pub(crate) struct ExecMatch {
    pub(crate) full: Span,
    pub(crate) array: *mut crate::array::ArrayHeader,
    pub(crate) groups: *mut crate::object::ObjectHeader,
}

/// Canonical, non-stateful test on a segment-local window of original storage.
/// Admission is non-observable; declined cases run the ordinary JS operation.
pub(crate) fn test_window(
    receiver: f64,
    input: *const StringHeader,
    start: usize,
    end: usize,
) -> Result<Option<bool>, EngineError> {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let input = scope.root_string_ptr(input);
    let re = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *const RegExpHeader;
    if !super::is_valid_regex_ptr(re) {
        return Ok(None);
    }
    let admitted = unsafe {
        !(*re).global && !(*re).sticky
            // Even non-stateful builtin exec performs ToLength(lastIndex).
            // Only an already-Number permits omitting that observable step.
            && crate::value::JSValue::from_bits((*re).last_index).is_number()
    };
    if !admitted
        || !crate::object::regex_proto_thunks::regexp_view_uses_builtin(receiver.get_nanbox_f64())
    {
        return Ok(None);
    }
    let raw_receiver = scope.root_raw_const_ptr(re);
    let mut budget = Budget::new(WORK);
    let memory = MemoryBudget::new(SCRATCH_BYTES);
    let program = program(&scope, &raw_receiver, &mut budget, &memory, &mut host::poll)?;
    let owner = unsafe { HeapSubject::window(input, start, end) }
        .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?;
    let subject = BoundSubject::new(owner).map_err(|e| EngineError::Subject(e.error))?;
    host::find(
        &program,
        &subject,
        0,
        CaptureMode::Full,
        &mut budget,
        &memory,
        QUANTUM,
        &mut host::poll,
    )
    .map(|found| Some(found.is_some()))
}

/// Materialize a final JavaScript segment from the same bounded original
/// owner. No source borrow crosses allocation or a collecting safepoint.
pub(crate) fn copy_window(
    input: *const StringHeader,
    start: usize,
    end: usize,
    units: usize,
) -> Result<*mut StringHeader, EngineError> {
    let scope = RuntimeHandleScope::new();
    let input = scope.root_string_ptr(input);
    let owner = unsafe { HeapSubject::window(input, start, end) }
        .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?;
    let subject = BoundSubject::new(owner).map_err(|e| EngineError::Subject(e.error))?;
    super::perex_strings::copy_span(
        &subject,
        Span::new(0, units).ok_or(EngineError::InvalidSpan)?,
        &mut Budget::new(WORK),
        OUTPUT_BYTES,
        QUANTUM,
        &mut host::poll,
    )
}

/// RegExpBuiltinExec, with a fresh allowance for a standalone operation.
pub(crate) fn execute(
    receiver: *mut RegExpHeader,
    input: *const StringHeader,
    materialize: bool,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Option<ExecMatch>, EngineError> {
    execute_with_resources(
        receiver,
        input,
        materialize,
        &mut Budget::new(WORK),
        &MemoryBudget::new(SCRATCH_BYTES),
        poll,
        None,
    )
}

/// Compound String operations keep one allowance across successive matches.
/// Each execution has its own root scope, so a global loop cannot retain a
/// root for every previous result. `reuse` carries the operation's bindings.
pub(crate) fn execute_with_resources(
    receiver: *mut RegExpHeader,
    input: *const StringHeader,
    materialize: bool,
    budget: &mut Budget,
    memory: &MemoryBudget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
    reuse: Option<&Reuse<'_, '_>>,
) -> Result<Option<ExecMatch>, EngineError> {
    let output = if materialize {
        ExecOutput::Object
    } else {
        ExecOutput::Test
    };
    execute_output(receiver, input, output, budget, memory, poll, reuse)
}

/// What a builtin search produces when it matches.
pub(crate) enum ExecOutput<'v> {
    /// Only the full match (`test`).
    Test,
    /// The exec result array and its groups object.
    Object,
    /// Every capture span, group zero first, appended to the vector as
    /// UTF-16 `start, end` pairs, with `u32::MAX, u32::MAX` for an unset
    /// group. No JS object is created (#10165).
    Spans(&'v mut Vec<u32>),
}

/// [`execute_with_resources`] with an explicit output.
pub(crate) fn execute_output(
    receiver: *mut RegExpHeader,
    input: *const StringHeader,
    output: ExecOutput<'_>,
    budget: &mut Budget,
    memory: &MemoryBudget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
    reuse: Option<&Reuse<'_, '_>>,
) -> Result<Option<ExecMatch>, EngineError> {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(receiver);
    let input = scope.root_string_ptr(input);
    let stored = receiver.with_const_ptr::<RegExpHeader, _>(|r| unsafe {
        crate::value::JSValue::from_bits((*r).last_index)
    });
    let last_index = if stored.is_number() {
        stored
            .as_number()
            .max(0.0)
            .floor()
            .min(9_007_199_254_740_991.0) as usize
    } else {
        caught(|| receiver.with_const_ptr(|p| super::regex_last_index_offset(p)))?
    };
    let (stateful, has_indices) = receiver.with_const_ptr::<RegExpHeader, _>(|r| unsafe {
        ((*r).global || (*r).sticky, (*r).has_indices)
    });
    let start = if stateful { last_index } else { 0 };
    let length = input.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len as usize });
    if start > length {
        if stateful {
            caught(|| {
                receiver.with_mut_ptr::<RegExpHeader, _>(|re| super::set_last_index_throwing(re, 0))
            })?;
        }
        return Ok(None);
    }
    let fresh_program;
    let program = match reuse.and_then(|reuse| reuse.program_for(&receiver)) {
        Some(program) => program,
        None => {
            fresh_program = program(&scope, &receiver, budget, memory, poll)?;
            &fresh_program
        }
    };
    let fresh_subject;
    let reused_subject = reuse.and_then(|reuse| reuse.subject_for(&input));
    // A position from this operation's own binding, or else from the previous
    // call's search on this same, unchanged string (#10164). Only a non-ASCII
    // string has an identity; its lengths cannot change during the search.
    let mut cross_call = None;
    let subject = match reused_subject {
        Some(subject) => subject,
        None => {
            let (bound, identity) = bind_heap_subject_observed(input)?;
            fresh_subject = bound;
            cross_call = identity;
            &fresh_subject
        }
    };
    let near = match reused_subject {
        Some(_) => reuse.and_then(|reuse| reuse.near()),
        None => cross_call.and_then(super::perex_position_hint::lookup),
    };
    let (found, position) = host::find_near(
        program,
        subject,
        start,
        near,
        if matches!(output, ExecOutput::Test) {
            CaptureMode::Full
        } else {
            CaptureMode::All
        },
        budget,
        memory,
        QUANTUM,
        poll,
    )?;
    if let (Some(reuse), Some(_)) = (reuse, reused_subject) {
        reuse.near.set(Some(position));
    } else if cross_call.is_some() {
        // Re-read after the search: a collection during it may have moved the
        // string, and the identity must be the one the next call will see.
        if let Some(identity) = super::perex_position_hint::identity_of(&input) {
            super::perex_position_hint::record(identity, position);
        }
    }
    if stateful {
        let next = found.as_ref().map_or(0, |m| m.full.end());
        caught(|| {
            receiver.with_mut_ptr::<RegExpHeader, _>(|re| super::set_last_index_throwing(re, next))
        })?;
    }
    let Some(found) = found else {
        return Ok(None);
    };
    let (array, groups) = match output {
        // Captures and all native owners remain ABOVE the JS trap. A thrown
        // allocation/property operation returns here before they are dropped.
        ExecOutput::Object => caught(|| {
            super::perex_results::materialize(
                &input,
                subject,
                program,
                &found,
                // From the search that just ran over this same binding.
                Some(position),
                has_indices,
                budget,
                poll,
            )
        })??,
        ExecOutput::Spans(spans) => {
            let captures = found.captures.as_ref().ok_or(EngineError::InvalidSpan)?;
            spans
                .try_reserve(captures.len() * 2)
                .map_err(|_| StorageError::Allocation)?;
            for capture in captures.iter() {
                let (start, end) = match capture {
                    Some(span) => (
                        u32::try_from(span.start()).map_err(|_| StorageError::Limit)?,
                        u32::try_from(span.end()).map_err(|_| StorageError::Limit)?,
                    ),
                    None => (u32::MAX, u32::MAX),
                };
                spans.push(start);
                spans.push(end);
            }
            (std::ptr::null_mut(), std::ptr::null_mut())
        }
        ExecOutput::Test => (std::ptr::null_mut(), std::ptr::null_mut()),
    };
    Ok(Some(ExecMatch {
        full: found.full,
        array,
        groups,
    }))
}
