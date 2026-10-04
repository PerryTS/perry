//! Perry's program and subject owners for Perex's scoped-borrow API.
//!
//! Programs contain only an inline word count and immutable program words.
//! The collector can move them; owners retain registered handles, never bases.
//! Compilation scratch belongs to the caller, and emission writes directly
//! into the final GC allocation after the pattern borrow has ended.

use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use perex::binding::{ImmutableProgram, ImmutableSubject, Subject};
use perex::compiler::{CompileError, Prepared};

#[repr(C)]
struct ProgramCell {
    word_count: usize,
    /// What validating these words established, so later bindings of this same
    /// cell skip validation (#10166). Plain data beside the words it describes:
    /// the words never change, and a recompile emits a new cell that starts
    /// with none, so it cannot describe other words. Stored by the first
    /// validating bind; the cell stays a pointer-free leaf.
    witness: Option<perex::binding::ProgramWitness>,
    /// The register count a search of these words needs, read from a bound
    /// view of them once (S6), so a per-call search need not open a view to
    /// size its scratch. Like the witness, it describes only these immutable
    /// words and a recompiled program starts without one.
    registers: Option<usize>,
    // Immediately followed by word_count initialized u32 words.
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OwnerError {
    Missing,
    InvalidLayout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BuildError {
    Compile(CompileError),
    SizeLimit,
    Allocation,
    Abrupt(u64),
}

/// An immutable program held by a real mutable collector root.
pub(crate) struct GcProgram<'scope> {
    root: RuntimeHandle<'scope>,
}

impl std::fmt::Debug for GcProgram<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcProgram").finish_non_exhaustive()
    }
}

impl<'scope> GcProgram<'scope> {
    /// Root an immutable program retrieved from the construction cache.
    ///
    /// # Safety
    /// `program` must be a live cell emitted here, with no collecting operation
    /// between the cache lookup and establishing this independent root.
    pub(crate) unsafe fn from_cached(
        scope: &'scope RuntimeHandleScope,
        program: *const u8,
    ) -> Self {
        Self {
            root: scope.root_raw_const_ptr(program),
        }
    }

    pub(crate) fn with_ptr<T>(&self, f: impl FnOnce(*const u8) -> T) -> T {
        self.root.with_const_ptr(f)
    }

    /// Consume a prepared compiler plan with no retained pattern/flag views.
    /// No host callback or collecting operation occurs during emission.
    pub(crate) fn emit(
        scope: &'scope RuntimeHandleScope,
        plan: Prepared<'_>,
        max_program_bytes: usize,
    ) -> Result<Self, BuildError> {
        let words = plan.required_words();
        let size = words
            .checked_mul(std::mem::size_of::<u32>())
            .and_then(|n| n.checked_add(std::mem::size_of::<ProgramCell>()))
            .filter(|&n| n <= max_program_bytes)
            // Arena sizes include the GC header and round to eight bytes.
            .filter(|&n| n <= u32::MAX as usize - crate::gc::GC_HEADER_SIZE - 7)
            .ok_or(BuildError::SizeLimit)?;
        let cell = crate::exception::catch_js_throw(|| {
            crate::arena::arena_alloc_gc(
                size,
                std::mem::align_of::<ProgramCell>(),
                crate::gc::GC_TYPE_REGEX_PROGRAM,
            )
        })
        .map_err(|value| BuildError::Abrupt(value.to_bits()))?
            as *mut ProgramCell;
        if cell.is_null() {
            return Err(BuildError::Allocation);
        }
        // The new leaf is not exposed until emission succeeds. Even an error
        // leaves a valid GC leaf that can be reclaimed normally, without a
        // finalizer or a leaked external owner. No GC call occurs in this scope.
        unsafe {
            // GC_STORE_AUDIT(POINTER_FREE): the program cell is a leaf of u32 words; its prefix is a count.
            cell.write(ProgramCell {
                word_count: words,
                witness: None,
                registers: None,
            });
            let output = cell.add(1).cast::<u32>();
            output.write_bytes(0, words);
            let output = std::slice::from_raw_parts_mut(output, words);
            plan.emit(output).map_err(BuildError::Compile)?;
        }
        Ok(Self {
            root: scope.root_raw_const_ptr(cell),
        })
    }

    /// Install the program edge with the ordinary old-to-young/incremental
    /// barrier. The receiver's registered root is re-read at the store.
    ///
    /// # Safety
    /// `receiver` must root a live initialized RegExpHeader. No mutable program
    /// words may be published through any other interface.
    pub(crate) unsafe fn install(&self, receiver: &RuntimeHandle<'_>) {
        // A field store and its barrier: neither allocates, so both addresses
        // stay current for the whole store.
        self.root.with_const_ptr::<u8, _>(|program| {
            receiver.with_mut_ptr::<super::RegExpHeader, _>(|receiver| unsafe {
                (*receiver).perex_program = program;
                crate::gc::runtime_write_barrier_gc_slot(
                    receiver as usize,
                    std::ptr::addr_of!((*receiver).perex_program) as usize,
                    program as u64,
                );
            })
        });
    }

    /// The witness stored beside this program's words, if a binding validated
    /// them before (#10166).
    pub(crate) fn witness(&self) -> Option<perex::binding::ProgramWitness> {
        self.root
            .with_const_ptr::<ProgramCell, _>(|cell| unsafe { (*cell).witness })
    }

    /// Record what validating this program established. `witness` must come
    /// from a binding of this same cell.
    pub(crate) fn record_witness(
        root: &RuntimeHandle<'_>,
        witness: perex::binding::ProgramWitness,
    ) {
        // The prefix lies outside the word slice, but the write still goes
        // through the cell's own pointer and never under a live view of its
        // words (a binding holds none between calls).
        #[cfg(debug_assertions)]
        debug_assert_eq!(
            PROGRAM_VIEWS.with(std::cell::Cell::get),
            0,
            "a program cell's witness must not be written while a view of its words is live"
        );
        // A plain-data store into a pointer-free leaf: no allocation, no barrier.
        root.with_const_ptr::<ProgramCell, _>(|cell| unsafe {
            (*(cell as *mut ProgramCell)).witness = Some(witness);
        });
    }

    /// This program's registered root, which survives consuming the owner.
    pub(crate) fn root(&self) -> RuntimeHandle<'scope> {
        self.root
    }

    /// Establish a separate operation root, so reentrant receiver recompilation
    /// cannot replace the immutable program of an already-running operation.
    ///
    /// # Safety
    /// The handle must root a live initialized RegExpHeader; its program edge
    /// must have been installed by this module.
    pub(crate) unsafe fn from_receiver(
        scope: &'scope RuntimeHandleScope,
        receiver: &RuntimeHandle<'_>,
    ) -> Result<Self, OwnerError> {
        receiver
            .with_const_ptr::<super::RegExpHeader, _>(|r| unsafe { Self::from_regexp(scope, r) })
    }

    /// [`Self::from_receiver`] for the RegExp at `re`, read now.
    ///
    /// # Safety
    /// `re` must be the current address of a live initialized RegExpHeader.
    pub(crate) unsafe fn from_regexp(
        scope: &'scope RuntimeHandleScope,
        re: *const super::RegExpHeader,
    ) -> Result<Self, OwnerError> {
        let ptr = unsafe { (*re).perex_program };
        if ptr.is_null() {
            return Err(OwnerError::Missing);
        }
        // Rooting pushes a handle slot and never collects.
        Ok(Self {
            root: scope.root_raw_const_ptr(ptr),
        })
    }
}

/// The witness stored in the program cell at `program` (a RegExp's
/// `perex_program`), for tests.
#[cfg(test)]
pub(crate) unsafe fn cell_witness(program: *const u8) -> Option<perex::binding::ProgramWitness> {
    unsafe { (*(program as *const ProgramCell)).witness }
}

/// Overwrite the witness stored in the program cell at `program`, for tests.
#[cfg(test)]
pub(crate) unsafe fn set_cell_witness(
    program: *const u8,
    witness: Option<perex::binding::ProgramWitness>,
) {
    unsafe { (*(program as *mut ProgramCell)).witness = witness };
}

// How many `with_words` views of any program cell are live on this thread, so
// debug builds can prove a witness is never written under one (#10166).
#[cfg(debug_assertions)]
thread_local! {
    static PROGRAM_VIEWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct ProgramView;

impl ProgramView {
    fn open() -> Self {
        #[cfg(debug_assertions)]
        PROGRAM_VIEWS.with(|views| views.set(views.get() + 1));
        ProgramView
    }
}

impl Drop for ProgramView {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        PROGRAM_VIEWS.with(|views| views.set(views.get() - 1));
    }
}

/// The words of the program cell at `cell`, after checking that it is one.
///
/// # Safety
/// `cell` must be null or the current address of a live GC allocation, and no
/// collecting action may run while `f` holds the words.
unsafe fn cell_words<T>(
    cell: *const ProgramCell,
    f: impl FnOnce(&[u32]) -> T,
) -> Result<T, OwnerError> {
    unsafe {
        if cell.is_null() {
            return Err(OwnerError::Missing);
        }
        let header = cell
            .cast::<u8>()
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>();
        let count = (*cell).word_count;
        let available = ((*header).size as usize)
            .checked_sub(crate::gc::GC_HEADER_SIZE + std::mem::size_of::<ProgramCell>())
            .ok_or(OwnerError::InvalidLayout)?;
        if (*header).obj_type != crate::gc::GC_TYPE_REGEX_PROGRAM || count > available / 4 {
            return Err(OwnerError::InvalidLayout);
        }
        // Only emit creates these cells; no mutable word access escapes.
        // Binding validation is separate, once per immutable owner. This
        // getter neither allocates nor polls.
        let _view = ProgramView::open();
        Ok(f(std::slice::from_raw_parts(cell.add(1).cast(), count)))
    }
}

impl ImmutableProgram for GcProgram<'_> {
    type Error = OwnerError;

    fn with_words<T>(&self, f: impl FnOnce(&[u32]) -> T) -> Result<T, Self::Error> {
        // Reacquires the base from the registered root on every view.
        self.root
            .with_const_ptr::<ProgramCell, _>(|cell| unsafe { cell_words(cell, f) })
    }
}

/// One builtin search's program and subject, read where they live: the
/// RegExp's program cell, which carries its own validation witness, and the
/// string's own bytes (S6). Nothing is copied, rooted or marked to start a
/// search.
///
/// Both bases are raw addresses, valid only until the next collecting action.
/// A search polls only between quanta (and before it builds owned scratch or
/// capture slots), and every one of those polls goes through
/// [`Reacquire`]: `before_poll` roots the program cell and marks the string
/// shared the first time, `after_poll` reloads both bases from their roots. So
/// a collection that moves either one during a long search is never read from
/// its old address. A search decided in its first quantum, which is nearly
/// every per-call `test`/`exec`, never reaches a poll and roots nothing.
///
/// The program is rooted on its own, not re-read through the RegExp, so a
/// receiver recompiled while the search is paused cannot change the matcher
/// of the running search (the `GcProgram::from_receiver` rule).
pub(crate) struct InPlace<'s, 'h> {
    scope: &'s RuntimeHandleScope,
    input: &'h RuntimeHandle<'h>,
    program: std::cell::Cell<*const ProgramCell>,
    string: std::cell::Cell<*const StringHeader>,
    /// The program cell's root, taken at the first poll.
    pinned: std::cell::Cell<Option<RuntimeHandle<'s>>>,
}

impl<'s, 'h> InPlace<'s, 'h> {
    /// Read the bases of the current program of the RegExp at `re` and of
    /// `input`. The caller must run no collecting action between this and the
    /// search, other than the polls the search routes through [`Reacquire`].
    ///
    /// # Safety
    /// `re` must be the current address of a live RegExpHeader and `input`
    /// must have been rooted with `root_string_ptr` from a live heap string.
    /// `scope` must be the innermost live handle scope whenever the search
    /// polls.
    pub(crate) unsafe fn new(
        scope: &'s RuntimeHandleScope,
        re: *const super::RegExpHeader,
        input: &'h RuntimeHandle<'h>,
    ) -> Result<Self, OwnerError> {
        let program = unsafe { (*re).perex_program.cast::<ProgramCell>() };
        let string = input.get_raw_const_ptr::<StringHeader>();
        if program.is_null() || string.is_null() {
            return Err(OwnerError::Missing);
        }
        Ok(Self {
            scope,
            input,
            program: std::cell::Cell::new(program),
            string: std::cell::Cell::new(string),
            pinned: std::cell::Cell::new(None),
        })
    }

    /// The witness stored beside the program's words, if a binding validated
    /// them before (#10166).
    pub(crate) fn witness(&self) -> Option<perex::binding::ProgramWitness> {
        self.check_current();
        unsafe { (*self.program.get()).witness }
    }

    /// Record what validating this program established; see
    /// [`GcProgram::record_witness`].
    pub(crate) fn record_witness(&self, witness: perex::binding::ProgramWitness) {
        self.check_current();
        #[cfg(debug_assertions)]
        debug_assert_eq!(PROGRAM_VIEWS.with(std::cell::Cell::get), 0);
        // A plain-data store into a pointer-free leaf: no allocation, no barrier.
        unsafe { (*(self.program.get() as *mut ProgramCell)).witness = Some(witness) };
    }

    /// The register count recorded in the program cell, if any.
    pub(crate) fn registers(&self) -> Option<usize> {
        self.check_current();
        unsafe { (*self.program.get()).registers }
    }

    /// Record the register count of this program's words, read from a view
    /// of a binding of this same cell.
    pub(crate) fn record_registers(&self, registers: usize) {
        self.check_current();
        // A plain-data store into a pointer-free leaf: no allocation, no barrier.
        unsafe { (*(self.program.get() as *mut ProgramCell)).registers = Some(registers) };
    }

    /// The string header at the current base. Read it, never keep it.
    pub(crate) fn string(&self) -> *const StringHeader {
        self.check_current();
        self.string.get()
    }

    /// The program's words, for a `BoundProgram`.
    pub(crate) fn words(&self) -> CellWords<'_, 's, 'h> {
        CellWords(self)
    }

    /// The subject's bytes, for a `BoundSubject`.
    pub(crate) fn bytes(&self) -> StringBytes<'_, 's, 'h> {
        StringBytes(self)
    }

    /// Debug builds prove every view reads the current bases: the string's
    /// against the root it came from, the program's against its own root once
    /// one exists (before that no poll has run, so nothing can have moved).
    #[inline]
    fn check_current(&self) {
        #[cfg(debug_assertions)]
        {
            debug_assert_eq!(
                self.string.get(),
                self.input.get_raw_const_ptr::<StringHeader>(),
                "in-place subject read after a collection without reacquiring its base"
            );
            if let Some(root) = self.pinned.get() {
                debug_assert_eq!(
                    self.program.get(),
                    root.get_raw_const_ptr::<ProgramCell>(),
                    "in-place program read after a collection without reacquiring its base"
                );
            }
        }
    }
}

impl super::perex_runtime::Reacquire for InPlace<'_, '_> {
    fn before_poll(&self) {
        if self.pinned.get().is_none() {
            // Rooting pushes a handle slot and never collects.
            self.pinned
                .set(Some(self.scope.root_raw_const_ptr(self.program.get())));
            // The binding now outlives a collecting action, so it takes the
            // sharing rule `HeapSubject::new` takes: no unique-owner append
            // may mutate these bytes until it ends.
            crate::string::js_string_addref(self.string.get() as *mut StringHeader);
        }
    }

    fn after_poll(&self) {
        if let Some(root) = self.pinned.get() {
            self.program.set(root.get_raw_const_ptr());
        }
        self.string.set(self.input.get_raw_const_ptr());
    }
}

/// [`InPlace`]'s program words.
pub(crate) struct CellWords<'a, 's, 'h>(&'a InPlace<'s, 'h>);

impl ImmutableProgram for CellWords<'_, '_, '_> {
    type Error = OwnerError;

    fn with_words<T>(&self, f: impl FnOnce(&[u32]) -> T) -> Result<T, Self::Error> {
        self.0.check_current();
        unsafe { cell_words(self.0.program.get(), f) }
    }
}

/// [`InPlace`]'s subject bytes: the string's original WTF-8 storage.
pub(crate) struct StringBytes<'a, 's, 'h>(&'a InPlace<'s, 'h>);

impl ImmutableSubject for StringBytes<'_, '_, '_> {
    type Error = OwnerError;

    fn with_subject<T>(&self, f: impl FnOnce(Subject<'_>) -> T) -> Result<T, Self::Error> {
        self.0.check_current();
        let s = self.0.string.get();
        // The base is current (see `InPlace`), and nothing here collects.
        let bytes = unsafe {
            std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize)
        };
        Ok(f(Subject::Wtf8(bytes)))
    }
}

/// Original heap-string storage. Sharing disables Perry's unique-owner append
/// mutation for the whole binding lifetime; a root alone would not do that.
pub(crate) struct HeapSubject<'scope> {
    root: RuntimeHandle<'scope>,
    byte_window: Option<(usize, usize)>,
}

impl std::fmt::Debug for HeapSubject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeapSubject").finish_non_exhaustive()
    }
}

impl<'scope> HeapSubject<'scope> {
    /// # Safety
    /// `root` must have been created with root_string_ptr from a live, initialized
    /// heap string. All mutable string writers must respect Perry's sharing rule.
    pub(crate) unsafe fn new(root: RuntimeHandle<'scope>) -> Result<Self, OwnerError> {
        if root.with_const_ptr::<StringHeader, _>(|ptr| ptr.is_null()) {
            return Err(OwnerError::Missing);
        }
        root.with_mut_ptr::<StringHeader, _>(|ptr| crate::string::js_string_addref(ptr));
        Ok(Self {
            root,
            byte_window: None,
        })
    }

    /// A segment-local subject over the original immutable string. Binding
    /// validation checks the window's encoding; every borrow reacquires the
    /// original allocation and applies the same byte bounds after movement.
    ///
    /// # Safety
    /// The root has the same provenance requirement as `new`.
    pub(crate) unsafe fn window(
        root: RuntimeHandle<'scope>,
        start: usize,
        end: usize,
    ) -> Result<Self, OwnerError> {
        let mut owner = unsafe { Self::new(root)? };
        owner.byte_window = Some((start, end));
        Ok(owner)
    }
}

impl ImmutableSubject for HeapSubject<'_> {
    type Error = OwnerError;

    fn with_subject<T>(&self, f: impl FnOnce(Subject<'_>) -> T) -> Result<T, Self::Error> {
        // The constructor seals the root's provenance. with_string_bytes
        // reacquires original WTF-8 storage and does not form a Rust str.
        unsafe {
            self.root.with_string_bytes(|bytes| {
                let bytes = match self.byte_window {
                    None => bytes,
                    Some((start, end)) => bytes.get(start..end).ok_or(OwnerError::InvalidLayout)?,
                };
                Ok(f(Subject::Wtf8(bytes)))
            })
        }
    }
}
