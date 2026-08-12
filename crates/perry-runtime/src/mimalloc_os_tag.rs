//! macOS VM tag for mimalloc's OS mappings (#6882, repaired by #7450).
//!
//! mimalloc tags every `mmap` it makes with `mi_option_os_tag`, default 100.
//! macOS decodes tag 100 as `IOAccelerator`, so `vmmap`, Instruments' VM
//! Tracker and `footprint` render Perry's entire JS heap as GPU-driver
//! memory. #6882 retagged it to `VM_MEMORY_APPLICATION_SPECIFIC_1` (240),
//! which those tools show as the neutral `Memory Tag 240`.
//!
//! **The retag has to happen before mimalloc's first OS mapping, which means
//! before `main`.** #6882 set the option from `js_gc_init` and reasoned that
//! only "regions mapped before this call (early Rust startup)" would keep tag
//! 100. That is wrong on a per-mapping basis and catastrophically wrong in
//! aggregate: mimalloc does not `mmap` per allocation, it reserves an *arena*
//! — `mi_option_arena_reserve`, 1 GiB on 64-bit — on the very first
//! allocation, and then satisfies everything afterwards by committing pages
//! *inside* that existing reservation. Committing does not re-tag: the tag is
//! fixed by the `mmap` that created the region. So the one mapping that
//! matters is made during Rust's own startup, long before any Perry code
//! runs, and every later retag lands on an option nobody reads again. On
//! tree.ts that showed up as ~230 MB of `IOAccelerator` against ~64 KB of
//! `Memory Tag 240` — the retag covering literally the rounding error.
//!
//! Moving the call earlier inside Rust does not help; measured, setting the
//! option as the first statement of `main` still yields a 100-tagged heap,
//! because `std`'s pre-`main` runtime setup has already allocated. The only
//! placement that beats mimalloc's first mapping is a Mach-O module
//! initializer (`__DATA,__mod_init_func`), which dyld runs while preparing the
//! image — i.e. before `main`, and before any Rust allocation.
//!
//! Consequently this file runs on the pre-`main` path and **must not
//! allocate**: an allocation here would initialize mimalloc and reserve the
//! arena before we had set the tag, reintroducing the bug it exists to fix.
//! That is why the `MIMALLOC_OS_TAG` opt-out is read with `libc::getenv`
//! rather than `std::env::var_os`, which allocates an `OsString`.

/// `VM_MEMORY_APPLICATION_SPECIFIC_1`. Chosen because macOS reserves 240-255
/// for applications and renders them as a plain `Memory Tag <n>`, so the heap
/// is both distinctive and obviously not a system subsystem.
#[cfg(all(
    target_pointer_width = "64",
    target_vendor = "apple",
    feature = "alloc-mimalloc"
))]
pub const PERRY_HEAP_VM_TAG: libc::c_long = 240;

/// Point mimalloc's `os_tag` at [`PERRY_HEAP_VM_TAG`], unless an operator is
/// already steering it with `MIMALLOC_OS_TAG` (profilers use that to isolate
/// a run; the runtime defers rather than fighting them).
///
/// Runs before `main` from [`MOD_INIT_FUNC_ENTRY`]. Allocation-free — see the
/// module docs. Idempotent: `mi_option_set` just writes mimalloc's static
/// option table, so the second call from `js_gc_init` is a no-op write.
#[cfg(all(
    target_pointer_width = "64",
    target_vendor = "apple",
    feature = "alloc-mimalloc"
))]
extern "C" fn apply_mimalloc_os_tag() {
    // SAFETY: `getenv` is well-defined pre-`main` on Apple platforms (dyld has
    // published `environ` before it runs module initializers), and takes a NUL-
    // terminated string, which the C literal is. `mi_option_set` only stores
    // into mimalloc's static option table — no allocation, no lock, no
    // dependency on mimalloc having been initialized.
    unsafe {
        if libc::getenv(c"MIMALLOC_OS_TAG".as_ptr()).is_null() {
            libmimalloc_sys::mi_option_set(libmimalloc_sys::mi_option_os_tag, PERRY_HEAP_VM_TAG);
        }
    }
}

/// The Mach-O equivalent of `__attribute__((constructor))`: dyld walks
/// `__DATA,__mod_init_func` when the image is prepared and calls every
/// function pointer in it, before `main` and before `std`'s runtime setup —
/// the only window that precedes mimalloc's arena reservation.
#[cfg(all(
    target_pointer_width = "64",
    target_vendor = "apple",
    feature = "alloc-mimalloc"
))]
#[used]
#[link_section = "__DATA,__mod_init_func"]
static MOD_INIT_FUNC_ENTRY: extern "C" fn() = apply_mimalloc_os_tag;

/// Called from `js_gc_init`. Does two things, neither of them the retag that
/// actually matters — that already happened pre-`main`:
///
/// 1. Gives the linker a reason to pull this module's object file out of
///    `libperry_runtime.a`. `#[used]` stops `-dead_strip` from dropping
///    [`MOD_INIT_FUNC_ENTRY`], but it cannot pull an otherwise unreferenced
///    archive member into the link in the first place, and at
///    `codegen-units > 1` this module is its own object. Being `#[inline(never)]`
///    and defined here, this function *is* that reason: the call from
///    `js_gc_init` selects the member, and the constructor rides along.
/// 2. Re-applies the option, covering the (currently hypothetical) case of a
///    host that ran Perry's runtime without honouring module initializers. Any
///    arena mimalloc reserves *after* this point is then tagged correctly.
///
/// It deliberately does *not* reference `MOD_INIT_FUNC_ENTRY` from code, which
/// is the obvious way to write step 1 and does not link: `ld` rewrites
/// `__DATA,__mod_init_func` into a `__TEXT,__init_offsets` table of 32-bit
/// offsets, so the static has no address left to take and the reference fails
/// as `ADRP out of range ... to 0x00000000`.
#[inline(never)]
pub fn ensure_mimalloc_os_tag_applied() {
    #[cfg(all(
        target_pointer_width = "64",
        target_vendor = "apple",
        feature = "alloc-mimalloc"
    ))]
    apply_mimalloc_os_tag();
}
