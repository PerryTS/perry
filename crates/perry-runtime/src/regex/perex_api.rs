//! Perry's public RegExp execution boundary. JS throws are caught below native
//! owners and rethrown only after those owners have been released normally.
use super::perex_memory::{MemoryBudget, StorageError};
use super::perex_owner::{BuildError, GcProgram, HeapSubject};
use super::perex_runtime::{self as host, CaptureMode, EngineError, Match};
use super::RegExpHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use perex::binding::{
    BoundProgram, BoundSubject, ImmutableProgram, ImmutableSubject, ProgramWitness,
};
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

// The allocation-point root witness arms pressure after the search's
// mandatory capture poll. A native function pointer is not a managed edge.
#[cfg(test)]
thread_local! {
    static BEFORE_RESULT_ALLOC: std::cell::Cell<Option<fn()>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_before_result_alloc(hook: Option<fn()>) -> Option<fn()> {
    BEFORE_RESULT_ALLOC.with(|slot| slot.replace(hook))
}

#[cfg(test)]
pub(super) fn before_result_alloc() {
    if let Some(hook) = BEFORE_RESULT_ALLOC.with(|slot| slot.take()) {
        hook();
    }
}

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
    if let EngineError::Build(BuildError::Abrupt(bits)) = error {
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
    let witness = owner.witness();
    bind_witnessed(owner, witness, budget, |witness| {
        GcProgram::record_witness(&root, witness)
    })
}

/// [`bind_program`] for any view of a program cell: `witness` is the one the
/// cell holds, and `record` stores a fresh one in the same cell.
#[inline]
pub(crate) fn bind_witnessed<P: ImmutableProgram>(
    storage: P,
    witness: Option<ProgramWitness>,
    budget: &mut Budget,
    record: impl FnOnce(ProgramWitness),
) -> Result<BoundProgram<P>, EngineError>
where
    P::Error: super::perex_owner::HostResourceError,
{
    let storage = match witness {
        Some(witness) => match BoundProgram::new_witnessed(storage, witness) {
            Ok(bound) => return Ok(bound),
            Err(failed) => failed.storage,
        },
        None => storage,
    };
    let bound = BoundProgram::new(storage, budget)
        .map_err(|e| EngineError::Program(host::program_error(e.error)))?;
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.perex_validations += 1);
    }
    record(bound.witness());
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
    // `HeapSubject::new` already wrote this header's refcount, so it is writable.
    let bound = bind_counted(owner, utf16_len, validated, || {
        input.with_const_ptr::<StringHeader, _>(|s| unsafe {
            (*(s as *mut StringHeader)).flags |= STRING_FLAG_WTF8_VALIDATED;
        })
    })?;
    Ok((bound, identity))
}

/// Bind a whole string's storage of `utf16_len` units: in constant work when
/// the header carries `STRING_FLAG_WTF8_VALIDATED` (`validated`), otherwise by
/// decoding it, after which `mark` sets the flag if the decode found exactly
/// `utf16_len` units. `mark` runs with no collecting action since the storage
/// was read, so it may write through the same header.
#[inline]
pub(crate) fn bind_counted<S: ImmutableSubject>(
    storage: S,
    utf16_len: usize,
    validated: bool,
    mark: impl FnOnce(),
) -> Result<BoundSubject<S>, EngineError>
where
    S::Error: super::perex_owner::HostResourceError,
{
    let storage = if validated {
        match BoundSubject::new_counted(storage, utf16_len) {
            Ok(bound) => return Ok(bound),
            Err(failed) => failed.storage,
        }
    } else {
        storage
    };
    let bound = BoundSubject::new(storage)
        .map_err(|e| EngineError::Subject(host::subject_error(e.error)))?;
    let decoded = bound
        .with_view(|view| view.len_utf16())
        .map_err(|e| EngineError::Subject(host::subject_error(e)))?;
    // An empty string has nothing to decode.
    if decoded == utf16_len && utf16_len > 0 {
        mark();
    }
    Ok(bound)
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
        let program = crate::regex::regexp_data_of(receiver.get_nanbox_f64()).and_then(|data| {
            if unsafe { (*data).perex_program.is_null() } {
                return None;
            }
            // These roots only push handle slots; no collection or callback
            // occurs between the matcher read and establishing program ownership.
            let receiver = scope.root_raw_const_ptr(re);
            let owner = unsafe { GcProgram::from_data(scope, data) }.ok()?;
            let cell = owner.root();
            let bound = bind_program(owner, budget).ok()?;
            Some(ReusedProgram {
                receiver,
                cell,
                bound,
            })
        });
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
        (receiver == bound
            && unsafe { (*crate::regex::regexp_data_ptr(receiver)).perex_program } == cell)
            .then(|| reused.bound.with_view(|program| program.name_count()).ok())
            .flatten()
    }

    fn program_for(&self, current: *const RegExpHeader) -> Option<&BoundProgram<GcProgram<'s>>> {
        let reused = self.program.as_ref()?;
        let bound = reused.receiver.with_const_ptr::<RegExpHeader, _>(|p| p);
        let cell = reused.cell.with_const_ptr::<u8, _>(|p| p);
        (current == bound
            && unsafe { (*crate::regex::regexp_data_ptr(current)).perex_program } == cell)
            .then_some(&reused.bound)
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
    // Both Gets a segments-view test skips: `test` and `exec` are the
    // builtins. The exec proof also brands the receiver and hands back its data.
    if !crate::object::regex_read_sites::test(receiver.get_nanbox_f64()) {
        return Ok(None);
    }
    let Some(data) = crate::object::regex_read_sites::builtin_exec_data(receiver.get_nanbox_f64())
    else {
        return Ok(None);
    };
    let data = scope.root_raw_const_ptr(data);
    let admitted = data.with_const_ptr::<super::RegExpData, _>(|data| unsafe {
        !(*data).global && !(*data).sticky
            // Even non-stateful builtin exec performs ToLength(lastIndex).
            // Only an already-Number permits omitting that observable step.
            && crate::value::JSValue::from_bits(crate::regex::get_last_index(re).to_bits()).is_number()
    });
    if !admitted {
        return Ok(None);
    }
    let mut budget = Budget::new(WORK);
    let memory = MemoryBudget::new(SCRATCH_BYTES);
    let owner = data
        .with_const_ptr::<super::RegExpData, _>(|data| unsafe {
            GcProgram::from_data(&scope, data)
        })
        .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?;
    let program = bind_program(owner, &mut budget)?;
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
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(receiver as i64));
    let input = scope.root_string_ptr(input);
    execute_rooted(&receiver, &input, output, budget, memory, poll, reuse)
}

/// The RegExp a NaN-boxed receiver handle holds now. Read it again after any
/// collecting action.
#[inline]
pub(crate) fn regexp(receiver: &RuntimeHandle<'_>) -> *mut RegExpHeader {
    // A RegExp is an object: its NaN-box always carries the pointer tag, so
    // the address is the payload (debug builds check the tag).
    crate::value::JSValue::from_bits(receiver.get_nanbox_u64())
        .as_pointer::<RegExpHeader>()
        .cast_mut()
}

/// RegExpBuiltinExec on a RegExp and a string the caller has rooted: the
/// receiver NaN-boxed (`root_nanbox_f64`, as RegExpExec roots it), the string
/// with `root_string_ptr`. The receiver must be a valid RegExp.
///
/// The search is [`search_builtin`]'s, over the current addresses of the two
/// roots, unless `reuse` holds bindings of both. Materialising an exec result
/// reads through the roots again afterwards, under the only other JS trap.
pub(crate) fn execute_rooted(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    output: ExecOutput<'_>,
    budget: &mut Budget,
    memory: &MemoryBudget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
    reuse: Option<&Reuse<'_, '_>>,
) -> Result<Option<ExecMatch>, EngineError> {
    let has_indices = unsafe { (*super::regexp_data_ptr(regexp(receiver))).has_indices };
    let mode = if matches!(output, ExecOutput::Test) {
        CaptureMode::Full
    } else {
        CaptureMode::All
    };
    let stateful = unsafe {
        let data = &*super::regexp_data_ptr(regexp(receiver));
        data.global || data.sticky
    };
    let Some(start) = exec_start(
        regexp(receiver),
        input.with_const_ptr::<StringHeader, _>(|s| s),
        stateful,
    )?
    .map(Start::at) else {
        return Ok(None);
    };
    let mut captures = None;
    // Bindings this operation already holds for this receiver's current
    // program and this same string, if any. Looked up after `lastIndex`,
    // whose `valueOf` may have recompiled the receiver.
    let reused = reuse.and_then(|reuse| {
        Some((
            reuse,
            reuse.program_for(regexp(receiver))?,
            reuse.subject_for(input)?,
        ))
    });
    let found = match reused {
        Some((reuse, program, subject)) => {
            let found = host::find_near_into(
                program,
                subject,
                start,
                reuse.near(),
                mode,
                budget,
                memory,
                QUANTUM,
                &mut captures,
                poll,
            )?;
            reuse.near.set(Some(found.1));
            if stateful {
                store_last_index(regexp(receiver), found.0.map_or(0, |full| full.end()))?;
            }
            found.0.map(|full| (full, found.1))
        }
        None => search_from(
            regexp(receiver),
            unsafe { (*super::regexp_data_ptr(regexp(receiver))).perex_program },
            input.with_const_ptr::<StringHeader, _>(|s| s),
            start,
            stateful,
            mode,
            budget,
            memory,
            &mut captures,
            poll,
        )?,
    };
    let Some((full, position)) = found else {
        return Ok(None);
    };
    let (array, groups) = match output {
        ExecOutput::Object => {
            let found = Match { full, captures };
            let scope = RuntimeHandleScope::new();
            let fresh;
            let (bound_subject, bound_program) = match reused {
                Some((_, program, subject)) => (subject, program),
                None => {
                    // The search read the string in place; materialisation
                    // allocates, so it reads through rooted owners. The
                    // position stays valid: these bind the same, unchanged
                    // string, and nothing has run since the search.
                    let owner = unsafe { GcProgram::from_regexp(&scope, regexp(receiver)) }
                        .map_err(|e| {
                            EngineError::Subject(perex::binding::SubjectError::Resource(e))
                        })?;
                    fresh = (bind_heap_subject(*input)?, bind_program(owner, budget)?);
                    (&fresh.0, &fresh.1)
                }
            };
            // Captures and all native owners remain ABOVE the JS trap. A
            // thrown allocation/property operation returns here before they
            // are dropped.
            caught(|| {
                super::perex_results::materialize(
                    input,
                    bound_subject,
                    bound_program,
                    &found,
                    Some(position),
                    has_indices,
                    budget,
                    poll,
                )
            })??
        }
        ExecOutput::Spans(spans) => {
            let captures = captures.as_ref().ok_or(EngineError::InvalidSpan)?;
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
        full,
        array,
        groups,
    }))
}

/// RegExpBuiltinExec steps 4-15 (and 18 for a match) without an exec result:
/// the start from `lastIndex`, one search of the RegExp's program over the
/// string's own bytes, and the `lastIndex` update of a g/y RegExp. Returns the
/// full match and where the search stood, or `None` for no match.
///
/// `re` is a branded RegExp whose immutable data is `data`, and `input` a heap
/// string, all at their current addresses: the caller runs nothing that can
/// collect between reading them and this call. Nothing here roots, copies or
/// marks anything unless a non-Number `lastIndex` runs user code or the
/// search polls (`perex_owner::InPlace`), so a short `test` holds no handle at
/// all. Any address the caller kept is stale afterwards. No JS trap is set
/// unless user code can run (that `lastIndex`); a non-writable `lastIndex` is
/// returned as a TypeError rather than thrown, so owners above unwind
/// normally.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn search_builtin<'mem>(
    re: *mut RegExpHeader,
    data: *const super::RegExpData,
    input: *const StringHeader,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    captures: &mut Option<host::Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Option<(Span, Position)>, EngineError> {
    let stateful = unsafe { (*data).global || (*data).sticky };
    let Some(start) = exec_start(re, input, stateful)? else {
        return Ok(None);
    };
    let (re, data, input) = match start {
        Start::Number(_) => (re, data, input),
        // `valueOf` ran: every address, and the receiver's data, may have
        // changed (`compile` publishes new data).
        Start::Coerced(_, re, input) => (re, super::regexp_data_ptr(re), input),
    };
    search_from(
        re,
        unsafe { (*data).perex_program },
        input,
        start.at(),
        stateful,
        mode,
        budget,
        memory,
        captures,
        poll,
    )
}

/// [`search_builtin`] from a start [`exec_start`] established, over the
/// RegExp's program cell `program`, with the same address contract. The one
/// copy of the in-place search every builtin exec runs.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn search_from<'mem>(
    re: *mut RegExpHeader,
    program: *const u8,
    input: *const StringHeader,
    start: usize,
    stateful: bool,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    captures: &mut Option<host::Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Option<(Span, Position)>, EngineError> {
    let (found, position, re) = host::find_in_place(
        re, program, input, start,
        // Only g/y searches can start away from zero. A non-stateful call
        // gains nothing from finding or recording a position.
        stateful, mode, budget, memory, QUANTUM, captures, poll,
    )?;
    if stateful {
        store_last_index(re, found.map_or(0, |full| full.end()))?;
    }
    Ok(found.map(|full| (full, position)))
}

/// Where RegExpBuiltinExec starts.
#[derive(Clone, Copy)]
enum Start {
    /// From a Number `lastIndex`: nothing ran, every address is unchanged.
    Number(usize),
    /// From a `lastIndex` whose ToLength ran user code, with the RegExp's and
    /// the string's addresses after it.
    Coerced(usize, *mut RegExpHeader, *const StringHeader),
}

impl Start {
    fn at(self) -> usize {
        match self {
            Self::Number(at) | Self::Coerced(at, ..) => at,
        }
    }
}

/// RegExpBuiltinExec steps 4-12: the search's start. `None` when the start is
/// past the end, after a g/y RegExp's `lastIndex` was reset to 0.
#[inline(always)]
fn exec_start(
    re: *mut RegExpHeader,
    input: *const StringHeader,
    stateful: bool,
) -> Result<Option<Start>, EngineError> {
    let stored = crate::value::JSValue::from_bits(super::get_last_index(re).to_bits());
    // ToLength(Get(R, "lastIndex")) is observable only when it is not a
    // Number (it may call `valueOf`); a Number matters only to g/y.
    let start = if stored.is_number() {
        Start::Number(if stateful {
            stored
                .as_number()
                .max(0.0)
                .floor()
                .min(9_007_199_254_740_991.0) as usize
        } else {
            0
        })
    } else {
        coerced_start(re, input, stateful)?
    };
    let (re, input) = match start {
        Start::Number(_) => (re, input),
        Start::Coerced(_, re, input) => (re, input),
    };
    if start.at() > unsafe { (*input).utf16_len as usize } {
        if stateful {
            store_last_index(re, 0)?;
        }
        return Ok(None);
    }
    Ok(Some(start))
}

/// [`exec_start`] for a `lastIndex` that is not a Number, whose ToLength may
/// run `valueOf` and so collect: the RegExp and the string are rooted across it.
#[cold]
#[inline(never)]
fn coerced_start(
    re: *mut RegExpHeader,
    input: *const StringHeader,
    stateful: bool,
) -> Result<Start, EngineError> {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(re);
    let string = scope.root_string_ptr(input);
    let last_index = caught(|| super::regex_last_index_offset(receiver.with_const_ptr(|re| re)))?;
    Ok(Start::Coerced(
        if stateful { last_index } else { 0 },
        receiver.with_mut_ptr(|re| re),
        string.with_const_ptr(|s| s),
    ))
}

/// Spec `Set(R, "lastIndex", n, true)` (RegExpBuiltinExec steps 14/18), with
/// the TypeError for a non-writable `lastIndex` returned instead of thrown.
/// Neither branch runs user code.
fn store_last_index(re: *mut RegExpHeader, n: usize) -> Result<(), EngineError> {
    super::set_last_index_caught(crate::value::js_nanbox_pointer(re as i64), n as f64)
        .map_err(EngineError::Abrupt)
}
