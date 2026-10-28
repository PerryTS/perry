//! Function names and retained source for reflection. Ordinary functions in a
//! permanent executable carry source as a relative range in their static
//! `JsFunctionInfo`; these registries remain for names, unloadable images,
//! runtime-created functions, and raw method/accessor bodies.
//!
//! # Why there are two entry points per registry (#9188)
//!
//! Registration runs once per function a bundle CONTAINS — 72,713 of them on
//! the compiled claude-code TUI — so whatever one call costs is a startup cost
//! the whole program pays, whether or not it ever reads a name. Copying the
//! bytes cost 5.1 MB of names and 23.8 MB of source text in dirty anonymous
//! memory, duplicating data the program image was already carrying read-only.
//!
//! The cheap fix is to store `(ptr, len)` and let the registry borrow the
//! image. That is only sound if the bytes outlive the PROCESS, which is a
//! strictly stronger promise than "outlives the call" — and these are
//! `#[no_mangle] pub extern "C"` symbols, reachable from separately-loaded
//! provider images and from FFI, so it is not a promise that can be imposed on
//! callers that already exist. The two demands are therefore split rather than
//! traded:
//!
//! * [`js_register_function_name`] / [`js_register_function_source`] keep the
//!   original contract — the bytes need only outlive the call, because the
//!   registry copies them. Every caller that is not codegen uses these.
//! * [`js_register_function_name_static`] /
//!   [`js_register_function_source_static`] require process lifetime and store
//!   the borrowed slice. Their plural batch forms are what executable codegen
//!   emits from `__perry_init_strings_<prefix>`, where the bytes are `@.str.N`
//!   `private unnamed_addr constant` globals in the image.
//!
//! All the volume is on the borrowing side, so the copy is gone from the path
//! that had it, and no published contract was tightened underneath a caller.
//!
//! # Storage
//!
//! Borrowed and owned bytes live in SEPARATE maps rather than one map of an
//! enum. An enum value would add 8 bytes to every one of the ~60,000 borrowed
//! entries to carry the handful of owned ones, which measured as a net loss
//! (+0.31 MB) even though it removed 1.8 MB of copies. Owned entries take
//! precedence on read: an owned entry can only come from an explicit runtime
//! registration, which is the more specific statement about that function.

#[cfg(test)]
use crate::codegen_abi::LayoutWord;
/// `PERRY_EAGER_FN_METADATA_UTF8=1` restores UTF-8 validation at registration
/// time, for A/B measurement of the startup cost #9187 moved to the read side.
fn eager_fn_metadata_validation() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || {
        matches!(
            std::env::var("PERRY_EAGER_FN_METADATA_UTF8").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// Decode a registered metadata slot, treating non-UTF-8 bytes as absent.
///
/// Registration stores raw bytes so module init does not validate every
/// function's name and source text; the check moves here, where it runs only
/// for metadata something actually reads. Invalid bytes yield `None`, which is
/// what the caller saw before when registration rejected them up front.
fn decode_registered(slot: Option<&[u8]>) -> Option<String> {
    decode_registered_with(slot, str::to_owned)
}

/// Consume a validated view while its registry owner is locked. The caller
/// chooses whether the result needs owned bytes; presence checks do not.
#[inline(always)]
fn decode_registered_with<R>(slot: Option<&[u8]>, read: impl FnOnce(&str) -> R) -> Option<R> {
    decode_registered_bytes(slot?).map(read)
}

/// ASCII is already a UTF-8 proof. Consume that byte view without entering
/// the general decoder; non-ASCII and malformed input keep its exact exits.
#[inline]
fn decode_registered_bytes(bytes: &[u8]) -> Option<&str> {
    if bytes.is_ascii() {
        // SAFETY: every byte is below 0x80, including embedded NULs.
        Some(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        std::str::from_utf8(bytes).ok()
    }
}

/// Link-time-relative descriptor emitted by codegen. Keeping offsets instead
/// of pointers avoids dynamic relocations and dirty data pages at process
/// startup. The static linker diagnoses an image whose symbols do not fit in
/// the same signed 32-bit PC-relative range used by generated calls.
#[repr(C)]
#[derive(Clone, Copy)]
struct FunctionNameDescriptor {
    function_offset: i32,
    bytes_offset: i32,
    byte_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FunctionSourceDescriptor {
    function_offset: i32,
    bytes_offset: i32,
    byte_len: u32,
    is_non_strict_ordinary: i32,
}

fn descriptor_pointer<T>(base: *const u8, offset: i32) -> *const T {
    // The target is normally in a DIFFERENT image allocation/section from the
    // descriptor table, so pointer::offset would violate Rust provenance even
    // though the link-time numeric difference is correct. Decode through the
    // exposed integer address, as the runtime's DWARF/stack-map readers do.
    (base as usize).wrapping_add_signed(offset as isize) as *const T
}

/// Sidecar registry mapping each user-defined function's compiled address
/// to the JS name it should print as via `console.log` / `util.inspect`.
/// Codegen emits a `js_register_function_name_static(func_ptr, name_bytes,
/// len)` call from module init for every named function in `Hir.functions`,
/// so by the time user code runs the map is fully populated. Functions never
/// rename, so we accept lossy single-writer semantics (last-write wins on the
/// rare duplicate). See #1202.
///
/// Direct lookup against the symbol table via `dladdr` doesn't work here
/// because the macOS linker's `-dead_strip` removes the symbol *names* of
/// perry_fn_* globals (the bodies stay — they're referenced by pointer — but
/// the symbol entries vanish, so `dli_sname` comes back null).
///
/// The bytes are BORROWED from the program image and never copied: this map
/// is written only by [`js_register_function_name_static`], whose contract is
/// process lifetime.
///
/// Ordered by code address so exact reflection/capture lookups and predecessor
/// lookups for stack rendering share this one owner. No stack-side snapshot.
fn function_name_registry(
) -> &'static std::sync::Mutex<std::collections::BTreeMap<usize, &'static [u8]>> {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<std::sync::Mutex<std::collections::BTreeMap<usize, &'static [u8]>>> =
        OnceLock::new();
    crate::once_init::get_or_init(&REGISTRY, || {
        std::sync::Mutex::new(std::collections::BTreeMap::new())
    })
}

/// Names whose bytes the registry owns, because the caller could not promise
/// they outlive the process.
///
/// Two writers: [`js_register_function_name`], the copying entry point every
/// caller that is not codegen uses; and [`register_function_name_if_absent`],
/// which INFERS a name at run time (a symbol's description, `get <key>`) from
/// bytes that exist nowhere in the image.
fn function_name_overrides(
) -> &'static std::sync::Mutex<crate::fast_hash::PtrHashMap<usize, std::sync::Arc<[u8]>>> {
    use std::sync::OnceLock;
    // The canonical keys are function addresses. Use the runtime's pointer
    // hasher without changing ownership, replacement or lookup precedence.
    static OVERRIDES: OnceLock<
        std::sync::Mutex<crate::fast_hash::PtrHashMap<usize, std::sync::Arc<[u8]>>>,
    > = OnceLock::new();
    crate::once_init::get_or_init(&OVERRIDES, || {
        std::sync::Mutex::new(crate::fast_hash::new_ptr_hash_map())
    })
}

/// The registered name for `func_ptr`: an owned registration first, then the
/// image.
///
/// Either slot counts as absent when its bytes do not decode — registration
/// does not reject invalid UTF-8 (#9187), so an undecodable entry occupies a
/// slot where nothing used to, and a reader that stopped there would report a
/// name of `None` while a usable one sat in the other map.
///
/// The two locks are never held at the same time, here or anywhere else in
/// this module, so there is no acquisition order to get wrong.
pub(super) fn registered_name_string(func_ptr: usize) -> Option<String> {
    with_registered_name(func_ptr, str::to_owned)
}

/// Read the canonical metadata under its owner lock. Presence checks need the
/// same UTF-8/precedence rules as reflection, but do not need an owned String.
fn with_registered_name<T>(func_ptr: usize, read: impl FnOnce(&str) -> T) -> Option<T> {
    if let Ok(overrides) = function_name_overrides().lock() {
        if let Some(name) = overrides
            .get(&func_ptr)
            .and_then(|bytes| decode_registered_bytes(bytes))
        {
            return Some(read(name));
        }
    }
    function_name_registry()
        .lock()
        .ok()
        .and_then(|map| decode_registered_with(map.get(&func_ptr).copied(), read))
}

/// Register `func_ptr` as the compiled address of a JS function called
/// `<name>` (`name_len` bytes, not NUL-terminated). The registry COPIES the
/// bytes. Idempotent — calling twice with the same `func_ptr` silently
/// overwrites the prior name.
///
/// Codegen does NOT call this; it calls
/// [`js_register_function_name_static`], which borrows instead of copying
/// (#9188). This entry point is for every other caller — the runtime's own
/// global-helper installation, separately-loaded provider images, FFI — where
/// the bytes are not guaranteed to outlive the process.
///
/// # Safety
///
/// `name_ptr..name_ptr+name_len` must point at a byte slice that outlives the
/// call (we copy it). The bytes need not be valid UTF-8: they are decoded on
/// read, and a slot that does not decode reads as absent. `func_ptr` may be
/// anything; we only use it as a map key.
#[no_mangle]
pub unsafe extern "C" fn js_register_function_name(
    func_ptr: *const u8,
    name_ptr: *const u8,
    name_len: u32,
) {
    if func_ptr.is_null() || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    if eager_fn_metadata_validation() && std::str::from_utf8(bytes).is_err() {
        return;
    }
    if let Ok(mut overrides) = function_name_overrides().lock() {
        overrides.insert(func_ptr as usize, std::sync::Arc::from(bytes));
    }
}

/// Codegen-facing entry point: like [`js_register_function_name`], but the
/// registry BORROWS the bytes rather than copying them.
///
/// This is where the volume is — one call per function the bundle contains —
/// and where the copy was worth removing (#9188). Codegen emits the name as an
/// `@.str.N` `private unnamed_addr constant`, which is read-only, file-backed
/// and already resident, so borrowing it costs nothing beyond the map slot.
///
/// # Safety
///
/// `name_ptr..name_ptr+name_len` must point at bytes valid for the REST OF THE
/// PROCESS, not merely for the duration of the call — the registry keeps the
/// pointer. A constant in the program image satisfies this; a heap buffer, a
/// stack buffer, or anything in an image that can be unloaded does not, and
/// must use [`js_register_function_name`] instead. The bytes need not be valid
/// UTF-8 (decoded on read; undecodable reads as absent). `func_ptr` may be
/// anything; we only use it as a map key.
#[no_mangle]
pub unsafe extern "C" fn js_register_function_name_static(
    func_ptr: *const u8,
    name_ptr: *const u8,
    name_len: u32,
) {
    if func_ptr.is_null() || name_ptr.is_null() || name_len == 0 {
        return;
    }
    // The bytes are stored UNVALIDATED and decoded on read (#9187). Module init
    // calls this once per function in the bundle — 72,713 of them for
    // claude-code — and `core::str::from_utf8` was 2.17% of `cc --help` doing
    // exactly this. Nothing observable changes: a non-UTF-8 name was dropped at
    // registration before and is dropped at lookup now, so both report "no
    // name".
    // SAFETY: the caller's contract is process lifetime, which is what makes
    // the `'static` slice below sound.
    let image: &'static [u8] = std::slice::from_raw_parts(name_ptr, name_len as usize);
    if eager_fn_metadata_validation() && std::str::from_utf8(image).is_err() {
        return;
    }
    if let Ok(mut map) = function_name_registry().lock() {
        map.insert(func_ptr as usize, image);
    }
}

/// Register a generated read-only table of function names while acquiring the
/// image registry lock once. Descriptor offsets are relative to
/// `descriptors`, so the table itself contains no absolute pointers.
///
/// # Safety
///
/// `descriptors` must address `count` valid [`FunctionNameDescriptor`] values.
/// Each relative address must resolve within the current process image, and
/// every name byte range must remain valid for the rest of the process.
#[no_mangle]
pub unsafe extern "C" fn js_register_function_names_static(descriptors: *const u8, count: u32) {
    if descriptors.is_null() || count == 0 {
        return;
    }
    // SAFETY: required by this entry point's contract.
    let entries = unsafe {
        std::slice::from_raw_parts(descriptors.cast::<FunctionNameDescriptor>(), count as usize)
    };
    let Ok(mut map) = function_name_registry().lock() else {
        return;
    };
    for entry in entries {
        if entry.byte_len == 0 {
            continue;
        }
        // SAFETY: both relative targets and the name range are guaranteed by
        // this entry point's contract.
        let function = descriptor_pointer::<u8>(descriptors, entry.function_offset);
        let name = descriptor_pointer::<u8>(descriptors, entry.bytes_offset);
        let image: &'static [u8] =
            unsafe { std::slice::from_raw_parts(name, entry.byte_len as usize) };
        if eager_fn_metadata_validation() && std::str::from_utf8(image).is_err() {
            continue;
        }
        map.insert(function as usize, image);
    }
}

/// Register `name` for `func_ptr` only if no name was previously registered.
/// Used by computed-key object literal assignment: when `{ [sym]: fn }` is
/// stored, Node infers the function's name from the symbol's description
/// (`[Function: [<desc>]]`). Anonymous closures hit this; closures that
/// already have a real name (`function f(){}`) are left alone.
///
/// Safe to call from any runtime path.
pub fn register_function_name_if_absent(func_ptr: usize, name: &str) {
    if func_ptr == 0 || name.is_empty() {
        return;
    }
    // "Absent" means "neither map holds bytes that DECODE" — which is exactly
    // the question the read side answers, so ask it rather than restating it
    // and letting the two drift.
    // Presence and reflection share decoded-byte semantics and owned-first
    // lookup; presence consumes only a validated view and never copies a name.
    if with_registered_name(func_ptr, |_| ()).is_some() {
        return;
    }
    if let Ok(mut overrides) = function_name_overrides().lock() {
        overrides
            .entry(func_ptr)
            .and_modify(|bytes| {
                if std::str::from_utf8(bytes).is_err() {
                    *bytes = std::sync::Arc::from(name.as_bytes());
                }
            })
            .or_insert_with(|| std::sync::Arc::from(name.as_bytes()));
    }
}

/// #9486: how many `(function address, name)` pairs the registries currently
/// hold, across both maps. Cheap enough to consult on every `.stack` read, so
/// the stack-frame resolver can tell a stale address-sorted snapshot from a
/// current one without cloning the table to compare it.
///
/// This is a CHANGE DETECTOR, not a cardinality: an address registered in both
/// maps counts twice here and appears once in
/// [`function_name_registry_entries`], which is fine for spotting a stale
/// snapshot and wrong for anything that wants the entry count. Summing rather
/// than counting the union is deliberate — the union costs a set build on
/// every `.stack` read, and double-counting can only ever make the resolver
/// refresh, never make it miss a registration.
pub fn function_name_registry_len() -> Option<usize> {
    let image = function_name_registry().lock().ok()?.len();
    let owned = function_name_overrides().lock().ok()?.len();
    Some(image + owned)
}

/// #9486: snapshot the registries as `(function address, name bytes)` pairs
/// for the `Error.stack` frame resolver to sort by address.
///
/// The lock is held only for each walk — resolution (a binary search per
/// frame) happens outside it, so a `.stack` read never blocks a concurrent
/// registration for longer than the snapshot itself.
pub fn function_name_registry_entries() -> Option<Vec<(usize, std::sync::Arc<[u8]>)>> {
    // The `Arc` in the return type is the resolver's, not the registry's: an
    // image name is borrowed, so materializing one costs a copy of the NAME
    // (tens of bytes), paid per `.stack` snapshot rather than per process.
    let mut out: Vec<(usize, std::sync::Arc<[u8]>)> = function_name_overrides()
        .lock()
        .ok()?
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    let owned: std::collections::HashSet<usize> = out.iter().map(|(k, _)| *k).collect();
    let image = function_name_registry().lock().ok()?;
    out.extend(
        image
            .iter()
            .filter(|(k, _)| !owned.contains(k))
            .map(|(k, v)| (*k, std::sync::Arc::from(*v))),
    );
    Some(out)
}

/// Borrow the generated-bundle metadata owner for exact capture/reflection or
/// address-containment lookups. Runtime-owned names stay outside this table:
/// their thunk addresses must never name adjacent runtime frames. `f` must
/// not register names while it holds the owner lock.
pub(crate) fn with_bundle_function_names<R>(
    f: impl FnOnce(&std::collections::BTreeMap<usize, &'static [u8]>) -> R,
) -> Option<R> {
    let names = function_name_registry().lock().ok()?;
    Some(f(&names))
}

/// The image string a compiled body's name range is the text of
/// ([`FN_NAME_IMAGE`](crate::codegen_abi::FN_NAME_IMAGE)): the function's own
/// `name` value, static data of its body, with nothing to allocate. `None`
/// for a body whose name is not image data.
///
/// # Safety
/// `info` is null or a live function info.
#[inline(always)]
pub(crate) unsafe fn function_name_image(
    info: *const crate::closure::JsFunctionInfo,
) -> Option<*const crate::StringHeader> {
    let body = info.as_ref()?;
    const IMAGE_NAME: u32 = crate::codegen_abi::FN_HAS_NAME | crate::codegen_abi::FN_NAME_IMAGE;
    if body.flags & IMAGE_NAME != IMAGE_NAME {
        return None;
    }
    let base = info.cast::<u8>();
    let source_tail = crate::codegen_abi::function_source_tail_size(body);
    let tail = base.add(std::mem::size_of::<crate::closure::JsFunctionInfo>() + source_tail);
    let bytes: *const u8 = descriptor_pointer(base, tail.cast::<i32>().read_unaligned());
    Some(bytes.sub(std::mem::size_of::<crate::StringHeader>()).cast())
}
const _: () = assert!(
    std::mem::size_of::<crate::StringHeader>() == 20,
    "codegen places an image name's bytes 20 bytes past its header"
);

/// A compiled closure's immutable name range comes from its body descriptor.
/// Legacy/native names retain their existing code-address metadata fallback.
///
/// # Safety
/// The closure is live; its static info owns the optional ranges its flags license.
#[inline(never)]
pub(crate) unsafe fn function_name_for_closure(
    closure: *const crate::closure::ClosureHeader,
) -> Option<String> {
    let closure = closure.as_ref()?;
    let info = closure.info.as_ref()?;
    if info.flags & crate::codegen_abi::FN_HAS_NAME != 0 {
        let base = closure.info.cast::<u8>();
        let source_tail = crate::codegen_abi::function_source_tail_size(info);
        let tail = base.add(std::mem::size_of::<crate::closure::JsFunctionInfo>() + source_tail);
        let delta = tail.cast::<i32>().read_unaligned();
        let len = tail.add(4).cast::<u32>().read_unaligned() as usize;
        // Compiler names originate as UTF-8 strings in immutable image bytes.
        let bytes = std::slice::from_raw_parts(descriptor_pointer(base, delta), len);
        return Some(String::from_utf8_unchecked(bytes.to_vec()));
    }
    function_name_for_ptr(info.code as usize)
}

/// Returns the name registered by `js_register_function_name{,_static}` (keyed
/// on the `__perry_wrap_<name>` wrapper address that
/// `js_closure_alloc_singleton` stamps into the `ClosureHeader`), or `None`
/// when no non-empty name was registered. Used by the spec `fn.name`
/// own-property read (#2059) and by `getOwnPropertyDescriptor(fn, "name")` —
/// the same registry the `[Function: <name>]` console formatter consults.
pub fn function_name_for_ptr(func_ptr: usize) -> Option<String> {
    if func_ptr == 0 {
        return None;
    }
    registered_name_string(func_ptr).filter(|n| !n.is_empty())
}

/// #4101 / #9525: one function's retained source text plus its
/// ordinary-function kind bit.
///
/// Generic over how the bytes are held so the borrowing and owning registries
/// below share one shape: `B` is `&'static [u8]` for image text and
/// `Arc<[u8]>` for a copy.
struct RegisteredFunctionSource<B> {
    bytes: B,
    is_non_strict_ordinary: bool,
}

/// Source text borrowed from the program image, written only by
/// [`js_register_function_source_static`]. Permanent ordinary compiled
/// functions use their info record instead; this map covers raw method/accessor
/// bodies and the other ABI shapes that cannot carry one. Mirrors the name
/// registry's single-writer, last-write-wins semantics.
fn function_source_registry() -> &'static std::sync::Mutex<
    std::collections::HashMap<usize, RegisteredFunctionSource<&'static [u8]>>,
> {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<
        std::sync::Mutex<std::collections::HashMap<usize, RegisteredFunctionSource<&'static [u8]>>>,
    > = OnceLock::new();
    crate::once_init::get_or_init(&REGISTRY, || {
        std::sync::Mutex::new(std::collections::HashMap::new())
    })
}

/// Source text the registry owns, written by [`js_register_function_source`]
/// — the copying entry point for callers that cannot promise process
/// lifetime.
fn function_source_overrides() -> &'static std::sync::Mutex<
    std::collections::HashMap<usize, RegisteredFunctionSource<std::sync::Arc<[u8]>>>,
> {
    use std::sync::OnceLock;
    static OVERRIDES: OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<usize, RegisteredFunctionSource<std::sync::Arc<[u8]>>>,
        >,
    > = OnceLock::new();
    crate::once_init::get_or_init(&OVERRIDES, || {
        std::sync::Mutex::new(std::collections::HashMap::new())
    })
}

/// Register `func_ptr` as the compiled address of a JS function whose original
/// source spans `src_ptr..src_ptr+src_len` (not NUL-terminated). The registry
/// COPIES the bytes. `is_non_strict_ordinary` also records whether the function
/// uses the sloppy ordinary-function `caller` / `arguments` behavior.
/// Idempotent — last write wins.
///
/// Codegen uses this for unloadable dylib/staticlib images; runtime-created
/// functions use it as well.
///
/// # Safety
///
/// `src_ptr..src_ptr+src_len` must point at a byte slice that outlives the
/// call (we copy it). `func_ptr` is used only as a map key. The flag is
/// treated as a boolean (`0` is false; every other value is true).
#[no_mangle]
pub unsafe extern "C" fn js_register_function_source(
    func_ptr: *const u8,
    src_ptr: *const u8,
    src_len: u32,
    is_non_strict_ordinary: i32,
) {
    if func_ptr.is_null() || src_ptr.is_null() || src_len == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(src_ptr, src_len as usize);
    if eager_fn_metadata_validation() && std::str::from_utf8(bytes).is_err() {
        return;
    }
    if let Ok(mut overrides) = function_source_overrides().lock() {
        overrides.insert(
            func_ptr as usize,
            RegisteredFunctionSource {
                bytes: std::sync::Arc::from(bytes),
                is_non_strict_ordinary: is_non_strict_ordinary != 0,
            },
        );
    }
}

/// Codegen-facing entry point: like [`js_register_function_source`], but the
/// registry BORROWS the bytes rather than copying them.
///
/// Permanent ordinary compiled functions now use a relative source range in
/// their `JsFunctionInfo`; this entry point remains for raw method/accessor
/// bodies and compatible external callers.
///
/// # Safety
///
/// `src_ptr..src_ptr+src_len` must point at bytes valid for the REST OF THE
/// PROCESS, not merely for the duration of the call — the registry keeps the
/// pointer. Codegen satisfies this with a range in the module's read-only
/// retained-source blob. Anything that can be freed, or that lives in an image
/// which may be unloaded, must use [`js_register_function_source`] instead.
/// `func_ptr` is used only as a map key. The flag is treated as a boolean (`0`
/// is false; every other value is true).
#[no_mangle]
pub unsafe extern "C" fn js_register_function_source_static(
    func_ptr: *const u8,
    src_ptr: *const u8,
    src_len: u32,
    is_non_strict_ordinary: i32,
) {
    if func_ptr.is_null() || src_ptr.is_null() || src_len == 0 {
        return;
    }
    // Stored unvalidated and decoded on read, as for names above.
    // SAFETY: the caller's contract is process lifetime.
    let image: &'static [u8] = std::slice::from_raw_parts(src_ptr, src_len as usize);
    if eager_fn_metadata_validation() && std::str::from_utf8(image).is_err() {
        return;
    }
    if let Ok(mut map) = function_source_registry().lock() {
        map.insert(
            func_ptr as usize,
            RegisteredFunctionSource {
                bytes: image,
                is_non_strict_ordinary: is_non_strict_ordinary != 0,
            },
        );
    }
}

/// Batch form of [`js_register_function_source_static`], using relative
/// descriptors and one registry lock acquisition.
///
/// # Safety
///
/// `descriptors` must address `count` valid [`FunctionSourceDescriptor`]
/// values. Relative function/source targets must be in the current image, and
/// source byte ranges must remain valid for the rest of the process.
#[no_mangle]
pub unsafe extern "C" fn js_register_function_sources_static(descriptors: *const u8, count: u32) {
    if descriptors.is_null() || count == 0 {
        return;
    }
    // SAFETY: required by this entry point's contract.
    let entries = unsafe {
        std::slice::from_raw_parts(
            descriptors.cast::<FunctionSourceDescriptor>(),
            count as usize,
        )
    };
    let Ok(mut map) = function_source_registry().lock() else {
        return;
    };
    map.reserve(entries.len());
    for entry in entries {
        if entry.byte_len == 0 {
            continue;
        }
        // SAFETY: relative targets and the byte range are guaranteed by this
        // entry point's contract.
        let function = descriptor_pointer::<u8>(descriptors, entry.function_offset);
        let source = descriptor_pointer::<u8>(descriptors, entry.bytes_offset);
        let image: &'static [u8] =
            unsafe { std::slice::from_raw_parts(source, entry.byte_len as usize) };
        if eager_fn_metadata_validation() && std::str::from_utf8(image).is_err() {
            continue;
        }
        map.insert(
            function as usize,
            RegisteredFunctionSource {
                bytes: image,
                is_non_strict_ordinary: entry.is_non_strict_ordinary != 0,
            },
        );
    }
}

/// Whether codegen registered this address as an ordinary non-strict
/// function declaration/expression. This is function-kind metadata rather
/// than an inference from source spelling, so methods and other callable
/// forms remain distinguishable.
pub fn function_is_non_strict_ordinary_for_ptr(func_ptr: usize) -> bool {
    if func_ptr == 0 {
        return false;
    }
    if let Ok(overrides) = function_source_overrides().lock() {
        if let Some(source) = overrides.get(&func_ptr) {
            return source.is_non_strict_ordinary;
        }
    }
    function_source_registry().lock().is_ok_and(|map| {
        map.get(&func_ptr)
            .is_some_and(|source| source.is_non_strict_ordinary)
    })
}

/// Look up the codegen-registered source text for a function pointer.
pub fn function_source_for_ptr(func_ptr: usize) -> Option<String> {
    if func_ptr == 0 {
        return None;
    }
    if let Ok(overrides) = function_source_overrides().lock() {
        if let Some(text) =
            decode_registered(overrides.get(&func_ptr).map(|s| &*s.bytes)).filter(|s| !s.is_empty())
        {
            crate::source_read_log::note("override", text.as_bytes());
            return Some(text);
        }
    }
    let text = function_source_registry()
        .lock()
        .ok()
        .and_then(|map| decode_registered(map.get(&func_ptr).map(|source| source.bytes)))
        .filter(|s| !s.is_empty());
    if let Some(text) = &text {
        crate::source_read_log::note("registry", text.as_bytes());
    }
    text
}

/// Read retained source from a compiled body's info record.
#[inline(always)]
fn retained_source_for_info(info: &crate::closure::JsFunctionInfo) -> Option<String> {
    if info.flags & crate::codegen_abi::FN_HAS_SOURCE == 0 {
        return None;
    }
    crate::source_read_count::note(crate::source_read_count::ReadKind::Info);
    // Most infos pack the range into the otherwise idle versioned-clone mask.
    // Only a body that also has a versioned clone uses the optional tail.
    unsafe {
        let base = info as *const crate::closure::JsFunctionInfo as *const u8;
        if info.flags & crate::codegen_abi::FN_SOURCE_BLOCKS != 0 {
            crate::source_read_count::note(crate::source_read_count::ReadKind::Framed);
            let (record, byte_len) = if info.versioned_code.is_null() {
                (
                    std::ptr::addr_of!(info.versioned_boxed_mask).cast::<u8>(),
                    info.versioned_captures as usize,
                )
            } else {
                (
                    base.add(std::mem::size_of_val(info)),
                    (base.add(std::mem::size_of_val(info) + 8).cast::<u32>()).read_unaligned()
                        as usize,
                )
            };
            let displacement = record.cast::<i32>().read_unaligned() as isize;
            let offset = record.add(4).cast::<u32>().read_unaligned() as usize;
            if byte_len == 0 {
                return None;
            }
            let bytes =
                super::retained_source::decode_range(base.offset(displacement), offset, byte_len);
            // Source ranges originated as &str; block boundaries never become
            // str boundaries, and no lossy conversion touches their bytes.
            return Some(String::from_utf8_unchecked(bytes));
        }
        let (displacement, byte_len) = if info.versioned_code.is_null() {
            (
                info.versioned_boxed_mask as i64 as isize,
                info.versioned_captures as usize,
            )
        } else {
            // SAFETY: with FN_HAS_SOURCE and a live versioned mask, codegen
            // emits `{ JsFunctionInfo, i32 displacement, u32 length }`.
            let tail = base.add(crate::codegen_abi::JS_FUNCTION_INFO_SIZE);
            (
                (tail as *const i32).read_unaligned() as isize,
                (tail.add(std::mem::size_of::<i32>()) as *const u32).read_unaligned() as usize,
            )
        };
        if byte_len == 0 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(base.offset(displacement), byte_len);
        crate::source_read_log::note("info", bytes);
        // Compiler source enters this record as Rust `&str`, so it is valid
        // UTF-8 by construction. Avoid the registry's defensive lossy decode
        // on this compact, compiler-owned path.
        Some(String::from_utf8_unchecked(bytes.to_vec()))
    }
}

pub fn function_source_for_info_or_native(info: &crate::closure::JsFunctionInfo) -> String {
    if let Some(source) = retained_source_for_info(info) {
        return source;
    }
    function_source_for_func_ptr(info.code as usize)
}

/// #4101: build the `Function.prototype.toString` result for a closure whose
/// `ClosureHeader.func_ptr` is `func_ptr`. Returns the retained source text
/// when codegen registered it, otherwise a synthesized native form
/// (`function <name>() { [native code] }`) matching Node's output for
/// functions without recoverable source (built-ins / bound natives).
pub fn function_source_for_func_ptr(func_ptr: usize) -> String {
    if let Some(src) = function_source_for_ptr(func_ptr) {
        return src;
    }
    let name = function_name_for_ptr(func_ptr).unwrap_or_default();
    format!("function {name}() {{ [native code] }}")
}

/// `PERRY_GC_CENSUS`: entries and estimated bytes of the function-name and
/// function-source registries.
pub(crate) fn function_registries_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::hash_table_bytes;
    let mut rows = Vec::new();
    if let Ok(map) = function_name_registry().lock() {
        // Image names cost nothing beyond the map slot (they point into the
        // binary); only the owned maps below carry heap payload.
        rows.push((
            // BTreeMap does not expose allocated node capacity. Report the
            // live entry payload explicitly, excluding node overhead.
            "fn.name_registry(entry-bytes)",
            map.len(),
            map.len() * std::mem::size_of::<(usize, &'static [u8])>(),
        ));
    }
    if let Ok(map) = function_name_overrides().lock() {
        let payload: usize = map.values().map(|b| b.len() + 16).sum();
        rows.push((
            "fn.name_overrides(copied)",
            map.len(),
            hash_table_bytes(
                map.capacity(),
                std::mem::size_of::<(usize, std::sync::Arc<[u8]>)>(),
            ) + payload,
        ));
    }
    if let Ok(map) = function_source_registry().lock() {
        rows.push((
            "fn.source_registry(toString)",
            map.len(),
            hash_table_bytes(
                map.capacity(),
                std::mem::size_of::<(usize, RegisteredFunctionSource<&'static [u8]>)>(),
            ),
        ));
    }
    if let Ok(map) = function_source_overrides().lock() {
        let payload: usize = map.values().map(|s| s.bytes.len() + 16).sum();
        rows.push((
            "fn.source_overrides(copied)",
            map.len(),
            hash_table_bytes(
                map.capacity(),
                std::mem::size_of::<(usize, RegisteredFunctionSource<std::sync::Arc<[u8]>>)>(),
            ) + payload,
        ));
    }
    rows
}

#[cfg(test)]
mod descriptor_name_tests {
    use super::*;
    #[repr(C)]
    struct Image {
        info: crate::closure::JsFunctionInfo,
        tail: [i32; 6],
        name: [u8; 5],
    }
    extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
        0.0
    }
    #[test]
    fn name_range_is_read_after_the_optional_source_range() {
        for source_tail in [0usize, 8, 16] {
            let mut image = Image {
                info: crate::closure::JsFunctionInfo::of(
                    body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
                ),
                tail: [0; 6],
                name: *b"named",
            };
            image.info.flags |= crate::codegen_abi::FN_HAS_NAME;
            if source_tail != 0 {
                image.info.flags |= crate::codegen_abi::FN_HAS_SOURCE;
                if source_tail == 16 {
                    image.info.flags |= crate::codegen_abi::FN_SOURCE_BLOCKS;
                }
                image.info.versioned_code = body as *const u8;
            }
            let slot = source_tail / 4;
            image.tail[slot] =
                (image.name.as_ptr() as isize - &image.info as *const _ as isize) as i32;
            image.tail[slot + 1] = 5;
            let closure = crate::closure::ClosureHeader {
                capture_count: 0,
                shape_id: LayoutWord(crate::codegen_abi::FUNCTION_LAYOUT_WORD),
                info: std::ptr::addr_of!(image.info),
                props: std::ptr::null_mut(),
            };
            assert_eq!(
                unsafe { function_name_for_closure(&closure) }.as_deref(),
                Some("named")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct InfoWithInlineSource<const N: usize> {
        info: crate::closure::JsFunctionInfo,
        bytes: [u8; N],
    }

    /// Fake function addresses. Never dereferenced — the registries only use
    /// them as map keys — but distinct per test so these cases stay
    /// independent of each other and of anything else in the process-global
    /// tables (`perry-runtime` tests share them; see CLAUDE.md).
    const KEY_NAME_COPIED: *const u8 = 0x9188_0001 as *const u8;
    const KEY_NAME_STATIC: *const u8 = 0x9188_0002 as *const u8;
    const KEY_NAME_PRECEDENCE: *const u8 = 0x9188_0003 as *const u8;
    const KEY_NAME_IF_ABSENT: *const u8 = 0x9188_0004 as *const u8;
    const KEY_SOURCE_COPIED: *const u8 = 0x9188_0005 as *const u8;
    const KEY_SOURCE_STATIC: *const u8 = 0x9188_0006 as *const u8;

    /// The copying entry point must not be reachable by a caller whose bytes
    /// die after the call — so prove it copies, without writing the
    /// use-after-free that would be the interesting failure.
    ///
    /// The buffer stays alive and is OVERWRITTEN in place. If registration had
    /// stored `(ptr, len)`, the read below would see the new contents; because
    /// it copied, it sees what was registered. This is the assertion that
    /// keeps #9188's split honest: delete the copy and this test fails
    /// deterministically instead of turning into latent UB in a provider image.
    #[test]
    fn copying_name_entry_point_owns_its_bytes() {
        let mut buffer = b"registeredName".to_vec();
        unsafe {
            js_register_function_name(KEY_NAME_COPIED, buffer.as_ptr(), buffer.len() as u32);
        }
        buffer.copy_from_slice(b"CLOBBEREDbytes");

        assert_eq!(
            function_name_for_ptr(KEY_NAME_COPIED as usize).as_deref(),
            Some("registeredName"),
            "js_register_function_name must copy: the registry returned the \
             caller's post-registration buffer contents, i.e. it kept a borrow"
        );
    }

    /// The same sabotage for source text, which is the larger of the two
    /// copies and carries a flag alongside the bytes.
    #[test]
    fn copying_source_entry_point_owns_its_bytes() {
        let mut buffer = b"function f() { return 1; }".to_vec();
        unsafe {
            js_register_function_source(KEY_SOURCE_COPIED, buffer.as_ptr(), buffer.len() as u32, 1);
        }
        buffer.copy_from_slice(b"function g() { return 2; }");

        assert_eq!(
            function_source_for_ptr(KEY_SOURCE_COPIED as usize).as_deref(),
            Some("function f() { return 1; }"),
            "js_register_function_source must copy its bytes"
        );
        assert!(
            function_is_non_strict_ordinary_for_ptr(KEY_SOURCE_COPIED as usize),
            "the kind bit travels with the copied source"
        );
    }

    /// The borrowing entry points are what codegen emits, so they have to
    /// round-trip through the same readers.
    #[test]
    fn static_entry_points_round_trip() {
        unsafe {
            js_register_function_name_static(KEY_NAME_STATIC, b"imageName".as_ptr(), 9);
            js_register_function_source_static(
                KEY_SOURCE_STATIC,
                b"function h() {}".as_ptr(),
                15,
                0,
            );
        }
        assert_eq!(
            function_name_for_ptr(KEY_NAME_STATIC as usize).as_deref(),
            Some("imageName")
        );
        assert_eq!(
            function_source_for_ptr(KEY_SOURCE_STATIC as usize).as_deref(),
            Some("function h() {}")
        );
        assert!(!function_is_non_strict_ordinary_for_ptr(
            KEY_SOURCE_STATIC as usize
        ));
        assert_eq!(
            function_source_for_func_ptr(KEY_SOURCE_STATIC as usize),
            "function h() {}"
        );
    }

    #[repr(C)]
    struct BatchImage {
        names: [FunctionNameDescriptor; 2],
        sources: [FunctionSourceDescriptor; 1],
        storage: [u8; 96],
    }

    fn relative_offset(base: *const u8, target: *const u8) -> i32 {
        let difference = (target as isize) - (base as isize);
        i32::try_from(difference).expect("test image is one small allocation")
    }

    #[test]
    fn generated_descriptor_layout_matches_codegen() {
        assert_eq!(std::mem::size_of::<FunctionNameDescriptor>(), 12);
        assert_eq!(std::mem::align_of::<FunctionNameDescriptor>(), 4);
        assert_eq!(std::mem::size_of::<FunctionSourceDescriptor>(), 16);
        assert_eq!(std::mem::align_of::<FunctionSourceDescriptor>(), 4);
    }

    /// The generated descriptor ABI is relative to each table base, preserves
    /// registration order for duplicate keys, and carries the source kind bit.
    #[test]
    fn static_batch_descriptors_round_trip_and_last_write_wins() {
        let image = Box::leak(Box::new(BatchImage {
            names: [FunctionNameDescriptor {
                function_offset: 0,
                bytes_offset: 0,
                byte_len: 0,
            }; 2],
            sources: [FunctionSourceDescriptor {
                function_offset: 0,
                bytes_offset: 0,
                byte_len: 0,
                is_non_strict_ordinary: 0,
            }],
            storage: [0; 96],
        }));
        image.storage[0..5].copy_from_slice(b"first");
        image.storage[8..14].copy_from_slice(b"second");
        image.storage[16..31].copy_from_slice(b"function b() {}");

        let name_base = image.names.as_ptr().cast::<u8>();
        let source_base = image.sources.as_ptr().cast::<u8>();
        let key = image.storage[80..].as_ptr();
        image.names[0] = FunctionNameDescriptor {
            function_offset: relative_offset(name_base, key),
            bytes_offset: relative_offset(name_base, image.storage.as_ptr()),
            byte_len: 5,
        };
        image.names[1] = FunctionNameDescriptor {
            function_offset: relative_offset(name_base, key),
            bytes_offset: relative_offset(name_base, image.storage[8..].as_ptr()),
            byte_len: 6,
        };
        image.sources[0] = FunctionSourceDescriptor {
            function_offset: relative_offset(source_base, key),
            bytes_offset: relative_offset(source_base, image.storage[16..].as_ptr()),
            byte_len: 15,
            is_non_strict_ordinary: 1,
        };

        unsafe {
            js_register_function_names_static(name_base, image.names.len() as u32);
            js_register_function_sources_static(source_base, image.sources.len() as u32);
        }
        assert_eq!(
            function_name_for_ptr(key as usize).as_deref(),
            Some("second")
        );
        assert_eq!(
            function_source_for_ptr(key as usize).as_deref(),
            Some("function b() {}")
        );
        assert!(function_is_non_strict_ordinary_for_ptr(key as usize));
    }

    #[test]
    fn compiled_info_packed_source_round_trips_without_a_registry_entry() {
        const SOURCE: &[u8; 15] = b"function h() {}";
        let mut info =
            unsafe { crate::closure::JsFunctionInfo::from_code(0x1234usize as *const u8, 0) }
                .with_flags(
                    crate::codegen_abi::FN_COMPILED_BODY
                        | crate::codegen_abi::FN_HAS_SOURCE
                        | crate::codegen_abi::FN_NON_STRICT_ORDINARY,
                );
        info.versioned_captures = SOURCE.len() as u32;
        info.versioned_boxed_mask = crate::codegen_abi::JS_FUNCTION_INFO_SIZE as u64;
        let fixture = InfoWithInlineSource {
            // The address is only a registry key/native-form fallback here.
            info,
            bytes: *SOURCE,
        };
        assert_eq!(
            retained_source_for_info(&fixture.info).as_deref(),
            std::str::from_utf8(SOURCE).ok()
        );
        assert_ne!(fixture.info.flags & crate::codegen_abi::FN_HAS_SOURCE, 0);
        assert_ne!(
            fixture.info.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY,
            0
        );
        assert_eq!(std::mem::size_of_val(&fixture.info), 64);
    }

    /// Borrowed and owned names live in different maps, so a reader has to
    /// pick one when both hold a usable name for the same address. An owned
    /// entry can only come from an explicit runtime registration, which is the
    /// more specific statement, so it wins — in either registration order.
    #[test]
    fn copied_name_takes_precedence_over_the_image_name() {
        unsafe {
            js_register_function_name_static(KEY_NAME_PRECEDENCE, b"fromImage".as_ptr(), 9);
        }
        assert_eq!(
            function_name_for_ptr(KEY_NAME_PRECEDENCE as usize).as_deref(),
            Some("fromImage")
        );

        let runtime_name = b"fromRuntime".to_vec();
        unsafe {
            js_register_function_name(
                KEY_NAME_PRECEDENCE,
                runtime_name.as_ptr(),
                runtime_name.len() as u32,
            );
        }
        assert_eq!(
            function_name_for_ptr(KEY_NAME_PRECEDENCE as usize).as_deref(),
            Some("fromRuntime")
        );

        // And a later image registration does not undo it.
        unsafe {
            js_register_function_name_static(KEY_NAME_PRECEDENCE, b"fromImage2".as_ptr(), 10);
        }
        assert_eq!(
            function_name_for_ptr(KEY_NAME_PRECEDENCE as usize).as_deref(),
            Some("fromRuntime")
        );
    }

    /// `register_function_name_if_absent` asks the read side whether a name
    /// exists, so a name registered into EITHER map counts as present. Before
    /// the split it only consulted the image map, which is the shape of bug
    /// that lets an inferred name shadow a real one.
    #[test]
    fn if_absent_respects_a_name_in_either_map() {
        register_function_name_if_absent(KEY_NAME_IF_ABSENT as usize, "inferred");
        assert_eq!(
            function_name_for_ptr(KEY_NAME_IF_ABSENT as usize).as_deref(),
            Some("inferred")
        );

        // A second inference does not replace the first.
        register_function_name_if_absent(KEY_NAME_IF_ABSENT as usize, "inferredAgain");
        assert_eq!(
            function_name_for_ptr(KEY_NAME_IF_ABSENT as usize).as_deref(),
            Some("inferred")
        );

        // Nor does an image name displace an already-registered one.
        unsafe {
            js_register_function_name_static(KEY_NAME_IF_ABSENT, b"fromImage".as_ptr(), 9);
        }
        register_function_name_if_absent(KEY_NAME_IF_ABSENT as usize, "inferredThird");
        assert_eq!(
            function_name_for_ptr(KEY_NAME_IF_ABSENT as usize).as_deref(),
            Some("inferred")
        );
    }

    #[test]
    fn metadata_ascii_views_match_utf8_validation_at_every_exit() {
        for len in 0..128 {
            let bytes: Vec<_> = (0..len).map(|i| ((i * 37) % 128) as u8).collect();
            assert_eq!(
                decode_registered_bytes(&bytes),
                std::str::from_utf8(&bytes).ok()
            );
        }
        for bytes in [
            "é".as_bytes(),
            "界😀".as_bytes(),
            b"a\0z",
            b"\xff",
            b"a\xc2",
            b"\x80",
            b"\xed\xa0\x80",
            b"\xf4\x90\x80\x80",
        ] {
            assert_eq!(
                decode_registered_bytes(bytes),
                std::str::from_utf8(bytes).ok()
            );
        }
    }

    #[test]
    fn if_absent_preserves_valid_name_behind_invalid_override() {
        let key = 0x9188_0171usize;
        function_name_registry()
            .lock()
            .unwrap()
            .insert(key, b"imageName");
        function_name_overrides()
            .lock()
            .unwrap()
            .insert(key, std::sync::Arc::from(&b"\xff"[..]));
        register_function_name_if_absent(key, "inferred");
        assert_eq!(function_name_for_ptr(key).as_deref(), Some("imageName"));
        assert_eq!(
            &**function_name_overrides().lock().unwrap().get(&key).unwrap(),
            b"\xff"
        );

        function_name_registry()
            .lock()
            .unwrap()
            .insert(key, b"\xff");
        function_name_overrides()
            .lock()
            .unwrap()
            .insert(key, std::sync::Arc::from(&b"ownedName"[..]));
        register_function_name_if_absent(key, "inferred");
        assert_eq!(function_name_for_ptr(key).as_deref(), Some("ownedName"));
    }

    #[test]
    fn inference_preserves_a_decodable_image_name_after_an_invalid_override() {
        let key = 0x9188_0010usize as *const u8;
        unsafe {
            js_register_function_name_static(key, "image世界".as_ptr(), "image世界".len() as u32);
            js_register_function_name(key, [0xffu8].as_ptr(), 1);
        }
        register_function_name_if_absent(key as usize, "inferred");
        assert_eq!(
            function_name_for_ptr(key as usize).as_deref(),
            Some("image世界")
        );

        let absent = 0x9188_0011usize as *const u8;
        unsafe { js_register_function_name_static(absent, [0xffu8].as_ptr(), 1) };
        register_function_name_if_absent(absent as usize, "inferred世界");
        assert_eq!(
            function_name_for_ptr(absent as usize).as_deref(),
            Some("inferred世界")
        );

        let invalid_owned = 0x9188_0012usize as *const u8;
        unsafe { js_register_function_name(invalid_owned, [0xffu8].as_ptr(), 1) };
        register_function_name_if_absent(invalid_owned as usize, "recovered世界");
        assert_eq!(
            function_name_for_ptr(invalid_owned as usize).as_deref(),
            Some("recovered世界")
        );
    }

    /// The `.stack` resolver reads both maps through one snapshot, and sizes
    /// its staleness check off `function_name_registry_len`. A name that the
    /// snapshot carries but the length does not count is a name the resolver
    /// caches and never refreshes.
    #[test]
    fn snapshot_and_len_cover_both_maps() {
        let before = function_name_registry_len().expect("registry lock");
        let key_image: *const u8 = 0x9188_0007 as *const u8;
        let key_owned: *const u8 = 0x9188_0008 as *const u8;
        let owned = b"ownedFrame".to_vec();
        unsafe {
            js_register_function_name_static(key_image, b"imageFrame".as_ptr(), 10);
            js_register_function_name(key_owned, owned.as_ptr(), owned.len() as u32);
        }
        // At least two more: the registry is process-wide, and a test running
        // beside this one may register names of its own meanwhile. Each of the
        // two registrations above lands in a different map, and the length is
        // their sum.
        assert!(
            function_name_registry_len().expect("registry lock") >= before + 2,
            "both maps count toward the resolver's staleness check"
        );

        let entries = function_name_registry_entries().expect("registry lock");
        let find = |key: *const u8| {
            entries
                .iter()
                .find(|(addr, _)| *addr == key as usize)
                .map(|(_, name)| String::from_utf8_lossy(name).into_owned())
        };
        assert_eq!(find(key_image).as_deref(), Some("imageFrame"));
        assert_eq!(find(key_owned).as_deref(), Some("ownedFrame"));
    }
}

#[cfg(test)]
mod compressed_source_tests {
    use super::*;
    #[repr(C)]
    struct Image {
        info: crate::closure::JsFunctionInfo,
        tail: [u32; 4],
        frame: [u8; 128],
    }
    extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
        0.0
    }
    #[test]
    fn info_reads_the_same_nested_utf8_range_with_and_without_a_clone() {
        let source = "function nested() { /*世界😀*/ return '\\ud800'; }";
        let raw = format!("xx{source}tail");
        for versioned in [false, true] {
            let mut image = Image {
                info: crate::closure::JsFunctionInfo::of(
                    body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
                ),
                tail: [0; 4],
                frame: [0; 128],
            };
            image.info.flags |=
                crate::codegen_abi::FN_HAS_SOURCE | crate::codegen_abi::FN_SOURCE_BLOCKS;
            let len = raw.len() as u16;
            image.frame[..4].copy_from_slice(&(u32::from(len) + 5).to_le_bytes());
            image.frame[4..8].copy_from_slice(&u32::from(len).to_le_bytes());
            image.frame[8] = 1;
            image.frame[9..11].copy_from_slice(&len.to_le_bytes());
            image.frame[11..13].copy_from_slice(&(!len).to_le_bytes());
            image.frame[13..13 + raw.len()].copy_from_slice(raw.as_bytes());
            let delta = (image.frame.as_ptr() as isize - &image.info as *const _ as isize) as i32;
            if versioned {
                image.info.versioned_code = body as *const u8;
                image.tail = [delta as u32, 2, source.len() as u32, 0];
            } else {
                let mut words = [0; 8];
                words[..4].copy_from_slice(&delta.to_ne_bytes());
                words[4..].copy_from_slice(&2u32.to_ne_bytes());
                image.info.versioned_boxed_mask = u64::from_ne_bytes(words);
                image.info.versioned_captures = source.len() as u32;
            }
            assert_eq!(
                retained_source_for_info(&image.info).as_deref(),
                Some(source)
            );
            assert_eq!(
                crate::codegen_abi::function_source_tail_size(&image.info),
                if versioned { 16 } else { 0 }
            );
        }
    }
}
