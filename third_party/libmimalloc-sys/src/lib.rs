#![no_std]
// Copyright 2019 Octavian Oncescu

use core::ffi::c_void;

#[cfg(feature = "extended")]
mod extended;
#[cfg(feature = "extended")]
pub use extended::*;

extern "C" {
    /// Allocate zero-initialized `size` bytes.
    ///
    /// Returns a pointer to newly allocated zero-initialized memory, or null if
    /// out of memory.
    pub fn mi_zalloc(size: usize) -> *mut c_void;

    /// Allocate `size` bytes.
    ///
    /// Returns pointer to the allocated memory or null if out of memory.
    /// Returns a unique pointer if called with `size` 0.
    pub fn mi_malloc(size: usize) -> *mut c_void;

    /// Re-allocate memory to `newsize` bytes.
    ///
    /// Return pointer to the allocated memory or null if out of memory. If null
    /// is returned, the pointer `p` is not freed. Otherwise the original
    /// pointer is either freed or returned as the reallocated result (in case
    /// it fits in-place with the new size).
    ///
    /// If `p` is null, it behaves as [`mi_malloc`]. If `newsize` is larger than
    /// the original `size` allocated for `p`, the bytes after `size` are
    /// uninitialized.
    pub fn mi_realloc(p: *mut c_void, newsize: usize) -> *mut c_void;

    /// Allocate `size` bytes aligned by `alignment`, initialized to zero.
    ///
    /// Return pointer to the allocated memory or null if out of memory.
    ///
    /// Returns a unique pointer if called with `size` 0.
    pub fn mi_zalloc_aligned(size: usize, alignment: usize) -> *mut c_void;

    /// Allocate `size` bytes aligned by `alignment`.
    ///
    /// Return pointer to the allocated memory or null if out of memory.
    ///
    /// Returns a unique pointer if called with `size` 0.
    pub fn mi_malloc_aligned(size: usize, alignment: usize) -> *mut c_void;

    /// Re-allocate memory to `newsize` bytes, aligned by `alignment`.
    ///
    /// Return pointer to the allocated memory or null if out of memory. If null
    /// is returned, the pointer `p` is not freed. Otherwise the original
    /// pointer is either freed or returned as the reallocated result (in case
    /// it fits in-place with the new size).
    ///
    /// If `p` is null, it behaves as [`mi_malloc_aligned`]. If `newsize` is
    /// larger than the original `size` allocated for `p`, the bytes after
    /// `size` are uninitialized.
    pub fn mi_realloc_aligned(p: *mut c_void, newsize: usize, alignment: usize) -> *mut c_void;

    /// Free previously allocated memory.
    ///
    /// The pointer `p` must have been allocated before (or be null).
    pub fn mi_free(p: *mut c_void);
}

/// When using the `"override"` feature flag, the user wants us to globally
/// override the system allocator.
///
/// However, since we build and link `mimalloc` as a static library/archive,
/// the linker may decide to not care about our overrides if it can't directly
/// see references to the symbols, see the following link for details:
/// <https://maskray.me/blog/2021-06-20-symbol-processing#archive-processing>
///
/// This is problematic if `mimalloc` is used from a library that by itself
/// doesn't allocate, yet invokes other shared libraries that do, since then
/// the linker wouldn't see any references to `malloc`/`free`, and the symbols
/// would not be overridden.
///
/// To avoid this problem, we make sure that the allocator functions are
/// visible to the linker.
///
/// To avoid this problem, we reference `mi_malloc` in a `#[used]` static.
/// This makes it known to `rustc`, which will create a reference to it in a
/// `symbols.o` stub file that is later passed directly to the linker (instead
/// of being in an archive). See the following link for details on how this
/// works: <https://github.com/rust-lang/rust/pull/95604>
///
/// NOTE: This works because `mimalloc` is compiled into a single object file
/// in `static.c`. If it was split across multiple files, we'd need to
/// reference each symbol. See also the comment at the top of `static.c`.
///
/// NOTE: On macOS, mimalloc doesn't just override malloc/free, but also
/// registers itself with the allocator's zone APIs in a ctor
/// (`_mi_macos_override_malloc`, marked with `__attribute__((constructor))`).
/// Similarly to above, for the Mach-O linker to actually consider ctors as
/// "used" when defined in an archive member in a static library, so we need
/// to explicitly reference something in the object file. The constructor
/// symbol itself is static, so we can't get a reference to that, so instead
/// we reference `mi_malloc` here too).
#[cfg(feature = "override")]
mod set_up_statics {
    use super::*;
    #[used] // Could be `#[used(linker)]` once stable
    static USED: unsafe extern "C" fn(usize) -> *mut c_void = mi_malloc;
}

#[cfg(test)]
mod tests {
    extern crate libc;
    use super::*;

    #[test]
    fn it_frees_memory_malloc() {
        let ptr = unsafe { mi_malloc_aligned(8, 8) } as *mut u8;
        unsafe { mi_free(ptr as *mut c_void) };
    }

    #[test]
    fn it_frees_memory_zalloc() {
        let ptr = unsafe { mi_zalloc_aligned(8, 8) } as *mut u8;
        unsafe { mi_free(ptr as *mut c_void) };
    }

    #[test]
    fn it_frees_memory_realloc() {
        let ptr = unsafe { mi_malloc_aligned(8, 8) } as *mut u8;
        let ptr = unsafe { mi_realloc_aligned(ptr as *mut c_void, 8, 8) } as *mut u8;
        unsafe { mi_free(ptr as *mut c_void) };
    }

    #[cfg(all(feature = "override", target_vendor = "apple"))]
    #[test]
    fn mimalloc_and_libc_are_interoperable_when_overridden() {
        let ptr = unsafe { mi_malloc(42) };
        unsafe { libc::free(ptr) };
    }
}
