//! Globs compile into the same GC-owned Perex programs as RegExp. Native
//! filesystem strings are borrowed directly; no JS subject copy is required.
use super::perex_api as api;
use super::perex_memory::MemoryBudget;
use super::perex_owner::{GcProgram, HeapSubject, OwnerError};
use super::perex_runtime::{self as host, CaptureMode, EngineError};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use perex::binding::{BoundProgram, BoundSubject, ImmutableSubject, Subject};
use perex::Budget;

#[derive(Debug)]
pub(crate) struct NativeText<'a>(pub(crate) &'a str);
impl ImmutableSubject for NativeText<'_> {
    type Error = OwnerError;
    fn with_subject<T>(&self, f: impl FnOnce(Subject<'_>) -> T) -> Result<T, OwnerError> {
        Ok(f(Subject::Wtf8(self.0.as_bytes())))
    }
}

pub(crate) struct GlobProgram<'s> {
    program: BoundProgram<GcProgram<'s>>,
}
impl<'s> GlobProgram<'s> {
    #[cfg(test)]
    pub(crate) fn program_words_address(&self) -> usize {
        self.program
            .with_view(|p| p.words().as_ptr() as usize)
            .unwrap()
    }
    pub(crate) fn new(scope: &'s RuntimeHandleScope, source: &str) -> Result<Self, EngineError> {
        let pattern =
            BoundSubject::new(NativeText(source)).map_err(|e| EngineError::Subject(e.error))?;
        let mut budget = Budget::new(api::WORK);
        let memory = MemoryBudget::new(api::SCRATCH_BYTES);
        let program = host::compile(
            scope,
            &pattern,
            super::validate_and_canonicalize_flags("u"),
            &mut budget,
            &memory,
            api::PROGRAM_BYTES,
            &mut host::poll,
        )?;
        let program =
            BoundProgram::new(program, &mut budget).map_err(|e| EngineError::Program(e.error))?;
        Ok(Self { program })
    }

    pub(crate) fn is_match(&self, input: &str) -> Result<bool, EngineError> {
        let subject =
            BoundSubject::new(NativeText(input)).map_err(|e| EngineError::Subject(e.error))?;
        self.test(&subject)
    }

    pub(crate) fn test_heap(&self, input: RuntimeHandle<'_>) -> Result<bool, EngineError> {
        let owner = unsafe { HeapSubject::new(input) }
            .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?;
        let subject = BoundSubject::new(owner).map_err(|e| EngineError::Subject(e.error))?;
        self.test(&subject)
    }

    pub(crate) fn test<S: ImmutableSubject<Error = OwnerError>>(
        &self,
        subject: &BoundSubject<S>,
    ) -> Result<bool, EngineError> {
        self.test_with(
            subject,
            &mut Budget::new(api::WORK),
            &MemoryBudget::new(api::SCRATCH_BYTES),
            api::QUANTUM,
            &mut host::poll,
        )
    }

    pub(crate) fn test_with<S: ImmutableSubject<Error = OwnerError>>(
        &self,
        subject: &BoundSubject<S>,
        budget: &mut Budget,
        memory: &MemoryBudget,
        quantum: usize,
        poll: &mut impl FnMut() -> Result<(), EngineError>,
    ) -> Result<bool, EngineError> {
        host::find(
            &self.program,
            subject,
            0,
            CaptureMode::Full,
            budget,
            memory,
            quantum,
            poll,
        )
        .map(|found| found.is_some())
    }
}

/// Filesystem helpers already return JS abrupt-completion values. Map a
/// Perex resource/error outcome through the common RegExp error boundary;
/// callers propagate it after dropping native filesystem temporaries.
pub(crate) fn js_error(error: EngineError) -> f64 {
    match api::caught(|| api::finish::<()>(Err(error))) {
        Err(EngineError::Abrupt(value)) => value,
        _ => unreachable!("the common error boundary must throw"),
    }
}
