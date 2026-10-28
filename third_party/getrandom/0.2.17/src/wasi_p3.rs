//! WASI 0.3 backend; backported using the getrandom 0.4.3 manual binding.
use crate::Error;
use core::{mem::MaybeUninit, ptr::copy_nonoverlapping};
// The std/libc linked by Perry supplies the WASI 0.3 component type metadata,
// as required by the upstream getrandom 0.4.3 manual-binding backend.
#[link(wasm_import_module = "wasi:random/random@0.3.0")]
extern "C" {
    #[link_name = "get-random-u64"]
    fn imported_get_random_u64() -> u64;
}

#[inline]
fn get_random_u64() -> u64 {
    // This canonical scalar import is infallible and has no pointer arguments.
    unsafe { imported_get_random_u64() }
}

#[inline]
pub fn getrandom_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    let (prefix, chunks, suffix) = unsafe { dest.align_to_mut::<MaybeUninit<u64>>() };

    // We use `get_random_u64` instead of `get_random_bytes` because the latter creates
    // an allocation due to the Wit IDL [restrictions][0]. This should be fine since
    // the main use case of `getrandom` is seed generation.
    //
    // [0]: https://github.com/WebAssembly/wasi-random/issues/27
    if !prefix.is_empty() {
        let val = get_random_u64();
        let src = (&val as *const u64).cast();
        unsafe {
            copy_nonoverlapping(src, prefix.as_mut_ptr(), prefix.len());
        }
    }

    for dst in chunks {
        dst.write(get_random_u64());
    }

    if !suffix.is_empty() {
        let val = get_random_u64();
        let src = (&val as *const u64).cast();
        unsafe {
            copy_nonoverlapping(src, suffix.as_mut_ptr(), suffix.len());
        }
    }

    Ok(())
}
