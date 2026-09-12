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
            cell.write(ProgramCell { word_count: words });
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
        let program = self.root.get_raw_const_ptr::<u8>();
        receiver.with_mut_ptr::<super::RegExpHeader, _>(|receiver| unsafe {
            (*receiver).perex_program = program;
            crate::gc::runtime_write_barrier_gc_slot(
                receiver as usize,
                std::ptr::addr_of!((*receiver).perex_program) as usize,
                program as u64,
            );
        });
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
        let ptr =
            receiver.with_const_ptr::<super::RegExpHeader, _>(|r| unsafe { (*r).perex_program });
        if ptr.is_null() {
            return Err(OwnerError::Missing);
        }
        Ok(Self {
            root: scope.root_raw_const_ptr(ptr),
        })
    }
}

impl ImmutableProgram for GcProgram<'_> {
    type Error = OwnerError;

    fn with_words<T>(&self, f: impl FnOnce(&[u32]) -> T) -> Result<T, Self::Error> {
        self.root.with_const_ptr::<ProgramCell, _>(|cell| unsafe {
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
            // getter neither allocates nor polls and always reacquires the base.
            Ok(f(std::slice::from_raw_parts(cell.add(1).cast(), count)))
        })
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
        let ptr = root.get_raw_const_ptr::<StringHeader>();
        if ptr.is_null() {
            return Err(OwnerError::Missing);
        }
        crate::string::js_string_addref(ptr.cast_mut());
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
