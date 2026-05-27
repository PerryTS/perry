//! Class method vtable registry — enables runtime dispatch for
//! interface-typed and dynamically-typed method calls. Each class
//! registers its methods, getters, and setters at startup;
//! `js_native_call_method` / `js_dynamic_object_get_property` look up
//! the vtable by the object's `class_id` when static dispatch isn't
//! possible. Also home for the per-callsite inline cache
//! (`vtable_ic_*` / `call_vtable_method`) and the parent-chain
//! registration helpers used by codegen.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

// ============================================================================
// Class method vtable registry — enables runtime dispatch for interface-typed
// and dynamically-typed method calls.  Each class registers its methods and
// getters at startup; js_native_call_method / js_dynamic_object_get_property
// look up the vtable by the object's class_id when static dispatch isn't possible.
// ============================================================================

/// Entry in the class method vtable
pub struct VTableMethodEntry {
    pub func_ptr: usize,
    pub param_count: u32,
}

/// Per-class vtable with methods, getters, and setters
pub struct ClassVTable {
    pub methods: HashMap<String, VTableMethodEntry>,
    pub getters: HashMap<String, usize>, // getter func_ptr (signature: fn(this_f64) -> f64)
    pub setters: HashMap<String, usize>, // setter func_ptr (signature: fn(this_f64, value_f64) -> f64)
}

/// Global vtable registry: class_id -> vtable
pub static CLASS_VTABLE_REGISTRY: RwLock<Option<HashMap<u32, ClassVTable>>> = RwLock::new(None);

/// #1788: per-class STATIC-method registry: class_id -> { name -> (func_ptr,
/// param_count, has_rest) }. Static methods are emitted as `perry_static_*`
/// (no `this` param — they read `this` from the implicit-this slot) and are
/// NOT in the instance vtable above, so a subclass whose parent is a
/// class-expression value (`class Sub extends make(...) {}`) can't resolve an
/// inherited static method (`Sub.greet()`) at compile time. This table is
/// walked up the class_id parent chain at runtime by
/// `js_class_static_method_call`. `has_rest` marks a trailing rest param
/// (`static pipe(...args)`, effect's `pipe`/`dual`) so the dispatcher bundles
/// the call args into an array for that slot.
pub static CLASS_STATIC_METHODS: RwLock<Option<HashMap<u32, HashMap<String, (usize, u32, bool)>>>> =
    RwLock::new(None);

/// Set of all registered class ids. Populated at module init by codegen
/// emitting `js_register_class_id(cid)` for every user class — even
/// classes without any methods. Refs #618 / #420 followup.
pub static REGISTERED_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Issue #711 part 2: `function Base() {}; Base.prototype = obj` pattern.
/// Effect's `internal/effectable.ts` declares classes via prototype
/// assignment on a plain function, not via `class` syntax. To make
/// `class Derived extends Base {}` walk into `obj`'s methods at dispatch
/// time, we model this as a synthetic class:
///   - `js_set_function_prototype(func, obj)` allocates a synthetic
///     class_id (high-bit-set to avoid collision with codegen-assigned
///     ids), stores `func_bits → synthetic_cid` in `FUNCTION_CLASS_IDS`,
///     and `synthetic_cid → obj_ptr` in `CLASS_PROTOTYPE_OBJECTS`.
///   - `js_register_class_parent_dynamic` extends to detect closure
///     parent values, looks up the synthetic class_id, and registers
///     the (child, synthetic) edge in CLASS_REGISTRY.
///   - The method-dispatch chain walk in `js_native_call_method`
///     consults `CLASS_PROTOTYPE_OBJECTS` when it reaches a synthetic
///     class_id: it resolves the method as a regular field lookup on
///     the prototype object and calls it with `this` bound to the
///     receiver.
pub static FUNCTION_CLASS_IDS: RwLock<Option<HashMap<u64, u32>>> = RwLock::new(None);
// Stored as `usize` (raw address) so the map is Send + Sync. The
// pointer is always converted back to `*mut ObjectHeader` at call sites
// (`class_prototype_object` / the dispatch walk) where single-threaded
// usage is guaranteed.
pub static CLASS_PROTOTYPE_OBJECTS: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// #36 / #321: maps a child class_id to the raw address of a parent CLOSURE
/// (function value) when `class Child extends <function value> {}`. effect's
/// `class Svc extends Context.Tag("Svc")<...>() {}` extends the function
/// `TagClass` returned by `Tag(id)()`. In JS this sets `Svc.__proto__ =
/// TagClass` so static-property reads on `Svc` (`Svc.key`, `Svc._op`,
/// `Svc[TagTypeId]`) walk to the parent function's own props + ITS static
/// prototype. Perry's existing dynamic-parent path only models OBJECT parents
/// (class-expression values), so this records the closure-parent axis so the
/// class-ref static getters can reach the closure's props and proto chain.
/// Stored as `usize` (raw address) for Send + Sync; converted back at use.
pub static CLASS_PARENT_CLOSURES: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Look up the parent-closure address recorded for a child class_id, if any.
pub(crate) fn class_parent_closure(class_id: u32) -> Option<usize> {
    CLASS_PARENT_CLOSURES
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(&class_id).copied()))
}

/// Synthetic class id allocator for prototype-object classes. High bit
/// set (0x8000_0000+) to keep them separate from codegen-assigned ids
/// (which start from 1 and grow by module). u32 wraparound is not a
/// concern in practice — would require ~2 billion `Function.prototype = X`
/// statements at module init.
pub static NEXT_SYNTHETIC_CLASS_ID: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x8000_0000);

/// Register a function's prototype object. Called by codegen-emitted
/// init code whenever the HIR detects `<expr>.prototype = <expr>` at
/// the assignment-statement level (lower_expr_assignment Member arm).
///
/// Returns the synthetic class_id allocated for this function (0 if
/// validation fails). The synthetic id is folded into CLASS_REGISTRY
/// when a class extends `func` via the #711 dynamic-parent path.
#[no_mangle]
pub extern "C" fn js_set_function_prototype(func: f64, proto: f64) -> u32 {
    let func_bits = func.to_bits();
    let func_tag = func_bits & 0xFFFF_0000_0000_0000;
    let proto_bits = proto.to_bits();
    let proto_tag = proto_bits & 0xFFFF_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    // Both must be heap-allocated pointers. Anything else (primitives,
    // ClassRef, etc.) is a no-op — preserves the pre-fix baseline
    // where `<not-a-function>.prototype = X` was just a property write
    // on a non-function value (effectively no-op in practice).
    if func_tag != POINTER_TAG || proto_tag != POINTER_TAG {
        return 0;
    }
    // Validate the proto pointer points at a real Object. If it's a
    // builtin header (Set/Map/Regex) or null, bail — Perry can't
    // currently model those as prototype sources.
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
    if proto_ptr.is_null() {
        return 0;
    }
    let proto_addr = proto_ptr as usize;
    if crate::set::is_registered_set(proto_addr)
        || crate::map::is_registered_map(proto_addr)
        || crate::regex::is_regex_pointer(proto_ptr as *const u8)
    {
        return 0;
    }
    unsafe {
        if !is_valid_obj_ptr(proto_ptr as *const u8) {
            return 0;
        }
        let gc_header =
            (proto_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return 0;
        }
    }

    // Allocate or reuse a synthetic class id for this function value.
    // The same `function Base() {}` ident can be assigned a prototype
    // multiple times in pathological code; we keep the FIRST mapping
    // and quietly ignore subsequent calls so existing parent edges
    // don't dangle.
    {
        let read = FUNCTION_CLASS_IDS.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                // Update the prototype object (allow re-pointing)
                // without changing the class_id.
                let mut proto_write = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
                if proto_write.is_none() {
                    *proto_write = Some(HashMap::new());
                }
                proto_write
                    .as_mut()
                    .unwrap()
                    .insert(existing, proto_ptr as usize);
                crate::typed_feedback::invalidate_method_change(existing);
                return existing;
            }
        }
    }
    let new_cid = NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut write = FUNCTION_CLASS_IDS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    }
    {
        let mut write = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(new_cid, proto_ptr as usize);
    }
    // Register the synthetic id so REGISTERED_CLASS_IDS-gated paths
    // (e.g., the #687 ClassRef-as-receiver short-circuit) recognize it.
    unsafe { js_register_class_id(new_cid) };
    crate::typed_feedback::invalidate_method_change(new_cid);
    new_cid
}

/// Lookup helper for the dispatch chain walk: returns the prototype
/// object pointer for a synthetic class id, or null if none.
#[inline]
pub(crate) fn class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    if let Ok(read) = CLASS_PROTOTYPE_OBJECTS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
        }
    }
    std::ptr::null_mut()
}

/// #711 / #809: resolve `key` by walking the synthetic-class-id prototype
/// chain (`CLASS_PROTOTYPE_OBJECTS`), recursing into each prototype object
/// as a normal field lookup. Used both when a receiver's own keys miss AND
/// when it has no `keys_array` at all (an `Object.create(proto)` result, or
/// a `Function.prototype = obj` instance with no own props). Returns the
/// first defined, non-null field found on the chain.
pub(crate) unsafe fn resolve_proto_chain_field(
    class_id: u32,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        let proto_obj = class_prototype_object(cid);
        if !proto_obj.is_null() {
            let field_val = js_object_get_field_by_name(proto_obj as *const _, key);
            if !field_val.is_undefined() && !field_val.is_null() {
                return Some(field_val);
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// #1758: symbol-keyed analogue of [`resolve_proto_chain_field`]. Walks the
/// `CLASS_PROTOTYPE_OBJECTS` chain and, at each prototype object (a POINTER
/// class-object), looks up its OWN symbol property via `own_symbol_property`.
/// Lets a subclass whose parent is a class-expression value inherit the
/// parent's static *symbol* statics — e.g. effect's
/// `class BigIntFromSelf extends make(bigIntKeyword) {}` inheriting
/// `static [TypeId]`, which `Predicate.hasProperty(.., TypeId)` (`isSchema`)
/// and `u[TypeId]` both read. Returns the first defined value found.
///
/// #26 / #321: the walk must advance along TWO axes, because a synthetic
/// `Object.create(proto)` class id links to its prototype via the *proto
/// object's own class id*, not via `parent_class_id` (which only models the
/// `class A extends B` axis). effect's `Either.right(x)` builds
/// `Object.create(RightProto)` where `RightProto = Object.create(CommonProto)`
/// and `CommonProto[TypeId]` carries the brand. With only the
/// `parent_class_id` axis the walk stopped after the first prototype object
/// (`RightProto`), so `TypeId in either` / `either[TypeId]` missed the brand
/// two links up — making `ParseResult.isEither(...)` false for every struct
/// property parse (`S.is`/`decodeUnknownSync`/`encodeSync` on a `Struct`).
/// At each node we follow the proto object's own class id (the
/// `Object.create` prototype link) first, then fall back to
/// `parent_class_id` (the `extends` link); a `visited` set bounds cycles.
pub(crate) unsafe fn resolve_proto_chain_symbol(class_id: u32, sym_f64: f64) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0usize;
    let mut visited: [u32; 32] = [0; 32];
    while depth < 32 {
        if visited[..depth].contains(&cid) {
            break;
        }
        visited[depth] = cid;
        let proto_obj = class_prototype_object(cid);
        let mut next_cid: u32 = 0;
        if !proto_obj.is_null() {
            let proto_f64 = f64::from_bits(JSValue::pointer(proto_obj as *const u8).bits());
            // OWN lookup only — this fn IS the chain walk, so recursing into
            // the full chain-walking getter would re-walk per prototype.
            if let Some(v) = crate::symbol::own_symbol_property(proto_f64, sym_f64) {
                return Some(v);
            }
            // Prefer the `Object.create` prototype link: the next chain node
            // is the proto object's own class id (which maps to ITS proto in
            // CLASS_PROTOTYPE_OBJECTS). Falls back to `parent_class_id` below.
            next_cid = crate::object::js_object_get_class_id(proto_obj as *const ObjectHeader);
        }
        if next_cid != 0 && next_cid != cid {
            cid = next_cid;
            depth += 1;
            continue;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Lookup the synthetic class id for a function value, if one was
/// registered via `js_set_function_prototype`.
#[inline]
pub(crate) fn function_class_id(value: f64) -> u32 {
    let bits = value.to_bits();
    if let Ok(read) = FUNCTION_CLASS_IDS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&bits).copied().unwrap_or(0);
        }
    }
    0
}

/// Register a class id so `js_value_typeof` can distinguish class refs
/// (INT32-tagged with class_id payload) from real int32 numeric values.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = REGISTERED_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// Maps `class_id → user-visible class name`. Populated by codegen via
/// `js_register_class_name`. Read back by V8-bridge code when surfacing a
/// Perry class to JS — NestJS's `ModuleTokenFactory.create()` reads
/// `metatype.name` to build the module token, so the empty default name
/// from `v8::Function::builder(...)` would collide every module under the
/// same token. (#1021.)
pub static CLASS_NAMES: RwLock<Option<HashMap<u32, String>>> = RwLock::new(None);

/// Register the user-visible name of a class so the V8 bridge can label
/// the V8-side wrapper for nice `metatype.name` reads. Idempotent.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_name(class_id: u32, name_ptr: *const u8, name_len: u32) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let name = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = CLASS_NAMES.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(class_id, name);
}

/// Look up the user-visible name of a registered class. Returns `None`
/// when the class id was never registered with `js_register_class_name`.
pub fn class_name_for_id(class_id: u32) -> Option<String> {
    let guard = CLASS_NAMES.read().ok()?;
    guard.as_ref()?.get(&class_id).cloned()
}

/// Whether dynamic-dispatch miss diagnostics are enabled (`PERRY_DISPATCH_DIAG`,
/// any non-empty/non-falsey value). Cached on first read.
///
/// When a dynamic dispatch falls through every resolution tower (vtable,
/// static-method, static-field, prototype, field-scan, namespace, symbol), the
/// runtime returns a *silent placeholder* — the receiver class ref, an empty
/// object, `undefined`, etc. — rather than throwing, because some of those
/// placeholders are load-bearing (effect's `.pipe()` chains yield the class ref
/// during module init, #687). The upside is no spurious crashes; the downside
/// is a typo'd / unsupported member surfaces far downstream as a stray
/// `{}`/`1`/`[]`/function, turning each one into a multi-hour localization.
///
/// This flag doesn't change behavior — it just prints a located, typed report
/// at the moment of the miss, so the bug surfaces at its true call site.
pub(crate) fn dispatch_diag_enabled() -> bool {
    use std::sync::OnceLock;
    static EN: OnceLock<bool> = OnceLock::new();
    *EN.get_or_init(|| {
        std::env::var("PERRY_DISPATCH_DIAG")
            .map(|v| !v.is_empty() && v != "0" && v != "off" && v != "false")
            .unwrap_or(false)
    })
}

/// Best-effort one-line description of a dispatch receiver for diagnostics:
/// class refs resolve to their registered name, pointers/primitives to a tag.
fn describe_dispatch_receiver(recv: f64) -> String {
    let bits = recv.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let cid = (bits & 0xFFFF_FFFF) as u32;
        return match class_name_for_id(cid) {
            Some(n) => format!("class-ref `{}` (id {})", n, cid),
            None => format!("class-ref (id {})", cid),
        };
    }
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        return "string".to_string();
    }
    if top16 == 0x7FFD {
        return "object/pointer".to_string();
    }
    match bits {
        x if x == crate::value::TAG_UNDEFINED => "undefined".to_string(),
        0x7FFC_0000_0000_0002 => "null".to_string(),
        0x7FFC_0000_0000_0003 => "false".to_string(),
        0x7FFC_0000_0000_0004 => "true".to_string(),
        _ if !recv.is_nan() => format!("number {}", recv),
        _ => "value".to_string(),
    }
}

/// Report a true dynamic-dispatch miss to stderr (only when
/// `PERRY_DISPATCH_DIAG` is set). `tower` names which resolution path fell
/// through; `returning` is the silent placeholder the runtime is about to hand
/// back. No-op (and near-zero cost) when the flag is off.
pub(crate) fn report_dispatch_miss(tower: &str, recv: f64, name: &str, returning: &str) {
    if !dispatch_diag_enabled() {
        return;
    }
    eprintln!(
        "[perry dispatch-miss] {tower}: {}.{:?} did not resolve \u{2192} returning {returning}. \
         A dynamic dispatch fell through every tower; downstream this usually surfaces as a stray \
         {{}}/1/[]/function. Check the call site for {:?}.",
        describe_dispatch_receiver(recv),
        name,
        name
    );
}

/// Resolve a closure-typed JSValue back to a built-in constructor name
/// (`"Date"`/`"Array"`/`"Object"`/...) when it matches one of the
/// singleton-installed thunks. Returns `None` for closures that aren't
/// the globalThis built-in constructors. Used by
/// `js_new_function_construct` to dispatch `new <inst.constructor>(...)`
/// shapes (date-fns `constructFrom`, lodash-style `Array` cloning, ...)
/// to the right runtime factory.
fn identify_global_builtin_constructor(func_value: f64) -> Option<&'static str> {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return None;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    // Identify by the closure's read-only `func_ptr` rather than the
    // GC-movable ClosureHeader address. Both the date-fns ctor closure
    // and the (later-evacuated) ctor closure carry the same
    // `global_this_builtin_noop_thunk` function pointer, so this match
    // survives GC moves. The per-name lookup must then walk the
    // globalThis singleton's keys to recover the constructor name —
    // accept the extra hop only when the func_ptr matches.
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return None;
        }
        let func_ptr = (*ptr).func_ptr as usize;
        let noop_thunk = global_this_builtin_noop_thunk as *const u8 as usize;
        if func_ptr != noop_thunk {
            return None;
        }
    }
    // Find which builtin name maps to this exact closure header on the
    // singleton. Walk via the existing
    // `js_get_global_this_builtin_value` helper — short loop (≤ ~50
    // entries), only fires on the constructFrom hot path.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return None;
    }
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = unsafe { js_object_get_field_by_name(global_obj, key) };
        if v.bits() == jv.bits() {
            return Some(name);
        }
    }
    None
}

/// Synthetic-anonymous-shape class IDs: classes the HIR generates for
/// bare object literals (`{ x: 1 }` → `__AnonShape_<hash>`). Instances
/// of these shapes should report `Object` from `.constructor`, not the
/// synthetic class itself, so date-fns's `new value.constructor(...)`,
/// drizzle's `value.constructor === Object` duck checks, and the standard
/// `({}).constructor === Object` semantics all match Node. The HIR
/// lowering registers each anon shape's id here at module init.
pub static ANON_SHAPE_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Mark `class_id` as a synthetic anon-shape class so `.constructor`
/// reads on instances of that class return the global `Object`
/// constructor rather than the synthetic class ref.
#[no_mangle]
pub unsafe extern "C" fn js_register_anon_shape_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = ANON_SHAPE_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// True if `class_id` was registered via `js_register_anon_shape_class_id`.
pub fn is_anon_shape_class_id(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = ANON_SHAPE_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            return set.contains(&class_id);
        }
    }
    false
}

/// Register a static field value on a class so `Cls.field` (when `Cls` is
/// accessed via dynamic dispatch — e.g. through an Any-typed local) finds
/// the value via the runtime path. Codegen calls this at module init for
/// every static field initializer in addition to writing the value to the
/// per-field module global. Refs #420 / #618 followup. Static-field values
/// stored in CLASS_DYNAMIC_PROPS keyed by class_id.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_field(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow_mut()
            .entry(class_id)
            .or_insert_with(std::collections::HashMap::new)
            .insert(name, value);
    });
}

/// Issue #838: JS-classic prototype method assignment.
///
/// `Class.prototype.method = function() {…}` (and the aliased form
/// `var p = Class.prototype; p.method = function() {…}`) is a pre-ES6
/// idiom dayjs, chalk, and a long tail of libraries still ship.
/// Pre-fix the assignment was lowered to a generic `PropertySet` whose
/// receiver evaluated to a class-prototype-shaped object that nothing
/// downstream consulted, so `(new Class()).method` came back as
/// `undefined`.
///
/// The HIR-level fix routes recognised shapes to
/// `js_register_prototype_method(class_id, name, value)`, which stores
/// the closure value into a per-class side-table here. The dispatch
/// hot paths (`js_object_get_field_by_name` for `inst.method` reads
/// and `js_native_call_method` for `inst.method(...)` calls) consult
/// this table after the regular vtable / proto-object lookups miss,
/// invoking the closure with `this` bound to the receiver.
///
/// Stored values use their full NaN-boxed bits (f64) — typically a
/// POINTER_TAG'd closure, but the dispatch path treats whatever is
/// stored as a callable value and routes it through
/// `js_native_call_value`, which itself accepts both closures and raw
/// `*ClosureHeader` shapes.
pub static CLASS_PROTOTYPE_METHODS: RwLock<Option<HashMap<u32, HashMap<String, u64>>>> =
    RwLock::new(None);

/// Register a JS-classic prototype-method assignment on a class.
/// Called by codegen-emitted init code for each `Class.prototype.<name>
/// = <fn>` (or aliased form) that the HIR recognises. `value` is the
/// NaN-boxed callable to be invoked with `this` bound to the receiver
/// at dispatch time.
#[no_mangle]
pub unsafe extern "C" fn js_register_prototype_method(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = CLASS_PROTOTYPE_METHODS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .entry(class_id)
        .or_insert_with(HashMap::new)
        .insert(name, value.to_bits());
    // Ensure the receiver class can be `typeof`-detected. Method-less
    // classes that only get extended via `Class.prototype.m = fn`
    // wouldn't otherwise reach js_register_class_id.
    js_register_class_id(class_id);
    crate::typed_feedback::invalidate_method_change(class_id);
}

/// Issue #838 followup (b): function-classic prototype-method dispatch.
/// dayjs's minified bundle declares its instance class via a function
/// declaration inside an IIFE (`function M(cfg) {…}; var m = M.prototype;
/// m.format = function(){…}; return M`). At HIR time `M` is a function
/// (no `class M` block), so the #838 recogniser bailed because
/// `lookup_class("M")` returned None. This helper closes the gap on the
/// runtime side: a single call takes the closure value of `M`, allocates
/// (or reuses) a synthetic class id keyed by the closure's NaN-boxed
/// bits, registers the method on that synthetic class, and returns the
/// id so a paired `new <FuncRef>(args)` allocator can stamp the same id
/// on the instance header. After both arms run, the existing dispatch
/// hot paths (`js_object_get_field_by_name`, `js_native_call_method`)
/// find the method without further changes.
///
/// `func_value` must be a POINTER_TAG'd ClosureHeader (the shape
/// `Expr::FuncRef` lowers to via `js_closure_alloc_singleton`). Anything
/// else is a no-op — preserves the pre-fix baseline where non-callable
/// `.prototype.m = fn` writes were silent property sets.
/// Issue #838 followup (b) — read side: look up a method previously
/// registered via `js_register_function_prototype_method` against the
/// synthetic class id derived from `func_value`. Pre-fix the AST shape
/// `<funcDecl>.prototype.<name>` lowered to a generic PropertyGet on a
/// `Function.prototype` object that never materialised, so the read
/// was always `undefined` — `typeof Foo.prototype.method` came back
/// `'undefined'` even when the method was correctly dispatched through
/// `(new Foo()).method` via the side-table walk. Pairs with the new
/// `Expr::GetFunctionPrototypeMethod` HIR variant.
///
/// Returns the NaN-boxed `undefined` tag if the function value isn't a
/// registered closure, or no method by that name was registered.
#[no_mangle]
pub unsafe extern "C" fn js_get_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    if name_ptr.is_null() || name_len == 0 {
        return undef;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return undef,
    };
    // Look up the (already-allocated) synthetic class id for this
    // function value. Don't allocate one here — reads on a function
    // that never had any `.prototype.x = fn` assignment should
    // return `undefined`, matching the spec'd behavior of reading a
    // missing property on the `Function.prototype` object.
    let cid = function_class_id(func_value);
    if cid == 0 {
        return undef;
    }
    match lookup_prototype_method(cid, name) {
        Some(v) => v,
        None => undef,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) -> u32 {
    let cid = synthetic_class_id_for_function(func_value);
    if cid == 0 || name_ptr.is_null() || name_len == 0 {
        return cid;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return cid,
    };
    let mut guard = CLASS_PROTOTYPE_METHODS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .entry(cid)
        .or_insert_with(HashMap::new)
        .insert(name, value.to_bits());
    js_register_class_id(cid);
    crate::typed_feedback::invalidate_method_change(cid);
    cid
}

/// Get-or-allocate a synthetic class id keyed by a function value's
/// NaN-boxed bits. Used by `js_register_function_prototype_method` (HIR
/// "Func.prototype.x = fn" recogniser) and `js_new_function_construct`
/// (HIR "new Func(args)" allocator) so both sides agree on the same id
/// — the instance's `(*obj).class_id` lands in the same bucket the
/// method registration stored against. Returns 0 if `func_value` isn't a
/// POINTER_TAG'd value (callable shape requirement).
pub(crate) fn synthetic_class_id_for_function(func_value: f64) -> u32 {
    let func_bits = func_value.to_bits();
    // Require a verified closure shape so we don't store arbitrary
    // POINTER_TAG'd pointers (arrays, objects, etc. all share the tag)
    // in `FUNCTION_CLASS_IDS`. The bits-as-key invariant only makes
    // sense for callable values that produced a stable singleton
    // closure pointer.
    if !is_callable_function_value(func_value) {
        return 0;
    }
    {
        let read = FUNCTION_CLASS_IDS.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                return existing;
            }
        }
    }
    let new_cid = NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut write = FUNCTION_CLASS_IDS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    }
    unsafe { js_register_class_id(new_cid) };
    new_cid
}

/// Issue #838 followup (b): construct an instance from a function value.
/// Pairs with `js_register_function_prototype_method` — both arms route
/// through `synthetic_class_id_for_function` so the instance's
/// `class_id` matches the bucket prototype methods were registered
/// against. Allocates a fresh object stamped with the synthetic id,
/// then invokes the function as the constructor with `IMPLICIT_THIS`
/// bound to the new object so any `this.foo = …` writes in the
/// function body land on the instance. Returns the NaN-boxed new
/// instance pointer.
///
/// `func_value` must be a POINTER_TAG'd closure. `args_ptr` is a flat
/// f64 array of length `args_len`. Falls back to a class_id=0
/// empty-object allocation when the function value isn't a closure
/// (preserves the pre-fix baseline for misuse).
#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if let Some((module, method)) = bound_native_callable_module_and_method(func_value) {
        if module == "tty" && matches!(method.as_str(), "ReadStream" | "WriteStream") {
            let fd = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            if !fd.is_finite() || fd < 0.0 || fd.fract() != 0.0 {
                crate::tty::throw_invalid_fd(fd);
            }
            return crate::value::js_nanbox_pointer(js_object_alloc(0, 0) as i64);
        }
    }

    // date-fns `constructFrom` clones a Date via
    // `new date.constructor(value)`. `date.constructor` resolves to
    // the global `Date` closure pointer (the noop thunk installed by
    // `populate_global_this_builtins`). Without this intercept the
    // call falls through to the generic empty-object path and
    // `cloned.getTime()` reads garbage. Detect the global Date /
    // Array / Object constructor pointers and dispatch into the
    // matching real factory. Refs date-fns blocker.
    if let Some(name) = identify_global_builtin_constructor(func_value) {
        let args = if args_ptr.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(args_ptr, args_len)
        };
        match name {
            "Date" => {
                if args.is_empty() {
                    return crate::date::js_date_new();
                }
                if args.len() == 1 {
                    return crate::date::js_date_new_from_value(args[0]);
                }
                let mut vals = [f64::from_bits(crate::value::TAG_UNDEFINED); 7];
                for (i, slot) in vals.iter_mut().enumerate() {
                    if i < args.len() {
                        *slot = args[i];
                    }
                }
                return crate::date::js_date_new_local_components(
                    vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6],
                );
            }
            "Array" => {
                // `new Array(n)`: empty array of length n.
                // `new Array(a, b, c)`: array filled with the args.
                let single_len = args.len() == 1 && args[0].is_finite() && args[0] >= 0.0;
                let len = if single_len {
                    args[0] as u32
                } else {
                    args.len() as u32
                };
                let arr = crate::array::js_array_alloc(len);
                if !single_len {
                    for (i, &v) in args.iter().enumerate() {
                        crate::array::js_array_set_f64(arr, i as u32, v);
                    }
                }
                return crate::value::js_nanbox_pointer(arr as i64);
            }
            "Object" => {
                let obj = js_object_alloc(0, 0);
                return crate::value::js_nanbox_pointer(obj as i64);
            }
            _ => {}
        }
    }
    // #1789: `new (classObjectValue)(args)` — the callee is a heap class
    // object (the value a class EXPRESSION evaluates to, e.g.
    // `const C = make(x); new C()`). Read its class_id (the compile-time
    // template) and allocate an instance of it, so instance methods
    // dispatch and `x instanceof C` matches. The template's constructor
    // body / field initializers are emitted on the static `new ClassName()`
    // path, not this dynamic helper, so a dynamically-constructed instance
    // starts with no own props (constructor-run on dynamic new is a
    // refinement); fields written afterward and prototype methods work.
    if is_class_object_value(func_value) {
        let obj =
            crate::value::JSValue::from_bits(func_value.to_bits()).as_pointer::<ObjectHeader>();
        let class_cid = js_object_get_class_id(obj);
        if class_cid != 0 {
            let inst = js_object_alloc(class_cid, 0);
            return crate::value::js_nanbox_pointer(inst as i64);
        }
    }
    let cid = synthetic_class_id_for_function(func_value);
    // Allocate the instance with the synthetic class id (or 0 if the
    // value isn't callable). The object starts with no own props; the
    // constructor body fills `this.<field>` writes through
    // PropertySet, and prototype-method dispatch consults the
    // synthetic class id's entry in CLASS_PROTOTYPE_METHODS.
    let obj_ptr = js_object_alloc(cid, 0);
    let nan_boxed = crate::value::js_nanbox_pointer(obj_ptr as i64);
    // Only run the constructor body when the callee is recognised as
    // a closure shape. The codegen LocalGet path widens the route to
    // any local-resolved callee, so we have to gate the
    // `js_native_call_value` dispatch on a verified closure pointer
    // here — otherwise `new <non-callable>()` would dereference an
    // arbitrary pointer as a `ClosureHeader` and crash.
    if is_callable_function_value(func_value) {
        // Bind `this` to the new instance, dispatch the constructor,
        // then restore the previous IMPLICIT_THIS. The dispatch
        // result is discarded — JS `new` semantics use the receiver,
        // not the returned value (object returns would override, but
        // dayjs and siblings rely on the receiver mutation pattern).
        let prev_this = crate::object::js_implicit_this_get();
        crate::object::js_implicit_this_set(nan_boxed);
        let _ = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
        crate::object::js_implicit_this_set(prev_this);
    }
    nan_boxed
}

/// Verify that a JSValue is a NaN-boxed pointer to a registered
/// closure header. `js_native_call_value` itself doesn't validate the
/// pointer shape — it dereferences whatever lower-48 bits it gets — so
/// the `new <LocalGet>(args)` widened path here in
/// `js_new_function_construct` needs to gate the constructor dispatch
/// on a real closure to avoid SIGSEGV'ing on non-callable callees
/// (`new someObject()`, `new someStringVar()`, etc.). Uses the
/// `_reserved` magic word `crate::closure::CLOSURE_MAGIC` that every
/// `js_closure_alloc*` site stamps on allocation.
fn is_callable_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return false;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    unsafe { (*ptr).type_tag == crate::closure::CLOSURE_MAGIC }
}

/// Lookup helper: returns the registered prototype-method value for
/// `(class_id, name)`, or None if no assignment matched. Walks the
/// parent-class chain so methods registered on a base class are found
/// via subclass instances.
pub(crate) fn lookup_prototype_method(class_id: u32, name: &str) -> Option<f64> {
    let guard = CLASS_PROTOTYPE_METHODS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        if let Some(per_class) = map.get(&cid) {
            if let Some(&bits) = per_class.get(name) {
                return Some(f64::from_bits(bits));
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Returns true if `class_id` corresponds to a registered class. Used by
/// `js_value_typeof` (refs #618 / #420 followup) to distinguish a class
/// reference (NaN-boxed INT32 with class_id payload) from a regular int32
/// numeric value — JS spec says `typeof <class>` is "function", but
/// Perry's INT32_TAG storage shape is shared with numeric int32, so the
/// runtime needs an explicit registry check. Consults both
/// REGISTERED_CLASS_IDS (every class) and CLASS_VTABLE_REGISTRY (classes
/// with methods) so even classes registered before the explicit-id call
/// runs still detect via the vtable.
pub fn is_class_id_registered(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = REGISTERED_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            if set.contains(&class_id) {
                return true;
            }
        }
    }
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .map(|m| m.contains_key(&class_id))
        .unwrap_or(false)
}

/// Function pointer type for dispatching method calls on handle-based objects.
/// Handle-based objects use small integer IDs (1, 2, 3...) instead of real heap pointers.
/// This is registered by perry-stdlib to dispatch to Fastify, ioredis, etc.
pub type HandleMethodDispatchFn = unsafe extern "C" fn(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64;

/// Function pointer type for dispatching property access on handle-based objects.
pub type HandlePropertyDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64;

/// Function pointer type for dispatching property set on handle-based objects.
pub type HandlePropertySetDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
);

/// #1545: probe for whether a numeric receiver is a live Web Streams handle.
/// Web Streams handles are returned as `id as f64` (a normal float), not the
/// subnormal bit-cast other handle subsystems use, so `js_native_call_method`
/// can't recognise them by bit pattern. This probe (registered by the stdlib)
/// lets it confirm a numeric whole-number receiver really is a stream handle
/// before routing the call to `handle_method_dispatch` — non-stream numbers
/// fall through to the normal `(number).x is not a function` TypeError.
pub type StreamHandleProbeFn = unsafe extern "C" fn(id: usize) -> bool;

/// #1545: classify a numeric Web Streams handle for `instanceof`.
/// Returns 0 = not a stream, 1 = ReadableStream, 2 = WritableStream,
/// 3 = reader, 4 = writer. Lets `x instanceof ReadableStream` /
/// `instanceof WritableStream` resolve for numeric stream handles
/// (`ts.readable`, `rs.pipeThrough(ts)`, …).
pub type StreamHandleKindProbeFn = unsafe extern "C" fn(id: usize) -> u8;

// Dispatch tables are written once at startup (by `js_register_handle_*_dispatch`)
// and read from many threads thereafter (perry/thread workers run user code that
// hits these). Stored as AtomicPtr to make reads/writes data-race-free; the
// underlying value is still a single function pointer with Option semantics
// (null = unset).
static HANDLE_METHOD_DISPATCH_PTR: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static HANDLE_PROPERTY_DISPATCH_PTR: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static HANDLE_PROPERTY_SET_DISPATCH_PTR: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static STREAM_HANDLE_PROBE_PTR: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static STREAM_HANDLE_KIND_PROBE_PTR: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[inline]
pub fn handle_method_dispatch() -> Option<HandleMethodDispatchFn> {
    let p = HANDLE_METHOD_DISPATCH_PTR.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandleMethodDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_property_dispatch() -> Option<HandlePropertyDispatchFn> {
    let p = HANDLE_PROPERTY_DISPATCH_PTR.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePropertyDispatchFn>(p) })
    }
}

#[inline]
pub fn handle_property_set_dispatch() -> Option<HandlePropertySetDispatchFn> {
    let p = HANDLE_PROPERTY_SET_DISPATCH_PTR.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), HandlePropertySetDispatchFn>(p) })
    }
}

/// Register a function to handle method calls on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_method_dispatch(f: HandleMethodDispatchFn) {
    HANDLE_METHOD_DISPATCH_PTR.store(f as *mut (), std::sync::atomic::Ordering::Release);
}

/// #1545: probe getter — see `StreamHandleProbeFn`.
#[inline]
pub fn stream_handle_probe() -> Option<StreamHandleProbeFn> {
    let p = STREAM_HANDLE_PROBE_PTR.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), StreamHandleProbeFn>(p) })
    }
}

/// #1545: register the Web Streams handle probe (called by the stdlib at init).
#[no_mangle]
pub unsafe extern "C" fn js_register_stream_handle_probe(f: StreamHandleProbeFn) {
    STREAM_HANDLE_PROBE_PTR.store(f as *mut (), std::sync::atomic::Ordering::Release);
}

/// #1545: kind-probe getter — see `StreamHandleKindProbeFn`.
#[inline]
pub fn stream_handle_kind_probe() -> Option<StreamHandleKindProbeFn> {
    let p = STREAM_HANDLE_KIND_PROBE_PTR.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute::<*mut (), StreamHandleKindProbeFn>(p) })
    }
}

/// #1545: register the Web Streams kind probe (called by the stdlib at init).
#[no_mangle]
pub unsafe extern "C" fn js_register_stream_handle_kind_probe(f: StreamHandleKindProbeFn) {
    STREAM_HANDLE_KIND_PROBE_PTR.store(f as *mut (), std::sync::atomic::Ordering::Release);
}

/// Register a function to handle property access on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_dispatch(f: HandlePropertyDispatchFn) {
    HANDLE_PROPERTY_DISPATCH_PTR.store(f as *mut (), std::sync::atomic::Ordering::Release);
}

/// Register a function to handle property set on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_set_dispatch(f: HandlePropertySetDispatchFn) {
    HANDLE_PROPERTY_SET_DISPATCH_PTR.store(f as *mut (), std::sync::atomic::Ordering::Release);
}

/// Register a class method in the vtable registry.
/// Called at startup from the init function for every class method/getter.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_method(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    param_count: i64,
) {
    let name = if name_ptr.is_null() || name_len <= 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.methods.insert(
        name,
        VTableMethodEntry {
            func_ptr: func_ptr as usize,
            param_count: param_count as u32,
        },
    );
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Register a class getter in the vtable registry.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_getter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    let name = if name_ptr.is_null() || name_len <= 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.getters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Register a class setter in the vtable registry.
///
/// Refs #486 (hono): hono's Context has `set res(_res) { ...; this.#res = _res;
/// this.finalized = true; }`. Without setter dispatch in `js_object_set_field_by_name`,
/// `c.res = response` from inside compose's `await handler(c, next)` chain stored
/// the response into a regular field slot but never ran the setter body — so
/// `this.finalized = true` never executed, `c.finalized` stayed false, and
/// hono-base's `if (!context.finalized) throw …` fired.
///
/// Setter signature: `fn(this_f64, value_f64) -> f64` (returns ignored, but
/// codegen emits a return so the LLVM signature matches a regular method body).
#[no_mangle]
pub unsafe extern "C" fn js_register_class_setter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    let name = if name_ptr.is_null() || name_len <= 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
        setters: HashMap::new(),
    });
    vtable.setters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

// ============================================================================
// Per-callsite-keyed inline cache for vtable method dispatch.
//
// `js_native_call_method` is the hot dispatch tower for cross-module class
// instance method calls (e.g. `archetype.set(...)` from CommandBuffer.execute
// in the ECS workloads). Per profile, ~12% of perf-comprehensive samples land
// in `core::hash::BuildHasher` from the per-call `HashMap.get(method_name)`
// SipHash on the vtable lookup.
//
// Cache key: `(class_id, method_name_ptr)` where `method_name_ptr` is the
// rodata byte-pointer perry-codegen passes for the interned method name. The
// pointer is stable across calls within a module, so its address acts as a
// faster identity than re-hashing the bytes. Different modules may produce
// different rodata copies of the same name — the cache simply gets one entry
// per (class_id, name_pointer) pair, no correctness impact.
//
// Invalidation: a global `VTABLE_GEN` atomic is bumped on every
// `js_register_class_method` / `js_register_class_getter`. Each cache entry
// records the gen at populate time; lookups skip stale entries. Registration
// is one-shot at init in practice, so steady-state lookups never miss on
// gen.
// ============================================================================

static VTABLE_GEN: AtomicU64 = AtomicU64::new(1);

const VTABLE_IC_SIZE: usize = 4096;
const VTABLE_IC_MASK: usize = VTABLE_IC_SIZE - 1;

#[repr(C)]
#[derive(Copy, Clone)]
struct VTableICEntry {
    gen: u64,
    class_id: u32,
    _pad: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
    _pad2: u32,
}

const EMPTY_VTABLE_IC_ENTRY: VTableICEntry = VTableICEntry {
    gen: 0,
    class_id: 0,
    _pad: 0,
    method_name_ptr: 0,
    func_ptr: 0,
    param_count: 0,
    _pad2: 0,
};

thread_local! {
    static VTABLE_IC: UnsafeCell<[VTableICEntry; VTABLE_IC_SIZE]> = const {
        UnsafeCell::new([EMPTY_VTABLE_IC_ENTRY; VTABLE_IC_SIZE])
    };
}

#[inline(always)]
fn vtable_ic_slot(class_id: u32, method_name_ptr: usize) -> usize {
    // Mix class_id into the upper bits of the pointer to spread (class, name)
    // pairs across slots. method_name_ptr is at least 1-byte aligned but
    // typically 8+ for rodata strings, so shift by 3 to drop the alignment
    // zeros before masking.
    let key = method_name_ptr
        .rotate_left(13)
        .wrapping_add((class_id as usize).wrapping_mul(0x9E37_79B9));
    (key >> 3) & VTABLE_IC_MASK
}

#[inline(always)]
pub(crate) unsafe fn vtable_ic_lookup(
    class_id: u32,
    method_name_ptr: usize,
) -> Option<(usize, u32)> {
    if method_name_ptr == 0 {
        return None;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &*cell.get();
        let entry = &cache[slot];
        if entry.gen == cur_gen
            && entry.class_id == class_id
            && entry.method_name_ptr == method_name_ptr
        {
            Some((entry.func_ptr, entry.param_count))
        } else {
            None
        }
    })
}

#[inline(always)]
pub(crate) unsafe fn vtable_ic_insert(
    class_id: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
) {
    if method_name_ptr == 0 {
        return;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &mut *cell.get();
        cache[slot] = VTableICEntry {
            gen: cur_gen,
            class_id,
            _pad: 0,
            method_name_ptr,
            func_ptr,
            param_count,
            _pad2: 0,
        };
    });
}

/// Call a vtable method with the correct arity.
/// All method params are f64, `this` is i64.
pub(crate) unsafe fn call_vtable_method(
    func_ptr: usize,
    this: i64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
) -> f64 {
    #[inline(always)]
    unsafe fn arg_or_nan(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len {
            *args_ptr.add(idx)
        } else {
            f64::NAN
        }
    }

    // LLVM-generated methods have signature `double(double this, double arg0, ...)`.
    // `this` is NaN-boxed as f64, so we must pass it as f64 — not i64 — to match
    // the calling convention. On ARM64 i64 and f64 share registers, so passing i64
    // works by accident; on Windows x64 ABI they use *different* registers (rcx vs
    // xmm0), causing segfaults when the method reads `this` from the wrong register.
    //
    // Issue #519: all call sites pass `this` as a RAW POINTER (the bottom-48-bit
    // address from `jsval.as_pointer()`). Bit-casting raw pointer bits to f64
    // produces a subnormal float (no NaN-box tag), which the method body
    // interprets as a number — every nested method call inside the body sees
    // `(number).<method>` and either returns garbage or throws TypeError via
    // the issue #510 catch-all (e.g. RegExpRouter.match → `this.buildAllMatchers()`
    // → "(number).buildAllMatchers is not a function" inside SmartRouter's
    // dispatch chain). NaN-box with POINTER_TAG before passing so the body
    // sees a real instance pointer.
    let this_f64: f64 = {
        let bits = this as u64;
        const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        if bits != 0 && bits <= PTR_MASK {
            // Raw pointer (no NaN-box tag) — wrap with POINTER_TAG so the
            // method body's `this` arrives as a real instance pointer.
            f64::from_bits(JSValue::pointer(bits as *mut u8).bits())
        } else {
            // Already NaN-boxed (top bits set) or null — pass through.
            f64::from_bits(bits)
        }
    };

    match param_count {
        0 => {
            let f: extern "C" fn(f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64)
        }
        1 => {
            let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0))
        }
        2 => {
            let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
            )
        }
        3 => {
            let f: extern "C" fn(f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
            )
        }
        4 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
            )
        }
        5 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
            )
        }
        6 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
            )
        }
        7 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
            )
        }
        8 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
                arg_or_nan(args_ptr, args_len, 7),
            )
        }
        9 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
                arg_or_nan(args_ptr, args_len, 7),
                arg_or_nan(args_ptr, args_len, 8),
            )
        }
        _ => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
                arg_or_nan(args_ptr, args_len, 7),
                arg_or_nan(args_ptr, args_len, 8),
                arg_or_nan(args_ptr, args_len, 9),
            )
        }
    }
}

/// Register a class with its parent class ID in the global registry
pub(crate) fn register_class(class_id: u32, parent_class_id: u32) {
    let mut registry = CLASS_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry.as_mut().unwrap().insert(class_id, parent_class_id);
}

/// Public registration entry point used by codegen module init.
///
/// The inline bump allocator (codegen-side `new ClassName()` lowering)
/// writes `parent_class_id` directly into the ObjectHeader and skips
/// the per-alloc `register_class` call that the runtime allocators
/// (`js_object_alloc_with_parent`, `js_object_alloc_class_inline_keys`,
/// etc.) make on every allocation. That breaks multi-level
/// `instanceof` chains: `class Square extends Rectangle extends Shape`
/// — `square instanceof Shape` walks the registry chain
/// `Square → Rectangle → Shape`, but if we never registered the
/// `Square → Rectangle` edge the walk stops immediately and returns
/// false.
///
/// Codegen now emits one call to this function per inheriting class
/// in the entry-block init prelude (after `__perry_init_strings_*`),
/// so the registry chain is fully populated before any user code runs.
#[no_mangle]
pub extern "C" fn js_register_class_parent(class_id: u32, parent_class_id: u32) {
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }
}

/// Issue #711: dynamic parent-class registration for
/// `class X extends fn(...)` shapes where the parent class_id is only
/// known at runtime. Called from codegen-emitted module-init code at
/// the source-order position of the class declaration (so the
/// extends expression's free variables — imports, top-level `let`s,
/// factory functions — are already initialized by the time we
/// evaluate the parent).
///
/// `parent_value` is the evaluated extends expression as a Perry
/// NaN-boxed value. We resolve a parent class_id from it via:
///   1. INT32-tagged ClassRef (the value `String$` produces) — the
///      payload IS the class_id, verified against REGISTERED_CLASS_IDS.
///   2. POINTER-tagged Object instance (the value a `make<T>(...)`
///      factory might return when it constructs and returns an
///      object) — read `class_id` from the ObjectHeader.
/// Anything else (closures, primitives, null/undefined) is a no-op:
/// the class stays parentless, identical to the pre-#711 behavior.
/// Self-registration (`parent_cid == class_id`) is rejected so a
/// recursive helper that returns its receiver can't create a cycle.
#[no_mangle]
pub extern "C" fn js_register_class_parent_dynamic(class_id: u32, parent_value: f64) {
    let bits = parent_value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

    let parent_cid: u32 = if tag == INT32_TAG {
        // ClassRef: lower 32 bits are the class id. Verify it's
        // actually a registered class id before trusting it.
        let payload = bits as u32;
        if payload == 0 {
            0
        } else {
            let guard = REGISTERED_CLASS_IDS.read().unwrap();
            match guard.as_ref() {
                Some(set) if set.contains(&payload) => payload,
                _ => 0,
            }
        }
    } else if tag == POINTER_TAG {
        // Object instance: read class_id from the ObjectHeader.
        let ptr = crate::value::js_nanbox_get_pointer(parent_value) as *const ObjectHeader;
        let from_obj = js_object_get_class_id(ptr);
        if from_obj != 0 {
            from_obj
        } else {
            // Issue #711 part 2: the value might be a closure whose
            // `.prototype` was assigned to an object via the
            // `function Base() {}; Base.prototype = X` pattern. Look
            // up the synthetic class id assigned at
            // `js_set_function_prototype` time. Returns 0 if the
            // closure has no registered prototype object — falls
            // through to the parentless baseline.
            function_class_id(parent_value)
        }
    } else {
        0
    };

    if parent_cid != 0 && parent_cid != class_id {
        register_class(class_id, parent_cid);
    }

    // #1788: when the parent is a per-evaluation class OBJECT (a class
    // expression value, POINTER-tagged), record it as `class_id`'s static
    // prototype so static-field lookups on the subclass walk to the parent
    // object's OWN per-evaluation static fields — effect's
    // `class Number$ extends make(numberKeyword) {}` → `Number$.ast`. Reuses
    // the CLASS_PROTOTYPE_OBJECTS map (the same #711/#809 vehicle), resolved
    // via `resolve_proto_chain_field`; the class_id parent edge above keeps
    // method/`new`/instanceof dispatch on the existing fast path.
    if tag == POINTER_TAG {
        let ptr = crate::value::js_nanbox_get_pointer(parent_value) as *mut ObjectHeader;
        if !ptr.is_null() && js_object_get_class_id(ptr as *const ObjectHeader) != 0 {
            let mut write = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
            if write.is_none() {
                *write = Some(HashMap::new());
            }
            write.as_mut().unwrap().insert(class_id, ptr as usize);
        } else if !ptr.is_null() && crate::closure::is_closure_ptr(ptr as usize) {
            // #36 / #321: the parent is a plain FUNCTION value (closure), e.g.
            // effect's `class Svc extends Context.Tag("Svc")<...>() {}`. Record
            // the closure-parent edge so static-field reads on the subclass
            // (`Svc.key`, `Svc._op`, `Svc[TagTypeId]`) walk to the parent
            // function's own props + ITS static prototype. The parent class_id
            // edge isn't wired (a closure carries no class_id), so this is the
            // only inheritance link for a function-valued superclass.
            let mut write = CLASS_PARENT_CLOSURES.write().unwrap();
            if write.is_none() {
                *write = Some(HashMap::new());
            }
            write.as_mut().unwrap().insert(class_id, ptr as usize);
        }
    }
}

/// #1789: stamp a freshly-allocated object as a heap "class object" (the
/// value a class EXPRESSION evaluates to). Sets `object_type =
/// OBJECT_TYPE_CLASS` so `typeof` reports "function" and `new`/`instanceof`
/// read `class_id` from it. Called by codegen right after `js_object_alloc`
/// in the `ClassExprFresh` lowering.
#[no_mangle]
pub extern "C" fn js_object_mark_class(obj: i64) {
    if obj != 0 {
        unsafe {
            (*(obj as *mut ObjectHeader)).object_type = crate::error::OBJECT_TYPE_CLASS;
        }
    }
}

/// #1789: is `ptr` a heap "class object" (`object_type == OBJECT_TYPE_CLASS`)?
/// Validates the GcHeader is a `GC_TYPE_OBJECT` before reading `object_type`,
/// so raw Map/Set/Buffer pointers (no GcHeader) are never misread. Used by
/// `typeof`, `new`, and `instanceof` to recognize a class value.
pub fn is_class_object_ptr(ptr: *const u8) -> bool {
    // Reject anything in the native-module handle range (< 0x100000). Those
    // are registry ids (net.Socket, zlib stream, crypto, fastify, ioredis,
    // timers, …) bit-OR'd with POINTER_TAG, not real heap pointers — real
    // objects always live well above 0x100000. The previous 0x1008 floor only
    // caught the tiny net/fastify id space; a mid-range handle (e.g. zlib's
    // 0x60000 stream base, #1843) sailed past it and this function then
    // segfaulted dereferencing `[handle - 8]` as a GcHeader. 0x100000 is the
    // same handle/real-pointer threshold `js_native_call_method` already uses.
    if ptr.is_null() || (ptr as usize) < 0x100000 {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
            && (*(ptr as *const ObjectHeader)).object_type == crate::error::OBJECT_TYPE_CLASS
    }
}

/// #1789: f64-value form of [`is_class_object_ptr`] — true only for a
/// POINTER-tagged value that is a class object.
pub fn is_class_object_value(value: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    jsval.is_pointer() && is_class_object_ptr(jsval.as_pointer::<u8>())
}

/// #1788: register a class STATIC method (`perry_static_*`, no `this` param)
/// in `CLASS_STATIC_METHODS`, keyed by the (template) class_id. Emitted by
/// codegen at module init alongside the instance-method vtable registration.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_method(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    param_count: i64,
    has_rest: i64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len <= 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = CLASS_STATIC_METHODS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .entry(class_id as u32)
        .or_default()
        .insert(name, (func_ptr as usize, param_count as u32, has_rest != 0));
}

/// Look up a static method by name in `CLASS_STATIC_METHODS`, walking the
/// class_id parent chain (so a subclass inherits a parent's static method).
pub(crate) fn lookup_static_method_in_chain(
    class_id: u32,
    name: &str,
) -> Option<(usize, u32, bool)> {
    let guard = CLASS_STATIC_METHODS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(m) = map.get(&cid) {
            if let Some(&entry) = m.get(name) {
                return Some(entry);
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Call a static method func_ptr with `args` (no `this` prepend — static
/// methods read `this` from the implicit-this slot, set by the caller).
/// Mirrors the arity dispatch of `call_vtable_method` minus the receiver arg.
unsafe fn call_static_method(
    func_ptr: usize,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
) -> f64 {
    #[inline(always)]
    unsafe fn a(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len {
            *args_ptr.add(idx)
        } else {
            f64::NAN
        }
    }
    match param_count {
        0 => (std::mem::transmute::<usize, extern "C" fn() -> f64>(func_ptr))(),
        1 => (std::mem::transmute::<usize, extern "C" fn(f64) -> f64>(func_ptr))(a(
            args_ptr, args_len, 0,
        )),
        2 => (std::mem::transmute::<usize, extern "C" fn(f64, f64) -> f64>(func_ptr))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
        ),
        3 => (std::mem::transmute::<usize, extern "C" fn(f64, f64, f64) -> f64>(func_ptr))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
        ),
        _ => (std::mem::transmute::<usize, extern "C" fn(f64, f64, f64, f64) -> f64>(func_ptr))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
            a(args_ptr, args_len, 3),
        ),
    }
}

unsafe fn try_native_static_method_in_proto_chain(
    class_id: u32,
    name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while cid != 0 && depth < 64 {
        let proto_obj = class_prototype_object(cid);
        if !proto_obj.is_null() && (*proto_obj).class_id == NATIVE_MODULE_CLASS_ID {
            if read_native_module_name(proto_obj as *const ObjectHeader).as_deref()
                == Some("buffer.Buffer")
            {
                let result = dispatch_native_module_method(proto_obj, name, args_ptr, args_len);
                if !JSValue::from_bits(result.to_bits()).is_undefined() {
                    return Some(result);
                }
            }
        }
        cid = get_parent_class_id(cid).unwrap_or(0);
        depth += 1;
    }
    None
}

/// #1788: dispatch a static method on a class value (`Sub.greet()` where
/// `Sub extends make(...)`, or a class-object value) by walking the class_id
/// parent chain in `CLASS_STATIC_METHODS`. Binds `this` to the receiver (so
/// `this.<field>` resolves through the subclass's static-field chain), calls
/// the method, and restores the previous implicit-this. On miss returns the
/// receiver unchanged — preserving the prior "yield the class ref for a
/// chained call during module init" behavior for genuinely-absent methods.
#[no_mangle]
pub unsafe extern "C" fn js_class_static_method_call(
    receiver: f64,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return receiver;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return receiver,
    };
    // Resolve the receiver's class_id: INT32 ClassRef payload, or the
    // class_id stamped on a POINTER class object's ObjectHeader.
    let bits = receiver.to_bits();
    let top16 = bits >> 48;
    let class_id = if top16 == 0x7FFE {
        (bits & 0xFFFF_FFFF) as u32
    } else if is_class_object_value(receiver) {
        let obj = crate::value::JSValue::from_bits(bits).as_pointer::<ObjectHeader>();
        js_object_get_class_id(obj)
    } else {
        0
    };
    if class_id == 0 {
        return receiver;
    }
    if let Some((func_ptr, param_count, has_rest)) = lookup_static_method_in_chain(class_id, name) {
        let prev_this = crate::object::js_implicit_this_set(receiver);
        let result = if has_rest {
            // `static foo(a, b, ...rest)` / `static pipe(...args)` (effect's
            // `pipe`/`dual`): pass the first `param_count-1` positional args
            // as-is, then bundle the remaining call args into a JS array for
            // the rest slot — matching JS `arguments`/rest semantics and the
            // direct-call (#1787 / #915) static-dispatch path.
            let fixed = (param_count as usize).saturating_sub(1);
            let arr = crate::array::js_array_alloc(args_len.saturating_sub(fixed) as u32);
            let mut i = fixed;
            while i < args_len {
                crate::array::js_array_push_f64(arr, *args_ptr.add(i));
                i += 1;
            }
            let rest_box = crate::value::js_nanbox_pointer(arr as i64);
            // Build the [param_count]-slot effective-args buffer:
            // positional fixed args, then the bundled rest array.
            let mut buf: Vec<f64> = Vec::with_capacity(param_count as usize);
            for j in 0..fixed {
                buf.push(if j < args_len {
                    *args_ptr.add(j)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                });
            }
            buf.push(rest_box);
            call_static_method(func_ptr, buf.as_ptr(), buf.len(), param_count)
        } else {
            call_static_method(func_ptr, args_ptr, args_len, param_count)
        };
        crate::object::js_implicit_this_set(prev_this);
        return result;
    }
    // #1787 / #321: not a static METHOD — try a static FIELD holding a
    // callable (effect's `static make = (...) => ...` / `static unify = ...`
    // on `SchemaAST.Union`). Walk the class_id chain in CLASS_DYNAMIC_PROPS
    // (where `js_class_register_static_field` records each static field) and,
    // if `name` resolves to a non-nullish value, invoke it as a closure with
    // the call args. Static-field arrows capture lexical `this` (the class) and
    // don't read dynamic `this`, so a plain closure call is correct. Without
    // this, `Class.staticField(args)` fell through to `receiver` (the class
    // ref / INT32 class id), which is why `Union.make([...])` returned `1`/
    // undefined and Schema decode died reading `_tag`.
    {
        let mut cid = class_id;
        let mut depth = 0u32;
        while cid != 0 && depth < 64 {
            let field_val = CLASS_DYNAMIC_PROPS
                .with(|m| m.borrow().get(&cid).and_then(|f| f.get(name).copied()));
            if let Some(v) = field_val {
                let fv = crate::value::JSValue::from_bits(v.to_bits());
                if !fv.is_undefined() && !fv.is_null() {
                    return crate::closure::js_native_call_value(v, args_ptr, args_len);
                }
            }
            cid = get_parent_class_id(cid).unwrap_or(0);
            depth += 1;
        }
    }
    if let Some(result) =
        try_native_static_method_in_proto_chain(class_id, name, args_ptr, args_len)
    {
        return result;
    }
    // True miss: no static method and no callable static field resolved on the
    // class chain. We hand back the receiver (load-bearing for effect's
    // `.pipe()`-during-init chains, #687) — but that silent class-ref is exactly
    // what surfaces downstream as a stray `1`. Surface it at the call site.
    report_dispatch_miss(
        "static-member-call",
        receiver,
        name,
        "the receiver (class ref)",
    );
    receiver
}

/// Look up parent class ID from the registry
pub(crate) fn get_parent_class_id(class_id: u32) -> Option<u32> {
    let registry = CLASS_REGISTRY.read().unwrap();
    registry.as_ref().and_then(|r| r.get(&class_id).copied())
}

/// Look up a method by name in the class vtable, walking the parent chain.
/// Returns `Some((func_ptr, param_count))` if found, `None` otherwise.
/// Used by `js_assimilate_thenable` (refs #586) and other runtime callers
/// that need to probe a class for a method without invoking it.
pub fn lookup_class_method_in_chain(class_id: u32, name: &str) -> Option<(usize, u32)> {
    let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
    let reg = registry.as_ref()?;
    let mut cur = class_id;
    for _ in 0..32 {
        if let Some(vt) = reg.get(&cur) {
            if let Some(entry) = vt.methods.get(name) {
                return Some((entry.func_ptr, entry.param_count));
            }
        }
        match get_parent_class_id(cur) {
            Some(pid) if pid != 0 => cur = pid,
            _ => return None,
        }
    }
    None
}
