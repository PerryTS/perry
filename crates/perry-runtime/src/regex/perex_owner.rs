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
            init_program_cell(cell, words);
            let output = cell.add(1).cast::<u32>();
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
            receiver.with_mut_ptr::<super::RegExpData, _>(|receiver| unsafe {
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
        Self::from_data(scope, crate::regex::regexp_data_ptr(re))
    }

    /// Root the immutable program edge from an already branded data cell.
    pub(crate) unsafe fn from_data(
        scope: &'scope RuntimeHandleScope,
        data: *const super::RegExpData,
    ) -> Result<Self, OwnerError> {
        let ptr = unsafe { (*data).perex_program };
        if ptr.is_null() {
            return Err(OwnerError::Missing);
        }
        // Rooting pushes a handle slot and never collects.
        Ok(Self {
            root: scope.root_raw_const_ptr(ptr),
        })
    }
}

/// Clear a freshly allocated program cell's whole payload, then write its
/// prefix: an empty count-sized cell whose words emission fills.
///
/// The arena hands out recycled bytes uncleared, and `witness: None` and
/// `registers: None` store only their discriminants, so without the clear the
/// unused option payloads (and the padding after an odd word count) keep the
/// previous occupant's words. Nothing reads them, but the whole-heap from-space
/// scan does, and on tsc they were stale nursery addresses it reported as
/// offenders in every regex program cell (16 per run, deterministic). The
/// clear also provides the zeroed words emission requires.
///
/// # Safety
/// `cell` must be the payload of a live `GC_TYPE_REGEX_PROGRAM` allocation
/// sized for `words` program words, not yet visible to anything else.
unsafe fn init_program_cell(cell: *mut ProgramCell, words: usize) {
    unsafe {
        let header = cell
            .cast::<u8>()
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>();
        let payload = (*header).size as usize - crate::gc::GC_HEADER_SIZE;
        debug_assert!(payload >= std::mem::size_of::<ProgramCell>() + words * 4);
        // GC_STORE_AUDIT(POINTER_FREE): the program cell is a leaf of u32 words; its prefix is a count.
        cell.cast::<u8>().write_bytes(0, payload);
        std::ptr::addr_of_mut!((*cell).word_count).write(words);
        std::ptr::addr_of_mut!((*cell).witness).write(None);
        std::ptr::addr_of_mut!((*cell).registers).write(None);
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

/// One builtin search's receiver, program and subject, read where they live:
/// the RegExp, its program cell, which carries its own validation witness, and
/// the string's own bytes (S6). Nothing is copied, rooted or marked to start a
/// search.
///
/// Each of the three is held as its current address. A search polls only
/// between quanta (and before it builds owned scratch or capture slots), and
/// every view of the program or the subject happens inside one quantum, which
/// neither allocates nor collects, so a view reads the address it holds.
/// [`PollRoots::before_poll`] roots all three before the first poll, and
/// [`PollRoots::after_poll`] reads every address back from those roots after
/// each poll, so a collection that moved any of them is seen by the next view.
/// Views therefore cannot fail: their resource error type is uninhabited.
///
/// A search decided in its first quantum, which is nearly every per-call
/// `test`/`exec`, never reaches a poll and roots nothing.
///
/// The program is rooted on its own, not re-read through the RegExp, so a
/// receiver recompiled while the search is paused cannot change the matcher
/// of the running search (the `GcProgram::from_receiver` rule).
///
/// [`PollRoots::before_poll`]: super::perex_runtime::PollRoots::before_poll
/// [`PollRoots::after_poll`]: super::perex_runtime::PollRoots::after_poll
pub(crate) struct InPlace {
    receiver: std::cell::Cell<*const super::RegExpHeader>,
    cell: std::cell::Cell<*const ProgramCell>,
    input: std::cell::Cell<*const StringHeader>,
    /// The program's word count, validated against the cell's size once.
    word_count: usize,
    /// The roots the first poll takes, slots of `scope`.
    rooted: std::cell::Cell<Option<Rooted>>,
    /// The handle scope the first poll opens. A search that never polls opens
    /// none. It is the innermost scope when it opens (nothing between this
    /// search's start and its polls opens one), and it closes when this does,
    /// after every scope opened inside the polls has closed.
    scope: std::cell::OnceCell<RuntimeHandleScope>,
    /// The heap generation the three addresses were read in, so debug builds
    /// can prove no view reads an address a collection has made stale.
    #[cfg(debug_assertions)]
    generation: std::cell::Cell<u64>,
}

/// The roots the first poll takes. Their lifetime is `InPlace::scope`'s, which
/// the type system cannot name for a field of the same struct; they never
/// leave the `InPlace` that owns that scope.
#[derive(Clone, Copy)]
struct Rooted {
    receiver: RuntimeHandle<'static>,
    cell: RuntimeHandle<'static>,
    input: RuntimeHandle<'static>,
}

impl InPlace {
    /// Take the RegExp at `re`, its program cell `program` and the string at
    /// `input`. The caller must run no collecting action between reading those
    /// addresses and the search, other than the polls the search routes
    /// through [`PollRoots`](super::perex_runtime::PollRoots).
    ///
    /// # Safety
    /// `re` must be the current address of a live, branded RegExp, `program`
    /// null or the current address of the program cell its data holds, and
    /// `input` the current address of a live, non-null heap string. No handle
    /// scope may open between this and the search's polls that is still open
    /// when this is dropped.
    #[inline(always)]
    pub(crate) unsafe fn new(
        re: *const super::RegExpHeader,
        program: *const u8,
        input: *const StringHeader,
    ) -> Result<Self, OwnerError> {
        let cell = program.cast::<ProgramCell>();
        if input.is_null() {
            return Err(OwnerError::Missing);
        }
        let word_count = unsafe { cell_words(cell, <[u32]>::len)? };
        Ok(Self {
            receiver: std::cell::Cell::new(re),
            cell: std::cell::Cell::new(cell),
            input: std::cell::Cell::new(input),
            word_count,
            rooted: std::cell::Cell::new(None),
            scope: std::cell::OnceCell::new(),
            #[cfg(debug_assertions)]
            generation: std::cell::Cell::new(crate::gc::heap_generation::heap_generation()),
        })
    }

    /// Debug builds: every address read is from the current heap generation.
    #[inline(always)]
    fn check_current(&self) {
        #[cfg(debug_assertions)]
        debug_assert_eq!(
            crate::gc::heap_generation::heap_generation(),
            self.generation.get(),
            "in-place search read an address after the heap changed"
        );
    }

    /// Pass the program cell's current address to `f`, which must neither
    /// collect nor retain it.
    #[inline(always)]
    fn with_cell<T>(&self, f: impl FnOnce(*const ProgramCell) -> T) -> T {
        self.check_current();
        f(self.cell.get())
    }

    /// Pass the subject's current header to `f`, which must neither collect
    /// nor retain it. Every subject reaching a search is a writable heap
    /// string, so `f` may store plain flags into the header.
    #[inline(always)]
    pub(crate) fn with_string_mut<T>(&self, f: impl FnOnce(*mut StringHeader) -> T) -> T {
        self.check_current();
        f(self.input.get().cast_mut())
    }

    /// The RegExp's current address. Read it again after any collecting action.
    #[inline(always)]
    pub(crate) fn receiver(&self) -> *mut super::RegExpHeader {
        self.check_current();
        self.receiver.get().cast_mut()
    }

    /// The witness stored beside the program's words, if a binding validated
    /// them before (#10166).
    #[inline(always)]
    pub(crate) fn witness(&self) -> Option<perex::binding::ProgramWitness> {
        self.with_cell(|cell| unsafe { (*cell).witness })
    }

    /// Record what validating this program established; see
    /// [`GcProgram::record_witness`].
    #[inline]
    pub(crate) fn record_witness(&self, witness: perex::binding::ProgramWitness) {
        #[cfg(debug_assertions)]
        debug_assert_eq!(PROGRAM_VIEWS.with(std::cell::Cell::get), 0);
        // A plain-data store into a pointer-free leaf: no allocation, no barrier.
        self.with_cell(|cell| unsafe { (*(cell as *mut ProgramCell)).witness = Some(witness) });
    }

    /// The register count recorded in the program cell, if any.
    #[inline(always)]
    pub(crate) fn registers(&self) -> Option<usize> {
        self.with_cell(|cell| unsafe { (*cell).registers })
    }

    /// Record the register count of this program's words, read from a view
    /// of a binding of this same cell.
    #[inline]
    pub(crate) fn record_registers(&self, registers: usize) {
        // A plain-data store into a pointer-free leaf: no allocation, no barrier.
        self.with_cell(|cell| unsafe { (*(cell as *mut ProgramCell)).registers = Some(registers) });
    }

    /// The program's words, for a `BoundProgram`.
    #[inline(always)]
    pub(crate) fn words(&self) -> CellWords<'_> {
        CellWords(self)
    }

    /// The subject's bytes, for a `BoundSubject`.
    #[inline(always)]
    pub(crate) fn bytes(&self) -> StringBytes<'_> {
        StringBytes(self)
    }
}

impl super::perex_runtime::PollRoots for InPlace {
    fn before_poll(&self) {
        if self.rooted.get().is_none() {
            // Rooting pushes handle slots and never collects.
            let scope = self.scope.get_or_init(RuntimeHandleScope::new);
            // SAFETY: the handles name slots of `scope`, which this owns and
            // drops last; they are only read through `self`.
            let scope: &'static RuntimeHandleScope = unsafe { &*(scope as *const _) };
            let input = scope.root_string_ptr(self.input.get());
            // The binding now outlives a collecting action, so it takes the
            // sharing rule `HeapSubject::new` takes: no unique-owner append
            // may mutate these bytes until it ends.
            input.with_mut_ptr::<StringHeader, _>(|s| crate::string::js_string_addref(s));
            self.rooted.set(Some(Rooted {
                receiver: scope.root_raw_const_ptr(self.receiver.get()),
                cell: scope.root_raw_const_ptr(self.cell.get()),
                input,
            }));
        }
    }

    fn after_poll(&self) {
        if let Some(rooted) = self.rooted.get() {
            // A collection may have moved any of the three: read every
            // address back from its root before the next view.
            self.receiver.set(
                rooted
                    .receiver
                    .with_const_ptr::<super::RegExpHeader, _>(|p| p),
            );
            self.cell
                .set(rooted.cell.with_const_ptr::<ProgramCell, _>(|p| p));
            self.input
                .set(rooted.input.with_const_ptr::<StringHeader, _>(|p| p));
            #[cfg(debug_assertions)]
            self.generation
                .set(crate::gc::heap_generation::heap_generation());
        }
    }
}

/// [`InPlace`]'s program words.
pub(crate) struct CellWords<'a>(&'a InPlace);

impl ImmutableProgram for CellWords<'_> {
    type Error = std::convert::Infallible;

    #[inline(always)]
    fn with_words<T>(&self, f: impl FnOnce(&[u32]) -> T) -> Result<T, Self::Error> {
        // `InPlace::new` validated the cell's type and size against this
        // count; a collection moves the cell's bytes, never changes them.
        Ok(self.0.with_cell(|cell| unsafe {
            f(std::slice::from_raw_parts(
                cell.add(1).cast::<u32>(),
                self.0.word_count,
            ))
        }))
    }
}

/// [`InPlace`]'s subject bytes: the string's original WTF-8 storage.
pub(crate) struct StringBytes<'a>(&'a InPlace);

impl ImmutableSubject for StringBytes<'_> {
    type Error = std::convert::Infallible;

    #[inline(always)]
    fn with_subject<T>(&self, f: impl FnOnce(Subject<'_>) -> T) -> Result<T, Self::Error> {
        // `InPlace::new`'s contract seals the string's provenance, and nothing
        // here collects.
        Ok(self.0.with_string_mut(|s| unsafe {
            f(Subject::Wtf8(std::slice::from_raw_parts(
                crate::string::string_data(s),
                (*s).byte_len as usize,
            )))
        }))
    }
}

/// A resource error a search can report. In-place views cannot fail.
pub(crate) trait HostResourceError {
    fn into_owner(self) -> OwnerError;
}

impl HostResourceError for OwnerError {
    #[inline(always)]
    fn into_owner(self) -> OwnerError {
        self
    }
}

impl HostResourceError for std::convert::Infallible {
    #[inline(always)]
    fn into_owner(self) -> OwnerError {
        match self {}
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

#[cfg(test)]
mod program_cell_tests {
    use super::*;

    /// A program cell laid out over `fill`-filled memory, as the arena hands it
    /// out, initialised for `words` words. Returns the payload bytes.
    fn init_over(fill: u8, words: usize) -> Vec<u8> {
        let payload = (std::mem::size_of::<ProgramCell>() + words * 4 + 7) & !7;
        let total = crate::gc::GC_HEADER_SIZE + payload;
        let mut backing = vec![u64::from_ne_bytes([fill; 8]); total / 8];
        unsafe {
            let header = backing.as_mut_ptr().cast::<crate::gc::GcHeader>();
            (*header).obj_type = crate::gc::GC_TYPE_REGEX_PROGRAM;
            (*header).size = total as u32;
            let cell = backing
                .as_mut_ptr()
                .cast::<u8>()
                .add(crate::gc::GC_HEADER_SIZE)
                .cast::<ProgramCell>();
            init_program_cell(cell, words);
            std::slice::from_raw_parts(cell.cast::<u8>(), payload).to_vec()
        }
    }

    /// The cell's bytes must not depend on what the memory held before: the
    /// whole-heap from-space scan reads every payload word, and residue in the
    /// unused `None` payloads read there as stale nursery references (tsc,
    /// 2026-10-06). An odd word count also leaves tail padding to cover.
    #[test]
    fn program_cell_bytes_do_not_carry_the_previous_occupant() {
        for words in [0usize, 1, 7, 64] {
            let clean = init_over(0x00, words);
            let dirty = init_over(0xA5, words);
            assert_eq!(
                clean, dirty,
                "a program cell for {words} words kept bytes of the memory it was built in"
            );
            unsafe {
                let cell = dirty.as_ptr().cast::<ProgramCell>();
                assert_eq!(
                    std::ptr::read_unaligned(std::ptr::addr_of!((*cell).word_count)),
                    words
                );
            }
        }
    }
}
