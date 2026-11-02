//! `Pristine(namespace)`: the shadow bit of a builtin namespace (#12643).
//!
//! Lowering binds a direct namespace call (`Math.max(a, b)`,
//! `Object.keys(o)`, `JSON.stringify(v)`, `console.log(x)`) to its intrinsic.
//! When non-constant dynamic code can reach the global scope, that code may
//! replace the member or the namespace itself, so the site is guarded: it
//! loads the namespace's holder from its agent slot
//! (`AGENT_PTR_INTRINSIC_PROTO + BUILTIN_PROTO_FAMILIES + i`, the same block
//! as the prototype families of `builtin_proto`) and tests bit 30 of its
//! layout word. Clear: the intrinsic arm. Set: the generic call, which reads
//! `globalThis[name][member]` and calls it.
//!
//! # The holder
//!
//! The object holding the namespace's own members: the namespace object
//! itself (`Math`, `JSON`, `Reflect`, `Atomics`, `Intl`, `console`), or a
//! constructor's property bag (`Object`, `Array`, `Number`, ... statics live
//! in `ClosureHeader::props`, an ordinary object). [`seal_namespaces`] seals
//! each holder as intrinsic family `BUILTIN_PROTO_FAMILIES + i` and the realm
//! global as [`GLOBAL_FAMILY`] once the realm is populated; from then on the
//! ordinary write funnels (`builtin_proto::note_write` and the value-store
//! hook) report every write, define, delete and relink of a holder here.
//!
//! # Who sets the bit
//!
//! * a write of any kind to a holder sets the holder's own bit;
//! * a value store into the realm global's slot of a namespace name, and any
//!   structural change of the global after which `globalThis[name]` is no
//!   longer a data property naming the namespace (a delete, an accessor, a
//!   redefinition), set that namespace's bit: the binding is part of the
//!   fact, and the global's other writes leave every namespace alone.
//!
//! The bit is never cleared (restoring a member does not reopen the fact),
//! survives restamps (`layout_desc::carry_shadow`), and nothing else records
//! anything. A holder whose word cannot carry the bit (not a site-matchable
//! layout) has its slot pointed at `PERRY_PATCHED_INTRINSIC`, whose word is
//! shadowed: its sites take the generic arm.

use super::ObjectHeader;
use crate::codegen_abi::LayoutWord;

/// The first namespace family (namespace `i` is `FIRST + i`).
pub(crate) const FIRST: usize = crate::codegen_abi::BUILTIN_PROTO_FAMILIES;
/// Number of builtin namespaces.
pub(crate) const COUNT: usize = crate::codegen_abi::BUILTIN_NAMESPACES.len();
/// The realm global's sealed family: its writes are attributed to namespaces.
pub(crate) const GLOBAL_FAMILY: usize = FIRST + COUNT;
const _: () = assert!(GLOBAL_FAMILY as u64 <= super::meta_flags::OBJECT_META_SEALED_FAMILY_MASK);
const _: () = assert!(FIRST + COUNT == crate::codegen_abi::INTRINSIC_FAMILIES);

/// The holder of namespace `i`'s members, named by its agent slot.
#[inline(always)]
fn holder(i: usize) -> *const ObjectHeader {
    crate::agent_ptrs::intrinsic_proto(FIRST + i)
}

/// Is the slot of namespace `i` still a real holder (not a stand-in)?
#[inline]
fn holder_is_live(p: *const ObjectHeader) -> bool {
    !std::ptr::eq(
        p.cast::<u64>(),
        crate::agent_ptrs::PERRY_PRISTINE_PROTO.as_ptr(),
    ) && !std::ptr::eq(
        p.cast::<u64>(),
        crate::agent_ptrs::PERRY_PATCHED_INTRINSIC.as_ptr(),
    )
}

/// `Pristine(namespace i)`: the holder's word is below `UNSHADOWED_WORD_END`.
#[inline(always)]
pub(crate) fn pristine(i: usize) -> bool {
    // SAFETY: the slot names a live holder of this agent or a static stand-in.
    unsafe { (*holder(i)).parent_class_id < super::layout_desc::UNSHADOWED_WORD_END }
}

/// The holder of a namespace VALUE: an ordinary object is its own holder, a
/// function its property bag (null when it has none).
unsafe fn holder_of_value(bits: u64) -> *mut ObjectHeader {
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return std::ptr::null_mut();
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if crate::closure::is_closure_ptr(addr) {
        return crate::closure::props::bag_of(addr);
    }
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(h) if crate::gc_type_is_object!(h.obj_type) => addr as *mut ObjectHeader,
        _ => std::ptr::null_mut(),
    }
}

/// `obj`'s own keys and inline bound from its LAYOUT id: an overflow (which
/// a dynamic key add installs on the global) hides the shape record from
/// `object_keys_and_live_slot_count`, not the layout's keys.
unsafe fn layout_keys(obj: *const ObjectHeader) -> (super::ObjectKeys, u32) {
    let id = super::layout_desc::layout_id_of_word((*obj).parent_class_id);
    match super::shapes::shape_record_for_word(LayoutWord(id)) {
        Some(record) => (
            super::object_keys_from_shape_record(obj, record),
            record.live_inline_slot_count(),
        ),
        None => (super::ObjectKeys::NONE, 0),
    }
}

/// `obj[name]` as an own data value, without running user code: `None` when
/// `obj` holds no such key or holds an accessor.
unsafe fn own_data(obj: *const ObjectHeader, name: &[u8]) -> Option<u64> {
    let (keys, live) = layout_keys(obj);
    if keys.is_null() {
        return None;
    }
    let pos = super::keys_find_property_slot_by_bytes(keys, keys.count(), name)?;
    if super::key_attrs::key_is_accessor_at(keys.arr(), pos) {
        return None;
    }
    let bits =
        super::field_get_set::object_key_field_at_with_keys(obj, keys.arr(), pos, live).bits();
    (bits != crate::value::TAG_HOLE).then_some(bits)
}

/// `globalThis[name]` as a data value (see [`own_data`]).
unsafe fn global_data_binding(global: *const ObjectHeader, name: &str) -> Option<u64> {
    own_data(global, name.as_bytes())
}

/// Seal every namespace holder of THIS thread's realm and the realm global
/// (`builtin_proto::seal_realm`, under its builtin-definition scope). A `vm`
/// realm's namespaces are its own and are never published.
pub(crate) fn seal_namespaces(global: *mut ObjectHeader) {
    if !super::global_this::is_thread_realm_global(global) {
        return;
    }
    // SAFETY: the populated realm global, under the caller's no-move scope.
    unsafe {
        for (i, name) in crate::codegen_abi::BUILTIN_NAMESPACES.iter().enumerate() {
            let Some(bits) = global_data_binding(global, name) else {
                continue;
            };
            // A constructor without statics has no bag yet: give it one, so
            // a later write lands in a sealed holder.
            let addr = (bits & crate::value::POINTER_MASK) as usize;
            if bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
                && crate::closure::is_closure_ptr(addr)
            {
                crate::closure::props::bag_ensure(addr);
            }
            let obj = holder_of_value(bits);
            if obj.is_null() {
                continue;
            }
            super::builtin_proto::seal_object_family(obj, FIRST + i);
            if contract_mode() {
                snapshot_members(i, obj);
            }
            trace(name, "sealed", (*obj).parent_class_id);
            if super::shapes::is_site_matchable_layout_word((*obj).parent_class_id) {
                crate::agent_ptrs::publish_intrinsic_proto(FIRST + i, obj);
            } else {
                // Its word cannot carry the bit: never pristine.
                crate::agent_ptrs::publish_patched_intrinsic(FIRST + i);
            }
        }
        super::builtin_proto::seal_object_family(global, GLOBAL_FAMILY);
    }
}

/// A write reached a sealed holder or the realm global
/// (`builtin_proto::note_sealed_write`). `field` is the inline or spill slot
/// a value store is about to write.
///
/// # Safety
/// `obj` is the live sealed object being written.
#[cold]
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn note_write(
    obj: *mut ObjectHeader,
    family: usize,
    field: Option<usize>,
    site: &'static str,
) {
    if super::own_override::in_builtin_definition() {
        return;
    }
    if family < GLOBAL_FAMILY {
        withdraw(family - FIRST, site);
        return;
    }
    // The realm global: only a namespace binding matters.
    let (keys, _) = layout_keys(obj);
    for (i, name) in crate::codegen_abi::BUILTIN_NAMESPACES.iter().enumerate() {
        if !pristine(i) || !holder_is_live(holder(i)) {
            continue;
        }
        let rebound = match field {
            // A value store: the slot of `name`, about to take a new value.
            Some(slot) => {
                !keys.is_null()
                    && super::keys_find_property_slot_by_bytes(keys, keys.count(), name.as_bytes())
                        .is_some_and(|pos| {
                            super::key_attrs::key_storage_slot(keys.arr(), pos) as usize == slot
                        })
            }
            // A structural change (key add or delete, descriptor, overflow,
            // relink): does the global still bind `name` to the namespace?
            // A funnel that reports before mutating sees the binding intact;
            // the publication after the change is reported again.
            None => match global_data_binding(obj, name) {
                Some(bits) => holder_of_value(bits) as *const ObjectHeader != holder(i),
                None => true,
            },
        };
        if rebound {
            withdraw(i, site);
        }
    }
}

/// Set namespace `i`'s bit (or point its slot at the patched stand-in when its
/// holder's word cannot carry it).
#[cold]
#[inline(never)]
unsafe fn withdraw(i: usize, site: &'static str) {
    let p = holder(i) as *mut ObjectHeader;
    if !holder_is_live(p) {
        return;
    }
    let fresh = pristine(i);
    super::shadow::set_bit(p);
    if pristine(i) {
        crate::agent_ptrs::publish_patched_intrinsic(FIRST + i);
    }
    if fresh {
        trace(
            crate::codegen_abi::BUILTIN_NAMESPACES[i],
            site,
            (*p).parent_class_id,
        );
    }
}

/// `PERRY_BUILTIN_PROTO_TRACE=1`: each holder's sealed word and each first
/// withdrawal with its funnel. A program that patches nothing prints only the
/// seals.
#[cold]
#[inline(never)]
fn trace(name: &str, what: &str, word: u32) {
    // SAFETY: a NUL-terminated literal; `getenv` only reads the environment.
    if unsafe { libc::getenv(c"PERRY_BUILTIN_PROTO_TRACE".as_ptr()) }.is_null() {
        return;
    }
    eprintln!("[builtin-namespace] {name} {what} word={word:#x}");
}

/// A guarded namespace site's generic arm, where the callee is read (before
/// the arguments): `globalThis[name]`, the call's `this`.
#[no_mangle]
pub extern "C-unwind" fn js_builtin_namespace_value(family: u64) -> f64 {
    let family = family as usize;
    let name = match family.checked_sub(FIRST) {
        Some(i) if i < COUNT => crate::codegen_abi::BUILTIN_NAMESPACES[i],
        _ => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let global = crate::object::js_get_global_this();
    // SAFETY: the realm global; the name is a static literal.
    unsafe { super::method_call::spec_get_value(global, name.as_bytes()) }
}

/// The generic arm's callee: `ns[member]` (a nullish `ns` throws node's
/// TypeError). The arm then evaluates the arguments and calls the value with
/// `ns` as `this` (`js_method_call_value`).
///
/// # Safety
/// `member_ptr` holds `member_len` bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_builtin_namespace_member(
    ns: f64,
    member_ptr: *const u8,
    member_len: usize,
) -> f64 {
    super::method_call::spec_get_value(ns, std::slice::from_raw_parts(member_ptr, member_len))
}

/// Contract mode (decision 129 §4.5): a guarded namespace site whose test
/// admitted the intrinsic arm calls this first (`PERRY_CHECK_CONTRACTS` at
/// compile time). A clear bit must mean `globalThis[name]` still names the
/// namespace and `member` is the function the realm sealed (contract runs
/// record it, [`snapshot_members`]) and not an accessor; a member the runtime
/// answers by native dispatch, as on `console`, is absent.
#[no_mangle]
pub extern "C" fn js_builtin_namespace_contract_check(
    family: u64,
    member_ptr: *const u8,
    member_len: usize,
) {
    let family = family as usize;
    if !(FIRST..GLOBAL_FAMILY).contains(&family) {
        return;
    }
    let i = family - FIRST;
    if !pristine(i) {
        violated(i, b"", "the site ran the intrinsic arm with the bit set");
    }
    let p = holder(i);
    if !holder_is_live(p) {
        // Not sealed yet: nothing can have reached the namespace.
        return;
    }
    let name = crate::codegen_abi::BUILTIN_NAMESPACES[i];
    // SAFETY: the realm global and a live holder; `member` is the site's
    // static name.
    unsafe {
        let global = crate::object::js_get_global_this();
        let global = (global.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
        match global_data_binding(global, name) {
            Some(bits) if holder_of_value(bits) as *const ObjectHeader == p => {}
            _ => violated(i, b"", "globalThis no longer binds the namespace"),
        }
        let member = std::slice::from_raw_parts(member_ptr, member_len);
        let (keys, _) = layout_keys(p);
        let present = !keys.is_null()
            && super::keys_find_property_slot_by_bytes(keys, keys.count(), member).is_some();
        let now = match own_data(p, member) {
            Some(bits) => function_identity(bits),
            None if present => violated(i, member, "the member is an accessor"),
            None => None,
        };
        let sealed = SNAPSHOT.with(|s| {
            s.borrow()
                .iter()
                .find(|(f, name, _)| *f == i && name.as_slice() == member)
                .map(|(_, _, id)| *id)
        });
        // A sealed function member must still be that function; a data
        // member (or one the realm never had) is not checked here.
        if let Some(sealed) = sealed.filter(|id| id.is_some()) {
            if now != sealed {
                violated(i, member, "the member is not the function the realm sealed");
            }
        }
    }
}

#[cold]
fn violated(i: usize, member: &[u8], why: &str) -> ! {
    eprintln!(
        "[contract] Pristine({}) violated at .{}: {why}",
        crate::codegen_abi::BUILTIN_NAMESPACES[i],
        String::from_utf8_lossy(member)
    );
    std::process::abort();
}

/// Contract runs (`PERRY_CHECK_CONTRACTS` in the environment) record, at
/// sealing, the function identity (its static `JsFunctionInfo`) of every
/// function-valued member, so a guarded site's check can compare the member
/// it is about to bypass with the one the realm installed. Never consulted
/// by anything but [`js_builtin_namespace_contract_check`].
fn contract_mode() -> bool {
    // SAFETY: a NUL-terminated literal; `getenv` only reads the environment.
    let v = unsafe { libc::getenv(c"PERRY_CHECK_CONTRACTS".as_ptr()) };
    // SAFETY: a non-null `getenv` result is a NUL-terminated string.
    !v.is_null()
        && !matches!(
            unsafe { std::ffi::CStr::from_ptr(v) }.to_bytes(),
            b"" | b"0"
        )
}

thread_local! {
    /// (namespace, member, function identity) at sealing; contract runs only.
    static SNAPSHOT: std::cell::RefCell<Vec<(usize, Vec<u8>, Option<usize>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The static function record of a closure value (stable across moves).
fn function_identity(bits: u64) -> Option<usize> {
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    crate::closure::closure_info(ptr as *const crate::closure::ClosureHeader)
        .map(|info| info as *const crate::closure::JsFunctionInfo as usize)
}

/// Record namespace `i`'s function members (contract runs).
unsafe fn snapshot_members(i: usize, holder: *mut ObjectHeader) {
    let names =
        super::js_object_get_own_property_names(crate::value::js_nanbox_pointer(holder as i64));
    let names = (names.to_bits() & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader;
    if names.is_null() {
        return;
    }
    let mut out = Vec::new();
    for k in 0..crate::array::js_array_length(names) {
        let name = crate::array::js_array_get_f64(names, k);
        let s = crate::value::js_jsvalue_to_string(name);
        if s.is_null() {
            continue;
        }
        let bytes =
            std::slice::from_raw_parts(super::string_header_payload(s), (*s).byte_len as usize)
                .to_vec();
        if let Some(bits) = own_data(holder, &bytes) {
            out.push((i, bytes, function_identity(bits)));
        }
    }
    SNAPSHOT.with(|s| s.borrow_mut().extend(out));
}

#[cfg(test)]
#[path = "builtin_namespace_tests.rs"]
mod tests;
