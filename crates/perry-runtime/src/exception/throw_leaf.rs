//! The four typed throw functions of the runtime.
//!
//! Every throw the runtime's own Rust code makes goes through one of these
//! leaves; the argument type says where the thrown value came from:
//!
//! * [`throw_core`]: an error object the runtime built itself
//!   ([`CoreError`], made only by the error constructors in `crate::error`
//!   and the io builders in `crate::io_error`);
//! * [`rethrow_caught`]: a value a catching funnel took from a throw
//!   ([`Caught`], made only by [`super::catch_thrown`] and
//!   [`super::pending_exception`]);
//! * [`throw_program_value`]: a value the program handed to the runtime (an
//!   argument, or state the program stored), thrown as is;
//! * [`throw_open`]: anything else (error-like objects the runtime builds
//!   outside the error constructors, markers).
//!
//! Each leaf is a distinct cold, never-inlined symbol, so the runtime's call
//! graph tells which kinds of value an entry can throw. `js_throw` stays the
//! entry point of compiled code (a program `throw` statement); runtime Rust
//! code does not call it.

/// Which leaf a throw came through. Printed by `PERRY_TRACE_THROW`; passing
/// it is also what keeps the leaves distinct functions (identical bodies
/// would be merged into one symbol).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ThrowKind {
    /// `js_throw`: a `throw` statement of compiled code.
    Compiled,
    Core,
    Caught,
    ProgramValue,
    Open,
}

impl ThrowKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            ThrowKind::Compiled => "compiled",
            ThrowKind::Core => "core",
            ThrowKind::Caught => "caught",
            ThrowKind::ProgramValue => "program value",
            ThrowKind::Open => "open",
        }
    }
}

/// An error object the runtime built: an `ErrorHeader` fresh from one of
/// the error constructors. The field is private: only
/// [`CoreError::mint`] makes one, and only the error constructors
/// (`crate::error`) and the io error builders (`crate::io_error`) call it.
#[derive(Debug)]
#[must_use = "a runtime error is built to be thrown or handed on"]
pub struct CoreError(f64);

impl CoreError {
    /// Trace an existing core-error slot without discarding its provenance.
    pub(crate) fn scan_roots(&mut self, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
        visitor.visit_nanbox_f64_slot(&mut self.0);
    }

    /// Wrap the NaN-boxed pointer of a freshly built `ErrorHeader`.
    /// Callers: `crate::error` and `crate::io_error` only (checked by the
    /// throw-funnel gate).
    #[inline]
    pub(crate) fn mint(value: f64) -> Self {
        CoreError(value)
    }

    /// The error as a JS value (to reject a promise with, to pass to a
    /// callback, or to set a property on before it is thrown).
    #[inline]
    pub fn value(&self) -> f64 {
        self.0
    }
}

/// A value a catching funnel took from a throw (compiled code or a callback
/// threw it). The field is private: only [`Caught::mint`] makes one, and
/// only the funnels in `crate::exception` call it.
#[derive(Clone, Copy, Debug)]
#[must_use = "a caught value is handled or rethrown"]
pub struct Caught(f64);

impl PartialEq for Caught {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for Caught {}

impl Caught {
    /// Trace the existing caught slot without losing its provenance.
    pub(crate) fn scan_roots(&mut self, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
        visitor.visit_nanbox_f64_slot(&mut self.0);
    }

    /// Callers: `crate::exception` only (checked by the throw-funnel gate).
    #[inline]
    pub(crate) fn mint(value: f64) -> Self {
        Caught(value)
    }

    /// The caught value.
    #[inline]
    pub fn value(&self) -> f64 {
        self.0
    }
}

/// A [`Caught`] value held in a GC root, for a value that is kept across
/// further allocation before it is rethrown. Made only from a `Caught`.
#[must_use = "a caught value is handled or rethrown"]
pub struct RootedCaught<'scope>(crate::gc::RuntimeHandle<'scope>);

impl Caught {
    /// Root the caught value in `scope` (the value is the same one).
    #[inline]
    pub fn root<'scope>(
        self,
        scope: &'scope crate::gc::RuntimeHandleScope,
    ) -> RootedCaught<'scope> {
        RootedCaught(scope.root_nanbox_f64(self.0))
    }
}

impl RootedCaught<'_> {
    /// Read the caught value back from its rooted slot after any collection.
    #[inline]
    pub fn caught(&self) -> Caught {
        Caught(self.0.get_nanbox_f64())
    }

    /// The caught value (current after any collection).
    #[inline]
    pub fn value(&self) -> f64 {
        self.0.get_nanbox_f64()
    }
}

/// Throw an error object the runtime built.
#[cold]
#[inline(never)]
pub fn throw_core(e: CoreError) -> ! {
    super::raise_thrown(e.0, ThrowKind::Core)
}

/// Throw a caught value on, unchanged.
#[cold]
#[inline(never)]
pub fn rethrow_caught(c: Caught) -> ! {
    super::raise_thrown(c.0, ThrowKind::Caught)
}

/// Throw a value the program handed to the runtime (an argument, or state
/// the program stored), as is.
#[cold]
#[inline(never)]
pub fn throw_program_value(v: f64) -> ! {
    super::raise_thrown(v, ThrowKind::ProgramValue)
}

/// Throw any other value (an error-like object built outside the error
/// constructors, a marker).
#[cold]
#[inline(never)]
pub fn throw_open(v: f64) -> ! {
    super::raise_thrown(v, ThrowKind::Open)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exception::{catch_thrown, js_clear_exception, js_has_exception, pending_exception};

    fn caught_bits(f: impl FnOnce()) -> u64 {
        let caught = catch_thrown(f).expect_err("the leaf throws");
        assert_eq!(
            js_has_exception(),
            0,
            "the funnel clears the pending exception"
        );
        caught.value().to_bits()
    }

    /// Each leaf throws exactly the value it was given.
    #[test]
    fn each_leaf_throws_exactly_its_value() {
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        assert_eq!(caught_bits(|| throw_program_value(7.5)), 7.5f64.to_bits());
        assert_eq!(
            caught_bits(|| throw_open(undefined)),
            crate::value::TAG_UNDEFINED
        );

        let bytes = b"leaf";
        let msg = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let error = crate::error::CoreError::type_error(msg);
        let bits = error.value().to_bits();
        assert_eq!(caught_bits(|| throw_core(error)), bits);

        let first = catch_thrown(|| throw_open(2.5)).expect_err("throws");
        assert_eq!(caught_bits(|| rethrow_caught(first)), 2.5f64.to_bits());
    }

    /// The untyped `catch_js_throw` is `catch_thrown` with the value read.
    #[test]
    fn catch_js_throw_reads_the_caught_value() {
        let r: Result<u8, f64> = crate::exception::catch_js_throw(|| throw_open(4.0));
        assert_eq!(r, Err(4.0));
        assert_eq!(catch_thrown(|| 9u8).ok(), Some(9));
    }

    /// `pending_exception` reads the pending value without clearing it.
    #[test]
    fn pending_exception_reads_without_clearing() {
        crate::exception::test_set_exception(3.0);
        assert_eq!(pending_exception().value(), 3.0);
        assert_eq!(js_has_exception(), 1);
        js_clear_exception();
    }

    /// A `CoreError` is a real error object of the constructor's kind.
    #[test]
    fn core_error_constructors_make_error_objects() {
        let bytes = b"m";
        let msg = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let error = crate::error::CoreError::range_error(msg);
        let addr = crate::value::js_nanbox_get_pointer(error.value()) as usize;
        unsafe {
            assert!(crate::error::ptr_is_native_error(addr));
            assert_eq!(
                (*(addr as *const crate::error::ErrorHeader)).error_kind,
                crate::error::ERROR_KIND_RANGE_ERROR
            );
        }
    }
}

/// A core error retained in an existing GC root across further allocation.
#[must_use = "a runtime error is handled or thrown"]
pub struct RootedCoreError<'scope>(crate::gc::RuntimeHandle<'scope>);

impl CoreError {
    #[inline]
    pub fn root<'scope>(
        self,
        scope: &'scope crate::gc::RuntimeHandleScope,
    ) -> RootedCoreError<'scope> {
        RootedCoreError(scope.root_nanbox_f64(self.0))
    }
}
impl RootedCoreError<'_> {
    #[inline]
    pub fn value(&self) -> f64 {
        self.0.get_nanbox_f64()
    }
    #[inline]
    pub fn error(&self) -> CoreError {
        CoreError(self.value())
    }
}
