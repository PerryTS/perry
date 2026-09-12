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
use perex::{span::Span, Budget};

// One explicit host policy; no retained scratch cache or alternate engine.
pub(crate) const WORK: usize = 100_000_000;
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
    let exception = if type_error {
        crate::error::js_typeerror_new(text.get_raw_mut_ptr::<StringHeader>())
    } else if syntax {
        crate::error::js_syntaxerror_new(text.get_raw_mut_ptr::<StringHeader>())
    } else {
        crate::error::js_rangeerror_new(text.get_raw_mut_ptr::<StringHeader>())
    };
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
    BoundProgram::new(owner, budget).map_err(|e| EngineError::Program(e.error))
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
    )
}

/// Compound String operations keep one allowance across successive matches.
/// Each execution has its own root scope, so a global loop cannot retain a
/// root for every previous result.
pub(crate) fn execute_with_resources(
    receiver: *mut RegExpHeader,
    input: *const StringHeader,
    materialize: bool,
    budget: &mut Budget,
    memory: &MemoryBudget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Option<ExecMatch>, EngineError> {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(receiver);
    let input = scope.root_string_ptr(input);
    let last_index = caught(|| receiver.with_const_ptr(super::regex_last_index_offset))?;
    let (stateful, has_indices) = receiver.with_const_ptr::<RegExpHeader, _>(|r| unsafe {
        ((*r).global || (*r).sticky, (*r).has_indices)
    });
    let start = if stateful { last_index } else { 0 };
    let length = input.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len as usize });
    if start > length {
        if stateful {
            caught(|| {
                super::set_last_index_throwing(receiver.get_raw_mut_ptr::<RegExpHeader>(), 0)
            })?;
        }
        return Ok(None);
    }
    let program = program(&scope, &receiver, budget, memory, poll)?;
    let subject = BoundSubject::new(
        unsafe { HeapSubject::new(input) }
            .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?,
    )
    .map_err(|e| EngineError::Subject(e.error))?;
    let found = host::find(
        &program,
        &subject,
        start,
        if materialize {
            CaptureMode::All
        } else {
            CaptureMode::Full
        },
        budget,
        memory,
        QUANTUM,
        poll,
    )?;
    if stateful {
        let next = found.as_ref().map_or(0, |m| m.full.end());
        caught(|| {
            super::set_last_index_throwing(receiver.get_raw_mut_ptr::<RegExpHeader>(), next)
        })?;
    }
    let Some(found) = found else {
        return Ok(None);
    };
    let (array, groups) = if materialize {
        // Captures and all native owners remain ABOVE the JS trap. A thrown
        // allocation/property operation returns here before they are dropped.
        caught(|| {
            super::perex_results::materialize(
                &input,
                &subject,
                &program,
                &found,
                has_indices,
                budget,
                poll,
            )
        })??
    } else {
        (std::ptr::null_mut(), std::ptr::null_mut())
    };
    Ok(Some(ExecMatch {
        full: found.full,
        array,
        groups,
    }))
}
