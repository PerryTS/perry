//! Contract witness for prototype constants (object model step 3,
//! `OBJECT_MODEL_2026-10-13.md` §3.6; decision 129 §4.5 contract builds).
//!
//! Codegen binds a class method call directly when the receiver's family set
//! is proven and the program's prototype-write census proves the method slot
//! constant (`perry-codegen/src/proto_constants.rs`). Those are compile-time
//! facts; nothing is checked at run time in a release build. A contract-check
//! build (`PERRY_CHECK_CONTRACTS=1` while compiling) calls
//! [`js_proto_constant_contract_check`] at every such call, which re-derives
//! both facts on the live receiver: its class id is one the site was compiled
//! for, it has no own property of the method's name, and its prototype chain
//! resolves the name to a data property holding a function object of exactly
//! the bound body. Any difference reports through the runtime's one
//! `contract_violated`.

use super::*;
use crate::object::native_call_method::class_holder::{chain_method, ChainMethod, MethodKey};

/// The receiver as the witness asks about it: a live object (not forwarded).
/// Not the method site's `ordinary_receiver`, which also refuses objects a
/// site must not learn from (outside the arena's generations, a layout with
/// holes or overflow): those are still objects the bound call is made on,
/// and the checks below re-derive everything the binding relied on.
unsafe fn object_receiver(addr: usize) -> Option<*const ObjectHeader> {
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    (crate::gc_type_is_object!(header.obj_type)
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0)
        .then_some(addr as *const ObjectHeader)
}

/// `PERRY_CHECK_CONTRACTS=1`: the run-time witness of a prototype-constant
/// call. `class_id` is the class the call was bound for (0: any), `info` the
/// bound body's `JsFunctionInfo`.
///
/// # Safety
/// `recv` is any value; `method_id` a static dispatch id.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn js_proto_constant_contract_check(
    recv: f64,
    method_id: i64,
    class_id: i32,
    info: u64,
) {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
        .map(|r| std::slice::from_raw_parts(r.ptr, r.len))
        .unwrap_or(b"?");
    let addr = (recv.to_bits() & crate::value::POINTER_MASK) as usize;
    let Some(obj) = super::is_site_receiver(recv)
        .then(|| object_receiver(addr))
        .flatten()
    else {
        abort(
            recv,
            name,
            class_id,
            info,
            "receiver is not an ordinary object",
        );
    };
    if class_id != 0 && (*obj).class_id != class_id as u32 {
        abort(
            recv,
            name,
            class_id,
            info,
            "receiver class is outside the proven family",
        );
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let key = crate::string::canonical_key(name);
    if crate::object::obj_value_has_own_key(recv, crate::value::js_nanbox_string(key as i64)) {
        abort(
            recv,
            name,
            class_id,
            info,
            "receiver has an own property of the method name",
        );
    }
    let proto = crate::object::js_object_get_prototype_of(recv).to_bits();
    let start = (proto & !crate::value::POINTER_MASK == crate::value::POINTER_TAG)
        .then(|| (proto & crate::value::POINTER_MASK) as *const ObjectHeader)
        .filter(|p| !p.is_null());
    let Some(start) = start else {
        abort(recv, name, class_id, info, "receiver has no prototype");
    };
    let ChainMethod::Data { value, .. } = chain_method(start, &MethodKey::bytes(name)) else {
        abort(
            recv,
            name,
            class_id,
            info,
            "chain does not resolve a data method",
        );
    };
    let f = (value & crate::value::POINTER_MASK) as usize;
    let live = if value & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::value::addr_class::is_above_handle_band(f)
        && crate::closure::is_closure_ptr(f)
    {
        (*(f as *const crate::closure::ClosureHeader)).info as u64
    } else {
        0
    };
    if live != info {
        abort(
            recv,
            name,
            class_id,
            info,
            "the holder slot holds another body",
        );
    }
}

#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn abort(recv: f64, name: &[u8], class_id: i32, info: u64, why: &str) -> ! {
    crate::entry_contracts::contract_violated(
        "proto_constant",
        "family(receiver) in proven set && holder slot == constant",
        "prototype-write-census",
        recv.to_bits(),
        &format!(
            " method={} why={why} class_id={class_id} body={info:#x}",
            String::from_utf8_lossy(name)
        ),
    )
}

/// `PERRY_CHECK_CONTRACTS=1`: the witness of an inherited read rewritten to
/// its known holder. The live receiver's [[Prototype]] must be `holder` and
/// the receiver must have no own property of the key.
///
/// # Safety
/// `recv`/`holder` are any values; `method_id` a static dispatch id.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn js_proto_holder_contract_check(
    recv: f64,
    holder: f64,
    method_id: i64,
) {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
        .map(|r| std::slice::from_raw_parts(r.ptr, r.len))
        .unwrap_or(b"?");
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = crate::object::js_object_get_prototype_of(recv);
    if proto.to_bits() != holder.to_bits() {
        abort(
            recv,
            name,
            0,
            0,
            "receiver's prototype is not the known holder",
        );
    }
    let key = crate::string::canonical_key(name);
    if crate::object::obj_value_has_own_key(recv, crate::value::js_nanbox_string(key as i64)) {
        abort(recv, name, 0, 0, "receiver has an own property of the key");
    }
}

/// `PERRY_CHECK_CONTRACTS=1`: the witness of a site that tests the shadow
/// bit (object model §3.6). `ok` is the emitted test's answer. When it
/// passed (the receiver's layout word carries no shadow bit), the receiver
/// must have no own property of the bound name: every write that gives an
/// object an own property of a shadow name sets the bit
/// (`object::shadow`).
///
/// # Safety
/// `recv` is any value; `method_id` a static dispatch id.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn js_proto_shadow_contract_check(ok: i32, recv: f64, method_id: i64) {
    if ok == 0 || !super::is_site_receiver(recv) {
        return;
    }
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let name = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
        .map(|r| std::slice::from_raw_parts(r.ptr, r.len))
        .unwrap_or(b"?");
    let _no_move = crate::gc::GcSuppressScope::new();
    let key = crate::string::canonical_key(name);
    if crate::object::obj_value_has_own_key(recv, crate::value::js_nanbox_string(key as i64)) {
        abort(
            recv,
            name,
            0,
            0,
            "an unshadowed receiver has an own property of the bound name",
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn shadow_contract_rejects_false_answer() {
        const CHILD: &str = "PERRY_TEST_PROTO_SHADOW_FALSE_ANSWER";
        if std::env::var_os(CHILD).is_some() {
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
            let key = scope.root_string_ptr(crate::string::canonical_key(b"m"));
            obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
                key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    crate::object::js_object_set_field_by_name(obj, key, 1.0);
                })
            });
            let value = obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
                crate::value::js_nanbox_pointer(obj as i64)
            });
            let method_id = key.with_const_ptr::<crate::StringHeader, _>(|key| key as i64);
            unsafe { super::js_proto_shadow_contract_check(1, value, method_id) };
            panic!("a false shadow answer returned");
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "object::method_site::proto_constant::tests::shadow_contract_rejects_false_answer",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(output.status.signal(), Some(libc::SIGABRT));
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("an unshadowed receiver has an own property"));
    }
}
