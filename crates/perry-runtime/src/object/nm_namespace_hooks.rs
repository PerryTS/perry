//! Armed dispatch for native-module NAMESPACE object behaviors (binary size).
//!
//! Generic object paths (`Object.getOwnPropertyDescriptor`, dynamic field
//! stores, `Reflect.defineProperty` redefine checks, own-key enumeration)
//! special-case objects with `class_id == NATIVE_MODULE_CLASS_ID`. Referencing
//! the native-module implementations statically from those always-linked
//! paths pinned the entire module-namespace web — `callable_exports`,
//! `constants`, the submodule export tables, and through them the
//! stream/sqlite/buffer/assert surfaces — into every binary, including ones
//! that import no module at all (measured on a `console.log` hello world via
//! `ld64 -why_live`).
//!
//! Instead, the generic paths call through this armed function-pointer table.
//! `install_native_module_vtable()` — reached from every path that can mint a
//! namespace-classed object, and nothing else — arms it, so:
//! - a program with no native-module import never links the implementations
//!   (the ops static lives on the native-module side and is dead with it);
//! - a program that CAN observe a namespace object arms the table before the
//!   first such object exists, so behavior is identical to the old static
//!   calls (unarmed + matching class_id is unreachable: the class id is only
//!   ever assigned by the same bootstrap that arms).
use std::sync::atomic::{AtomicPtr, Ordering};

use super::ObjectHeader;
use crate::string::StringHeader;

pub(crate) struct NmNamespaceOps {
    /// `Object.getOwnPropertyDescriptor` on a namespace object. Returns the
    /// NaN-boxed descriptor object, or `None` to fall through to the generic
    /// own-property handling.
    pub get_own_descriptor:
        unsafe fn(*mut ObjectHeader, *const StringHeader, key_name: Option<&str>) -> Option<f64>,
    /// Dynamic field store on a namespace object (CommonJS namespaces are
    /// mutable in Node — overrides are recorded, not thrown). Returns true
    /// when the store was fully handled.
    pub field_set_override: unsafe fn(*mut ObjectHeader, *const StringHeader, f64) -> bool,
    /// `Reflect.defineProperty`-style "is this an existing own key" probe.
    /// The key is the NaN-boxed property key value.
    pub reflect_has_enumerable: unsafe fn(*mut ObjectHeader, f64) -> bool,
    /// Own-key enumeration (`Object.keys` / `getOwnPropertyNames` clone
    /// paths). `None` when the value is not a namespace object.
    pub own_keys_array: unsafe fn(*const ObjectHeader) -> Option<*mut crate::array::ArrayHeader>,
    /// Bound-method materialization for a dynamic property get on a
    /// namespace object (`const fn = fs.lstatSync`): NaN-boxed namespace
    /// value + property-name bytes → bound callable (or undefined).
    pub bind_method: unsafe fn(f64, *const u8, usize) -> f64,
    /// What the receiver's namespace shape says about an own member (`None`: no such
    /// member, or deleted, or hidden): see [`NmMember`].
    pub member: fn(*const ObjectHeader, &str, &str) -> Option<NmMember>,
}

/// An own member as its namespace shape describes it: the one
/// classification its descriptor (`nm_get_own_descriptor`) and `delete`
/// ([`nm_delete_member`]) read.
#[derive(Clone, Copy)]
pub(crate) enum NmMember {
    /// A zlib read-only constant: non-writable, non-configurable data.
    ZlibReadonly { enumerable: bool },
    /// An fs stream class or `fs.promises`: a configurable accessor.
    FsAccessor,
    /// `fs.constants`, `process.permission`: non-writable, non-configurable data.
    ReadonlyData,
    /// Writable data (`module`, `async_hooks` and `vm` members are not
    /// configurable).
    Data { configurable: bool },
}

impl NmMember {
    pub(crate) fn configurable(self) -> bool {
        matches!(
            self,
            NmMember::FsAccessor | NmMember::Data { configurable: true }
        )
    }
}

/// `[[Delete]]` of an own member of a native namespace (`delete console.log`,
/// `delete require("path").join`). Called only by the program-level deletes
/// (`delete o.k`, `delete o[k]`, `Reflect.deleteProperty` and a trap-less
/// proxy), so a binary without one links none of it; the module tables are
/// reached through the armed ops (a namespace exists only once they are). A
/// member the tables describe as non-configurable is kept: `Some(0)`. A
/// described or stored member is marked deleted in the override table, then
/// `None`: the caller's ordinary delete drops any own field, accessor or
/// attributes of the key and answers `true`.
#[inline(never)]
pub(crate) unsafe fn nm_delete_member(obj: f64, key: f64) -> Option<i32> {
    let ops = nm_namespace_ops()?;
    let ptr = crate::value::js_nanbox_get_pointer(obj) as *const ObjectHeader;
    if !(crate::value::JSValue::from_bits(obj.to_bits()).is_pointer()
        && crate::value::addr_class::is_above_handle_band(ptr as usize)
        && super::is_valid_obj_ptr(ptr as *const u8)
        && (*ptr).class_id == super::native_module::NATIVE_MODULE_CLASS_ID)
    {
        return None;
    }
    // Both names borrow heap strings: nothing below allocates before the
    // override store has copied them.
    let (mut module_buf, mut key_buf) = (
        [0u8; crate::value::SHORT_STRING_MAX_LEN],
        [0u8; crate::value::SHORT_STRING_MAX_LEN],
    );
    let module = crate::string::js_string_key_bytes(
        super::js_object_get_field(ptr as *mut _, 0),
        &mut module_buf,
    )?;
    let name = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key.to_bits()),
        &mut key_buf,
    )?;
    let (module, name) = (
        std::str::from_utf8(module).ok()?,
        std::str::from_utf8(name).ok()?,
    );
    match (ops.member)(ptr, module, name) {
        Some(member) if !member.configurable() => return Some(0),
        None if name == "__module__"
            || super::native_module::native_namespace_prop_override_get(module, name).is_none() =>
        {
            return None
        }
        _ => {}
    }
    super::native_module::native_namespace_prop_override_delete(module, name);
    None
}

static NM_NAMESPACE_OPS: AtomicPtr<NmNamespaceOps> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn arm_nm_namespace_ops(ops: &'static NmNamespaceOps) {
    // `black_box` for the same reason as `NM_INSTALL_ALL_HOOK` / the fs
    // thread codec: whole-program opt speculatively devirtualizes a
    // single-store AtomicPtr back into direct references, re-pinning
    // everything this table exists to unpin.
    NM_NAMESPACE_OPS.store(
        std::hint::black_box(ops as *const NmNamespaceOps as *mut NmNamespaceOps),
        Ordering::Release,
    );
}

/// EventEmitter behaviors of the bound `events` / `stream` exports, split out
/// of [`NmNamespaceOps`] (binary size): that table is armed by every namespace
/// (including the always-present `console` / `process`), and these two ops pin
/// the whole EventEmitter/stream method tower. They only ever act on a bound
/// `events.EventEmitter` / `EventEmitterAsyncResource` / `stream.Stream`
/// callable, which exists only after its module's `js_nm_install_events()` /
/// `js_nm_install_stream()` ran (every import form emits it; dynamic `require`
/// / `getBuiltinModule` go through install-all) — those installers arm this.
pub(crate) struct NmEeOps {
    /// #5477: install the EventEmitter prototype methods on the synthetic
    /// prototype of the bound `events.EventEmitter` export. No-op for every
    /// other function value.
    pub ee_prototype_install: unsafe fn(f64, *mut ObjectHeader),
    /// Declared layout of an events prototype, null for another function.
    pub ee_prototype_layout: unsafe fn(f64) -> *const super::layout_desc::PerryLayoutDesc,
    /// Dynamic `super()` for `class X extends <runtime EventEmitter export>`:
    /// installs the EE methods on the fresh instance. `None` when the callee
    /// is not the bound events export (fall through to normal call dispatch).
    /// The second argument is the call's `this` (the fresh instance).
    pub ee_dynamic_super: unsafe fn(f64, crate::closure::JsThis, *const f64, usize) -> Option<f64>,
}

static NM_EE_OPS: AtomicPtr<NmEeOps> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn arm_nm_ee_ops(ops: &'static NmEeOps) {
    NM_EE_OPS.store(
        std::hint::black_box(ops as *const NmEeOps as *mut NmEeOps),
        Ordering::Release,
    );
}

#[inline]
pub(crate) fn nm_ee_ops() -> Option<&'static NmEeOps> {
    let p = NM_EE_OPS.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // SAFETY: only ever stores `&'static NmEeOps`.
        Some(unsafe { &*p })
    }
}

#[inline]
pub(crate) fn nm_namespace_ops() -> Option<&'static NmNamespaceOps> {
    let p = NM_NAMESPACE_OPS.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // SAFETY: only ever stores `&'static NmNamespaceOps`.
        Some(unsafe { &*p })
    }
}
