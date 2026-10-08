use super::*;
use crate::{object::object_ops::throw_object_type_error, JSValue};
use std::collections::HashMap;
use std::sync::atomic::Ordering;

/// Register a class with its parent class ID in the global registry
pub(crate) fn register_class(class_id: u32, parent_class_id: u32) {
    // Re-registering an edge that is ALREADY registered with this same parent
    // changes nothing: the chain a reader walks is identical, so there is no
    // cached store plan to flush and no entry to publish.
    //
    // Every allocation of an inheriting class arrives here —
    // `object_alloc_class_inline_keys_impl` calls `register_class` whenever
    // `parent_class_id != 0`, and codegen ALSO emits one
    // `js_register_class_parent` per inheriting class in the init prelude, so
    // by the time user code allocates, the edge is always already there. The
    // work being skipped is a process-global `prop_plan` epoch bump (which
    // invalidates every cached store plan in the program) plus a write lock on
    // `CLASS_REGISTRY` and a map insert, per `new`. Measured on `new Sub()`
    // where `Sub extends Base`: 993 -> 901 instructions per allocation, and
    // 1,197 -> 1,105 for a two-level chain. The epoch bump's own cost is not in
    // those numbers: it is paid by every store site whose cached plan it threw
    // away, which a microbenchmark that allocates and nothing else cannot see.
    //
    // The read is the same dense indexed load every parent-chain walk uses; an
    // in-window child answers without touching the map at all. A genuinely new
    // or CHANGED edge falls through to the full publication below, so
    // re-parenting still flushes.
    if crate::object::class_meta_registry::get_parent_class_id(class_id) == Some(parent_class_id) {
        return;
    }

    // Parent linking changes what a class chain can intercept — flush cached
    // store plans (`object::prop_plan`).
    crate::object::prop_plan::prop_plan_epoch_bump();
    // An in-window child's edge goes to the dense table only, everything else
    // to the map (#11502) — no write lock for the common case.
    crate::object::class_meta_registry::publish_parent_edge(class_id, parent_class_id);
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

/// Resolve a class_id from an arbitrary NaN-boxed runtime VALUE: an INT32
/// `ClassRef` (the payload IS the class_id, verified registered) or a
/// POINTER-tagged object (its `ObjectHeader.class_id`, falling back to the
/// synthetic class id a plain closure's reassigned `.prototype` was given).
/// `0` for anything else (primitives, an unregistered closure, `undefined`,
/// `null`) — "no answer", never a wrong one.
///
/// Shared by `js_register_class_parent_dynamic` (deriving the class_id to
/// register a NEW parent edge) and `object/instanceof.rs`'s
/// `class_chain_reaches_dynamic` (#10624, walking an EXISTING
/// per-evaluation pin chain) — both need the identical "what class_id does
/// this value denote" answer.
pub(crate) fn dynamic_value_class_id(value: f64) -> u32 {
    let bits = value.to_bits();
    const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == INT32_TAG {
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
    } else if let Some(class_id) = crate::object::class_value::class_value_id_bits(bits) {
        // A class function object names its class.
        class_id
    } else if tag == POINTER_TAG {
        // Object instance: read class_id from the ObjectHeader.
        let ptr = crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader;
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
            function_class_id(value)
        }
    } else {
        0
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
/// recursive helper that returns its receiver can't create a cycle — and the
/// VALUE stash below applies the same rejection (`is_self_heritage_value`).
#[no_mangle]
pub extern "C" fn js_register_class_parent_dynamic(class_id: u32, parent_value: f64) {
    register_class_parent_dynamic(class_id, parent_value, true);
    // ClassDefinitionEvaluation fixes the instance prototype edge now,
    // before a later assignment can replace the superclass's prototype.
    // Store that edge on the existing prototype object, not a second table.
    class_decl_prototype_value(class_id);
}

pub(crate) fn register_class_parent_dynamic(
    class_id: u32,
    parent_value: f64,
    publish_shared: bool,
) {
    // Stash the parent VALUE keyed by child class id so `super()` can read it
    // back (`js_get_dynamic_parent_value`) instead of re-evaluating the extends
    // expression inside the constructor scope. The decl-time call here runs in
    // the module-init scope where the extends expression's free variables
    // (require aliases such as `_suffix` in `class X extends _suffix.default`)
    // are bound. Skip undefined (the bare placeholder) — a genuinely undefined
    // superclass throws below anyway.
    {
        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        let bits = parent_value.to_bits();
        if bits != TAG_UNDEFINED && class_id != 0 && !super::is_self_heritage_value(class_id, bits)
        {
            CLASS_DYNAMIC_PARENT_VALUE.with(|table| {
                let mut guard = table.write().unwrap();
                if guard.is_none() {
                    *guard = Some(HashMap::new());
                }
                guard.as_mut().unwrap().insert(class_id, bits);
            });
        }
    }
    // A globalThis builtin constructor closure is a valid superclass
    // (`class CloseEvent extends Event` — the `ws` package's WebSocket
    // events). Resolve it through the same name table the dynamic
    // `instanceof` path uses and register the edge when the builtin has a
    // runtime class id, so subclass instances satisfy `instanceof Event`
    // and Event-shaped dispatch gates. Builtins without a class id keep the
    // parentless baseline (no throw — they ARE constructors).
    if let Some(name) = identify_global_builtin_constructor(parent_value) {
        // `%Proxy%` is constructable but intentionally has no usable
        // `prototype` property, so it fails ClassDefinitionEvaluation's
        // prototype-object-or-null check.
        if name == "Proxy" {
            super::super::object_ops::throw_object_type_error(
                b"Class extends value has invalid prototype property",
            );
        }
        let parent_cid = super::super::instanceof::global_builtin_constructor_class_id(name);
        if parent_cid != 0 && parent_cid != class_id {
            register_class(class_id, parent_cid);
        }
        // A dynamic subclass that resolves its parent through this builtin
        // branch must still record the fetch-parent kind so `new X()` attaches
        // the native Request/Response handle — the bookkeeping below this
        // early return would otherwise be skipped.
        match name {
            "Request" => super::super::register_fetch_parent_kind(class_id, 1),
            "Response" => super::super::register_fetch_parent_kind(class_id, 2),
            _ => super::super::data_view_registry::register_builtin_view_parent(class_id, name),
        }
        return;
    }
    // A bound native-module export (`const { Writable } = require('stream');
    // class Receiver extends Writable` — the `ws` package's shape) is a real
    // Node constructor even though Perry models it as a BOUND_METHOD closure.
    // Keep the parentless baseline rather than mis-throwing; native-parent
    // method inheritance is handled by codegen's extends_name machinery, not
    // by this registry edge.
    if let Some((module, method)) = unsafe {
        super::super::native_module::bound_native_callable_module_and_method(parent_value)
    } {
        if !super::super::native_module::is_native_module_constructor_export(&module, &method) {
            throw_object_type_error(b"Class extends value is not a constructor");
        }
        let module = super::super::native_module::normalize_native_module_alias(&module);
        if module == "wasi" && method == "WASI" {
            register_class(class_id, crate::wasi::CLASS_ID_WASI);
        }
        if module == "async_hooks" {
            let parent = match method.as_str() {
                "AsyncLocalStorage" => 0xFFFF0078,
                "AsyncResource" => 0xFFFF0079,
                _ => 0,
            };
            if parent != 0 {
                register_class(class_id, parent);
            }
        }
        if module == "events" || (module == "stream" && method == "Stream") {
            // #10430: the legacy `Stream` constructor extends EventEmitter, so a
            // `class X extends require('stream')` subclass inherits the same
            // EventEmitter parent edge (`new X() instanceof EventEmitter`).
            //
            // #10798: `Stream` gets its OWN hop in the chain — the reserved id
            // `instanceof/static_dispatch.rs` already uses to NAME 0xFFFF0070
            // as "Stream" — rather than collapsing straight onto EventEmitter's
            // id. `js_instanceof` walks the full class-id chain
            // (`subclass_of_builtin_reaches` / `class_chain_reaches`), so
            // registering `class_id -> CLASS_ID_STREAM -> CLASS_ID_EVENT_EMITTER`
            // keeps `instanceof EventEmitter` true transitively while making
            // `instanceof Stream` true ONLY for a genuine `extends Stream`
            // subclass — a plain `extends EventEmitter` class (registered
            // directly on 0xFFFF0076, no Stream hop) must NOT satisfy
            // `instanceof Stream`, and collapsing both onto the same id would
            // have made it. The Stream->EventEmitter edge is registered on
            // every call; `register_class` no-ops when the edge already
            // matches, so this is idempotent.
            match method.as_str() {
                "EventEmitter" => register_class(class_id, 0xFFFF0076),
                "Stream" => {
                    const CLASS_ID_STREAM: u32 = 0xFFFF0070;
                    const CLASS_ID_EVENT_EMITTER: u32 = 0xFFFF0076;
                    register_class(CLASS_ID_STREAM, CLASS_ID_EVENT_EMITTER);
                    register_class(class_id, CLASS_ID_STREAM);
                }
                "EventEmitterAsyncResource" => register_class(class_id, 0xFFFF0077),
                _ => {}
            }
        }
        // A native superclass is also the constructor's actual [[Prototype]].
        // Keep this edge on the class function shape, alongside instance
        // heritage, so static reads and their receivers use ordinary lookup.
        if publish_shared && !crate::object::class_value::class_value_is_first_evaluation(class_id)
        {
            let scope = crate::gc::RuntimeHandleScope::new();
            let parent = scope.root_nanbox_f64(parent_value);
            // Materialize the child before passing a raw parent to the store.
            crate::object::class_value::class_value_ptr(class_id);
            class_static_prototype_root_store(
                class_id,
                crate::value::js_nanbox_get_pointer(parent.get_nanbox_f64()) as *mut ObjectHeader,
            );
        }
        return;
    }
    // Spec: a non-`null` superclass that is not a constructor throws a TypeError
    // at class-definition time (before any `.prototype` access). (Test262
    // subclass/superclass-* and definition/invalid-extends.)
    if extends_target_must_throw(parent_value) {
        throw_object_type_error(b"Class extends value is not a constructor");
    }

    // The shared prototype birth performs the ordinary superclass.prototype
    // Get and validates object-or-null. Repeating it here for bound functions
    // would invoke an observable getter twice during one class definition.

    let bits = parent_value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

    let parent_cid: u32 = dynamic_value_class_id(parent_value);

    if parent_cid != 0 && parent_cid != class_id {
        register_class(class_id, parent_cid);
    }

    // Record whether the parent value is the global Request/Response
    // constructor (possibly via an alias like `GlobalRequest = global.Request`),
    // resolved here in the scope where the alias is live. The runtime
    // dynamic-construction path (`new (classExprValue)(...)`) consults this to
    // attach the underlying native fetch handle on the instance — the static
    // codegen `super()` path can't, because the textual parent name is the
    // alias, not "Request". Refs `@hono/node-server`'s `class Request extends
    // GlobalRequest`.
    match identify_global_builtin_constructor(parent_value) {
        Some("Request") => super::super::register_fetch_parent_kind(class_id, 1),
        Some("Response") => super::super::register_fetch_parent_kind(class_id, 2),
        _ => {}
    }

    // #1788: when the parent is a per-evaluation class OBJECT (a class
    // expression value, POINTER-tagged), record it as `class_id`'s static
    // prototype so static-field lookups on the subclass walk to the parent
    // object's OWN per-evaluation static fields — effect's
    // `class Number$ extends make(numberKeyword) {}` → `Number$.ast`. Reuses
    // the CLASS_PROTOTYPE_OBJECTS map (the same #711/#809 vehicle), resolved
    // via `resolve_proto_chain_field`; the class_id parent edge above keeps
    // method/`new`/instanceof dispatch on the existing fast path.
    // #11759 (c′): a later evaluation pins its parent on its own class object;
    // the template's static parent stays the first evaluation's.
    if publish_shared
        && tag == POINTER_TAG
        && !crate::object::class_value::class_value_is_first_evaluation(class_id)
    {
        let ptr = crate::value::js_nanbox_get_pointer(parent_value) as *mut ObjectHeader;
        if !ptr.is_null() && js_object_get_class_id(ptr as *const ObjectHeader) != 0 {
            class_prototype_object_root_store(class_id, ptr);
        } else if !ptr.is_null() && crate::closure::is_closure_ptr(ptr as usize) {
            // #36 / #321: the parent is a plain FUNCTION value (closure), e.g.
            // effect's `class Svc extends Context.Tag("Svc")<...>() {}`. Record
            // the closure-parent edge so static-field reads on the subclass
            // (`Svc.key`, `Svc._op`, `Svc[TagTypeId]`) walk to the parent
            // function's own props + ITS static prototype. The parent class_id
            // edge isn't wired (a closure carries no class_id), so this is the
            // only inheritance link for a function-valued superclass.
            class_parent_closure_root_store(class_id, ptr as usize);
        }
    }
}

/// Own-property key under which a per-evaluation class object
/// (`ClassExprFresh`) pins ITS OWN parent class value. See
/// `js_class_evaluation_object`.
pub(crate) const CLASS_OBJECT_PARENT_KEY: &str = "__perry_parent_class";

/// The ordinary path of one evaluation's class object
/// (`js_class_evaluation_object`): give `obj`, a newborn class object of
/// template `template_class_id`, its own `length`, `name` and static methods
/// (with or without heritage), then pin `parent`, this evaluation's heritage,
/// onto it. Without the pin, a factory invoked more than once (effect's
/// `class DeclareClass extends make(ast) { … }`) has every instance walk to
/// the LAST parent (#6438). `record` sees the finished object and its parent.
/// Returns `obj`'s current address.
pub(crate) unsafe fn class_object_define_members(
    obj: *mut crate::object::ObjectHeader,
    template_class_id: u32,
    static_field_mask: u32,
    parent: f64,
    record: &dyn Fn(*mut crate::object::ObjectHeader, f64),
) -> *mut crate::object::ObjectHeader {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if obj.is_null() || template_class_id == 0 {
        return obj;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_raw_mut_ptr(obj);
    let parent = scope.root_nanbox_f64(parent);
    class.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
        crate::object::field_get_set::define_class_object_own_properties(obj, static_field_mask);
    });
    if parent.get_nanbox_f64().to_bits() != TAG_UNDEFINED {
        // #10624: arm BEFORE the write it advertises (the ordering rule in
        // `registry_latch.rs`) — everything the latch gates (this own-property
        // write, and `pin_instance_constructing_class`'s later instance pin,
        // which never fires without this one already having happened) follows
        // in this thread's program order.
        super::evaluation_heritage::CLASS_OBJECT_HERITAGE_PIN_LATCH.arm();
        let key_bytes = CLASS_OBJECT_PARENT_KEY.as_bytes();
        let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        let key = scope.root_string_ptr(key);
        class.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                crate::object::define_builtin_data_property(
                    obj,
                    key,
                    parent.get_nanbox_f64(),
                    CLASS_OBJECT_PARENT_KEY.to_string(),
                    PropertyAttrs::new(true, true, true),
                )
            })
        });
    }
    class.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
        record(obj, parent.get_nanbox_f64());
        obj
    })
}

/// Read back the parent pinned by `js_class_evaluation_object`, or `None` when
/// this class object has no own parent edge.
///
/// Scans the keys array DIRECTLY rather than going through the by-name read
/// path: that path consults the pinned parent itself (that is the whole point
/// of the edge), so reading the marker through it re-enters this function and
/// recurses until the stack guard page — an immediate SIGSEGV. A re-entrancy
/// flag is not an option either: it would abort the legitimate
/// parent→grandparent walk of a multi-level factory chain. An own-only scan has
/// neither problem, and it is cheap: the pinned key sits among a handful of own
/// statics on a class object.
pub(crate) fn class_object_pinned_parent(obj: *const crate::object::ObjectHeader) -> Option<f64> {
    class_object_own_field_bytes(obj, CLASS_OBJECT_PARENT_KEY.as_bytes())
}

/// OWN-ONLY field read on a class object: scans the keys array directly and
/// consults no prototype chain, no registry, and no pinned parent.
///
/// Needed because `get_field_by_name_object_tail` folds the own lookup together
/// with a class_id-keyed prototype-chain walk. For a per-evaluation class object
/// that chain resolves through the TEMPLATE's parent edge — which is last-wins —
/// so it answers with a sibling evaluation's inherited value instead of this
/// object's. Callers that must order "own, then MY pinned parent, then the
/// generic tail" need the own half in isolation.
pub(crate) fn class_object_own_field_bytes(
    obj: *const crate::object::ObjectHeader,
    want: &[u8],
) -> Option<f64> {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    // `is_valid_obj_ptr` alone does not reject the fetch/zlib/proxy handle
    // bands, and dereferencing a handle id as an ObjectHeader segfaults on Linux
    // (macOS hides it). Gate on `is_above_handle_band` first — a real class
    // object is always a heap allocation above the band.
    if obj.is_null()
        || !crate::value::addr_class::is_above_handle_band(obj as usize)
        || !crate::object::is_valid_obj_ptr(obj as *const u8)
    {
        return None;
    }
    unsafe {
        let keys_view = crate::object::object_keys(obj);
        let keys = keys_view.arr();
        if keys.is_null() {
            return None;
        }
        // #10724: raw dense slots (one resolve, not the JS-facing element
        // accessor per key — 2.8 M `js_array_get_f64` calls on a native `tsc`),
        // compared in place: the old `js_get_string_pointer_unified` heap-
        // materialized every short-string key it passed, allocating while
        // `keys` was held as a bare pointer.
        let (slots, slot_len) = crate::object::keys_array_dense_slots(keys);
        for i in 0..(keys_view.count() as usize).min(slot_len) {
            let k = crate::JSValue::from_bits((*slots.add(i)).to_bits());
            if !crate::string::js_string_key_matches_bytes(k, want) {
                continue;
            }
            // An own ACCESSOR's slot holds its pair, not a value: it reads as
            // absent here, and the caller's `[[Get]]` runs the getter.
            let v = crate::object::key_attrs::object_slot_data(obj, i as u32);
            if v.bits() == TAG_UNDEFINED {
                return None;
            }
            return Some(f64::from_bits(v.bits()));
        }
    }
    None
}

/// Does class object `obj` have an own property `want`, whatever it holds (a
/// data value, `undefined` included, or an accessor)?
pub(crate) fn class_object_owns_key_bytes(
    obj: *const crate::object::ObjectHeader,
    want: &[u8],
) -> bool {
    if obj.is_null()
        || !crate::value::addr_class::is_above_handle_band(obj as usize)
        || !crate::object::is_valid_obj_ptr(obj as *const u8)
    {
        return false;
    }
    unsafe {
        let keys_view = crate::object::object_keys(obj);
        let keys = keys_view.arr();
        if keys.is_null() {
            return false;
        }
        let (slots, slot_len) = crate::object::keys_array_dense_slots(keys);
        (0..(keys_view.count() as usize).min(slot_len)).any(|i| {
            crate::string::js_string_key_matches_bytes(
                crate::JSValue::from_bits((*slots.add(i)).to_bits()),
                want,
            )
        })
    }
}

/// Read back the parent constructor value stashed at class-definition time by
/// `js_register_class_parent_dynamic` (see `CLASS_DYNAMIC_PARENT_VALUE`).
/// `super()` in a `class X extends <runtime-value>` body uses this so the
/// parent is resolved from the value captured in the module-init scope, not
/// re-evaluated in the constructor scope (where an IIFE-local require alias
/// like `_suffix` in `extends _suffix.default` is not in scope). Returns
/// `undefined` when nothing was stashed for this class id — the caller then
/// falls back to re-evaluating its extends expression.
#[no_mangle]
pub extern "C" fn js_get_dynamic_parent_value(class_id: u32) -> f64 {
    // #9364: while a per-evaluation class OBJECT's constructor is being
    // replayed, THAT evaluation's pinned heritage is the answer. The stash
    // below is keyed by the shared TEMPLATE id and is last-wins, so for a
    // factory whose class is evaluated more than once it names one evaluation's
    // parent for all of them — and when that parent is an earlier evaluation of
    // the same template, `super()` resolves to it at every level and re-enters
    // the same constructor forever.
    if let Some(parent) = super::active_class_evaluation_parent(class_id) {
        return parent;
    }
    template_dynamic_parent_value(class_id)
}

/// The TEMPLATE-keyed heritage: the `js_register_class_parent_dynamic` stash,
/// then the static parent-id edge as a ClassRef. This is
/// [`js_get_dynamic_parent_value`] without its per-evaluation override, for the
/// callers that must see what the class DEFINITION recorded rather than what
/// the constructor currently running inherits.
pub(crate) fn template_dynamic_parent_value(class_id: u32) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if class_id == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // #11759 (c′): a first evaluation keeps its static heritage.
    if !crate::object::class_value::class_value_is_first_evaluation(class_id) {
        if let Some(parent) = super::stashed_dynamic_parent_value(class_id) {
            return parent;
        }
    }
    // #5957/#806: no dynamic VALUE stashed — fall back to the STATIC
    // parent-id edge as a ClassRef. An `extends <call>(...)` mixin
    // materialized by the HIR inline-init can resolve the chain statically
    // (module init registers `js_register_class_parent(child, parent)`)
    // while the per-value `js_register_class_parent_dynamic` side effect
    // lived in a body that never runs; before this fallback the
    // dynamic-parent super leg dispatched `undefined` and silently no-op'd
    // — the ancestor ctor never saw the forwarded args (the #806 mixin's
    // `seed` stayed undefined). A ClassRef routes the caller into the
    // registered-constructor flat dispatch, which fills user args and
    // snapshot caps by the signature split.
    if let Some(parent_cid) = crate::object::get_parent_class_id(class_id) {
        // Only a compiled parent class has a class function object; a
        // builtin parent id (`extends Error`) never gets one.
        if parent_cid != 0 && crate::object::is_class_id_registered(parent_cid) {
            return crate::object::class_value::class_value(parent_cid);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// #1789: stamp a freshly-allocated object as a heap "class object" (the
/// value a class EXPRESSION evaluates to). Transitions the authoritative
/// ShapeId descriptor kind. #8113 deleted the `object_type` compatibility
/// mirror this also used to write; the descriptor kind is the only record.
/// Called by codegen right after `js_object_alloc` in the `ClassExprFresh`
/// lowering.
#[no_mangle]
pub extern "C" fn js_object_mark_class(obj: i64) {
    if obj != 0 {
        unsafe {
            let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
                return;
            };
            if header.obj_type != crate::gc::GC_TYPE_OBJECT
                || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            {
                return;
            }
            // Becoming a class object changes dispatch semantics even though
            // the rooted keys and slot layout stay the same.
            crate::object::shapes::transition_object_shape_to_class(obj as *mut ObjectHeader);
            // #6530: record cid → class object so `instance.constructor`
            // resolves to the SAME value the module scope/exports hold (see
            // `CLASS_OBJECT_VALUES`). The template cid was stamped by the
            // `js_object_alloc(cid, …)` call directly preceding this mark.
            let cid = (*(obj as *const ObjectHeader)).class_id;
            // #10501: a class object is a ClassDefinitionEvaluation of `cid`;
            // private accesses of that template now need their full brand
            // resolution (see `note_private_template_evaluated`).
            crate::object::field_get_set::note_private_template_evaluated(cid);
            super::class_object_value_root_store(cid, obj as *mut ObjectHeader);
        }
    }
}

/// #1789: is `ptr` a heap class object?
/// Validates the GcHeader is a live `GC_TYPE_OBJECT` before reading its ShapeId,
/// so raw Map/Set/Buffer pointers (no GcHeader) are never misread. Used by
/// `typeof`, `new`, and `instanceof` to recognize a class value.
pub fn is_class_object_ptr(ptr: *const u8) -> bool {
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_gc_header(ptr as usize) else {
            return false;
        };
        header.obj_type == crate::gc::GC_TYPE_OBJECT
            && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && object_shape_kind_is_class(ptr)
    }
}

/// [`is_class_object_ptr`] for a caller that already holds `ptr`'s header.
///
/// # Safety
/// `header` is the GcHeader of the cell at `ptr`, obtained from a checked
/// reader (`try_read_gc_header` / `try_read_tracked_gc_header`).
pub(crate) unsafe fn is_class_object_with_header(
    ptr: *const u8,
    header: &crate::gc::GcHeader,
) -> bool {
    header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && object_shape_kind_is_class(ptr)
}

/// Does the ShapeId of the live, unforwarded object at `ptr` name a class
/// object?
///
/// # Safety
/// `ptr` is a live `GC_TYPE_OBJECT` cell that has not been forwarded.
#[inline(always)]
unsafe fn object_shape_kind_is_class(ptr: *const u8) -> bool {
    crate::object::shapes::object_shape_descriptor(ptr.cast())
        .is_some_and(|shape| shape.object_kind == crate::object::shapes::ShapeObjectKind::Class)
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
    {
        let mut guard = CLASS_STATIC_METHODS.write().unwrap();
        if guard.is_none() {
            *guard = Some(crate::fast_hash::new_ptr_hash_map());
        }
        guard
            .as_mut()
            .unwrap()
            .entry(class_id as u32)
            .or_default()
            .entry(name.clone())
            .and_modify(|e| {
                (e.0, e.1, e.2) = (func_ptr as usize, param_count as u32, has_rest != 0)
            })
            .or_insert((func_ptr as usize, param_count as u32, has_rest != 0, 0));
    }
    crate::object::class_value::note_intrinsic_registration(class_id as u32, &name);
}

/// Record the `JsFunctionInfo` of the closure-convention entry
/// `<static body>__clo(callee, this, args...)` codegen emitted for ClassBody static method `name` of class `class_id`:
/// the body of the method's own function object (one per class and method).
/// Its arity, length and strictness are facts of that info; its name and source
/// were registered on the code.
/// Emitted at module init after `js_register_class_static_method`.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_static_method_entry(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    entry: i64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len <= 0 || entry == 0 {
        return;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize))
    else {
        return;
    };
    {
        let mut guard = CLASS_STATIC_METHODS.write().unwrap();
        let Some(record) = guard
            .as_mut()
            .and_then(|all| all.get_mut(&(class_id as u32)))
            .and_then(|m| m.get_mut(name))
        else {
            return;
        };
        record.3 = entry as usize;
    }
    crate::object::class_value::note_intrinsic_registration(class_id as u32, name);
}

fn property_key_string(key: f64) -> Option<String> {
    let property_key = unsafe { crate::object::js_to_property_key(key) };
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        return None;
    }
    let str_ptr = crate::value::js_jsvalue_to_string(property_key);
    if str_ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        Some(std::str::from_utf8(bytes).unwrap_or("").to_string())
    }
}

/// Register a computed-key ClassBody method when the class definition
/// evaluates its key. A static one with a string key is a ClassBody static
/// method like any other: `entry` is the `JsFunctionInfo` of its
/// closure-convention entry (`<body>__clo`), the body of its own function object, whose `name` is the
/// key.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn js_register_class_computed_method(
    class_id: i64,
    key: f64,
    func_ptr: i64,
    param_count: i64,
    is_static: i64,
    has_rest: i64,
    definition_order: i64,
    entry: i64,
) {
    if class_id == 0 || func_ptr == 0 {
        return;
    }
    let property_key = crate::object::js_to_property_key(key);
    let class_id = class_id as u32;
    if crate::symbol::js_is_symbol(property_key) != 0 {
        let sym_key = crate::symbol::sym_key_from_f64(property_key);
        if sym_key == 0 {
            return;
        }
        crate::symbol::note_symbol_key_installed(sym_key);
        super::registration::record_class_symbol_member_order(
            class_id,
            sym_key,
            is_static != 0,
            definition_order as u32,
        );
        CLASS_SYMBOL_METHODS.with(|table| {
            let mut guard = table.write().unwrap();
            if guard.is_none() {
                *guard = Some(HashMap::new());
            }
            guard.as_mut().unwrap().insert(
                (class_id, sym_key, is_static != 0),
                (func_ptr as usize, param_count as u32, has_rest != 0),
            );
        });
        // A computed key that evaluates to a WELL-KNOWN symbol — e.g. the
        // minified `[(gm = new WeakMap, Symbol.asyncIterator)]() {…}` comma
        // form, whose key expression the lowering can't see through
        // statically — must land in the same synthetic vtable slot the
        // static `[Symbol.asyncIterator]` lowering uses. Every consumer
        // (GetIterator(async), the #5128 symbol-read binder,
        // `js_to_primitive`, the using-block desugar) resolves these by the
        // synthetic NAME on the class; `CLASS_SYMBOL_METHODS` above is not
        // consulted for instance dispatch. Without the alias,
        // `for await (… of instance)` threw `TypeError: value is not
        // iterable` for the comma-keyed form.
        if is_static == 0 {
            let alias = [
                ("iterator", "@@iterator"),
                ("asyncIterator", "@@asyncIterator"),
                ("toPrimitive", "@@toPrimitive"),
                ("dispose", "__perry_dispose__"),
                ("asyncDispose", "__perry_async_dispose__"),
            ]
            .iter()
            .find_map(|(wk, method_name)| {
                let s = crate::symbol::well_known_symbol(wk);
                if s.is_null() {
                    return None;
                }
                let f = f64::from_bits(crate::value::JSValue::pointer(s as *const u8).bits());
                if sym_key == crate::symbol::sym_key_from_f64(f) {
                    Some(*method_name)
                } else {
                    None
                }
            })
            .or_else(|| {
                (sym_key == crate::symbol::inspect_custom_symbol_ptr())
                    .then_some("__perry_inspect_custom__")
            });
            if let Some(method_name) = alias {
                let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
                if registry.is_none() {
                    *registry = Some(crate::fast_hash::new_ptr_hash_map());
                }
                let vtable = registry.as_mut().unwrap().entry(class_id).or_default();
                vtable.methods.insert(
                    method_name.to_string(),
                    VTableMethodEntry {
                        func_ptr: func_ptr as usize,
                        param_count: param_count as u32,
                        has_synthetic_arguments: false,
                        has_rest: has_rest != 0,
                        entry: 0,
                    },
                );
            }
        }
        let proto = super::state::class_decl_prototype_object(class_id);
        if is_static == 0 && !proto.is_null() {
            super::state::install_class_decl_prototype_symbol_member(proto, class_id, sym_key);
        }
        VTABLE_GEN.fetch_add(1, Ordering::Release);
        return;
    }
    let name = match property_key_string(property_key) {
        Some(name) => name,
        None => return,
    };
    super::registration::record_class_string_member_order(
        class_id,
        name.clone(),
        is_static != 0,
        definition_order as u32,
    );
    if is_static != 0 && name == "prototype" {
        throw_object_type_error(b"Classes may not have a static property named 'prototype'");
    }
    if is_static != 0 {
        {
            let mut guard = CLASS_STATIC_METHODS.write().unwrap();
            if guard.is_none() {
                *guard = Some(crate::fast_hash::new_ptr_hash_map());
            }
            guard
                .as_mut()
                .unwrap()
                .entry(class_id)
                .or_default()
                .entry(name.clone())
                .and_modify(|e| {
                    (e.0, e.1, e.2, e.3) = (
                        func_ptr as usize,
                        param_count as u32,
                        has_rest != 0,
                        entry as usize,
                    )
                })
                .or_insert((
                    func_ptr as usize,
                    param_count as u32,
                    has_rest != 0,
                    entry as usize,
                ));
        }
        if entry != 0 {
            // SetFunctionName(F, key): the key is known only now.
            // The name is keyed by the body's code; `entry` is its info.
            crate::builtins::js_register_function_name(
                (*(entry as *const crate::closure::JsFunctionInfo)).code,
                name.as_ptr(),
                name.len() as u32,
            );
        }
        crate::object::class_value::note_intrinsic_registration(class_id, &name);
    } else {
        let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
        if registry.is_none() {
            *registry = Some(crate::fast_hash::new_ptr_hash_map());
        }
        let vtable = registry.as_mut().unwrap().entry(class_id).or_default();
        vtable.methods.insert(
            name.clone(),
            VTableMethodEntry {
                func_ptr: func_ptr as usize,
                param_count: param_count as u32,
                // Computed class methods don't carry synthetic-`arguments`
                // metadata through this registration path (only `has_rest`),
                // so they never receive a synthesized arguments object.
                has_synthetic_arguments: false,
                has_rest: has_rest != 0,
                entry: 0,
            },
        );
        // Backfill when reflection already materialized `C.prototype`.
        drop(registry);
        let proto = class_decl_prototype_object(class_id);
        super::state::install_class_decl_prototype_method_field(proto, class_id, &name);
    }
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

#[no_mangle]
pub unsafe extern "C" fn js_register_class_computed_accessor(
    class_id: i64,
    key: f64,
    getter_ptr: i64,
    setter_ptr: i64,
    is_static: i64,
    definition_order: i64,
) {
    if class_id == 0 || (getter_ptr == 0 && setter_ptr == 0) {
        return;
    }
    let property_key = crate::object::js_to_property_key(key);
    let class_id = class_id as u32;
    if crate::symbol::js_is_symbol(property_key) != 0 {
        let sym_key = crate::symbol::sym_key_from_f64(property_key);
        if sym_key == 0 {
            return;
        }
        crate::symbol::note_symbol_key_installed(sym_key);
        super::registration::record_class_symbol_member_order(
            class_id,
            sym_key,
            is_static != 0,
            definition_order as u32,
        );
        CLASS_SYMBOL_ACCESSORS.with(|table| {
            let mut guard = table.write().unwrap();
            if guard.is_none() {
                *guard = Some(HashMap::new());
            }
            let entry = guard
                .as_mut()
                .unwrap()
                .entry((class_id, sym_key, is_static != 0))
                .or_insert((0, 0));
            if getter_ptr != 0 {
                entry.0 = getter_ptr as usize;
            }
            if setter_ptr != 0 {
                entry.1 = setter_ptr as usize;
            }
        });
        let proto = super::state::class_decl_prototype_object(class_id);
        if is_static == 0 && !proto.is_null() {
            super::state::install_class_decl_prototype_symbol_member(proto, class_id, sym_key);
        }
        VTABLE_GEN.fetch_add(1, Ordering::Release);
        return;
    }
    if let Some(name) = property_key_string(property_key) {
        super::registration::record_class_string_member_order(
            class_id,
            name.clone(),
            is_static != 0,
            definition_order as u32,
        );
        if is_static != 0 && name == "prototype" {
            throw_object_type_error(b"Classes may not have a static property named 'prototype'");
        }
        if is_static == 0 {
            let newly_declared = class_own_accessor_ptrs(class_id, &name).is_none();
            let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
            if registry.is_none() {
                *registry = Some(crate::fast_hash::new_ptr_hash_map());
            }
            let vtable = registry.as_mut().unwrap().entry(class_id).or_default();
            vtable.declare_accessor_half(&name, getter_ptr as usize, false);
            vtable.declare_accessor_half(&name, setter_ptr as usize, true);
            drop(registry);
            super::decl_accessors::note_instance_accessor_registered(
                class_id,
                &name,
                newly_declared,
            );
        } else {
            {
                let mut guard = CLASS_STATIC_ACCESSORS.write().unwrap();
                if guard.is_none() {
                    *guard = Some(crate::fast_hash::new_ptr_hash_map());
                }
                let entry = guard
                    .as_mut()
                    .unwrap()
                    .entry(class_id)
                    .or_default()
                    .entry(name.clone())
                    .or_default();
                if getter_ptr != 0 {
                    entry.get = getter_ptr as usize;
                }
                if setter_ptr != 0 {
                    entry.set = setter_ptr as usize;
                }
            }
            crate::object::class_value::note_intrinsic_registration(class_id, &name);
        }
    }
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Look up a static method by name in `CLASS_STATIC_METHODS`, walking the
/// class_id parent chain (so a subclass inherits a parent's static method).
/// Own-only static method lookup (no parent-chain walk) — for
/// `getOwnPropertyDescriptor(C, name)`, where inherited statics must NOT be
/// reported as own properties of `C`.
pub(crate) fn class_has_own_static_method(class_id: u32, name: &str) -> bool {
    CLASS_STATIC_METHODS
        .read()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .and_then(|m| m.get(&class_id).map(|inner| inner.contains_key(name)))
        })
        .unwrap_or(false)
}

/// ClassBody static method `name` declared by class `class_id` itself:
/// `(func_ptr, param_count, has_rest)`.
pub(crate) fn class_own_static_method_entry(
    class_id: u32,
    name: &str,
) -> Option<(usize, u32, bool)> {
    let guard = CLASS_STATIC_METHODS.read().ok()?;
    let e = guard.as_ref()?.get(&class_id)?.get(name).copied()?;
    Some((e.0, e.1, e.2))
}

/// The closure-convention entry of ClassBody static method `name` declared by
/// class `class_id` itself: the code of its own function object.
pub(crate) fn class_own_static_method_code(class_id: u32, name: &str) -> Option<usize> {
    let guard = CLASS_STATIC_METHODS.read().ok()?;
    let e = guard.as_ref()?.get(&class_id)?.get(name).copied()?;
    (e.3 != 0).then_some(e.3)
}

/// The static method `name` a call on class `class_id` runs: the nearest
/// declaration whose own property on its class's function object is still
/// that declaration. A deleted one is skipped (the parent's applies); a
/// redefined one ends the lookup (the property's value is what runs).
pub(crate) fn lookup_static_method_in_chain(
    class_id: u32,
    name: &str,
) -> Option<(usize, u32, bool)> {
    lookup_static_method_owner(class_id, name).map(|(_, e)| e)
}

/// [`lookup_static_method_in_chain`] plus the class whose declaration runs.
/// The walk reads the class function objects: a class whose object owns
/// `name` (declared, assigned, or deleted and reassigned) ends it — its
/// declaration when the property still is that declaration's function,
/// otherwise nothing (the property's value is what a call runs).
pub(crate) fn lookup_static_method_owner(
    class_id: u32,
    name: &str,
) -> Option<(u32, (usize, u32, bool))> {
    use crate::object::class_value::StaticMethodProperty;
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        let entry = {
            let guard = CLASS_STATIC_METHODS.read().ok()?;
            guard.as_ref()?.get(&cid).and_then(|m| m.get(name)).copied()
        };
        match crate::object::class_value::static_method_property(cid, name, entry.map(|e| e.3)) {
            StaticMethodProperty::Live => {
                return entry.map(|e| (cid, (e.0, e.1, e.2)));
            }
            StaticMethodProperty::Replaced => return None,
            StaticMethodProperty::Deleted => {}
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

/// Spec `Function.prototype.length` for a class method named `name` — the
/// count of formal parameters, excluding a trailing rest param and the
/// synthesized `arguments` slot (neither contributes to `.length`). Walks the
/// instance vtable chain, then the static-method table. Used to stamp the
/// bound-method closure's length so `C.prototype.m.length` is correct
/// (Test262 .../class/{gen,async}-method/...-trailing-comma + length tests).
/// Note: does not subtract for default-valued params (the registry doesn't
/// record the first-default position); methods with defaults already reported
/// the wrong length, so this is a strict improvement, never a regression.
pub(crate) fn class_method_bind_length(class_id: u32, name: &str) -> Option<u32> {
    // Exact spec length (default-aware) when codegen recorded it; walk the
    // parent chain so an inherited method's `.length` resolves too.
    if let Ok(guard) = CLASS_METHOD_BIND_LENGTHS.read() {
        if let Some(map) = guard.as_ref() {
            let mut cid = class_id;
            let mut depth = 0usize;
            while cid != 0 && depth < 32 {
                if let Some(&len) = map.get(&(cid, name.to_string())) {
                    return Some(len);
                }
                match get_parent_class_id(cid) {
                    Some(p) if p != 0 && p != cid => {
                        cid = p;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
    }
    if let Ok(guard) = CLASS_VTABLE_REGISTRY.read() {
        if let Some(reg) = guard.as_ref() {
            let mut cid = class_id;
            let mut depth = 0usize;
            while cid != 0 && depth < 32 {
                if let Some(vt) = reg.get(&cid) {
                    if let Some(e) = vt.methods.get(name) {
                        let mut len = e.param_count;
                        if e.has_rest {
                            len = len.saturating_sub(1);
                        }
                        if e.has_synthetic_arguments {
                            len = len.saturating_sub(1);
                        }
                        return Some(len);
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
        }
    }
    // Static methods: prefer the default-aware spec length recorded by codegen
    // (params before the first default/rest), walking the parent chain; fall
    // back to the raw `CLASS_STATIC_METHODS` param_count otherwise.
    if let Ok(guard) = CLASS_STATIC_METHOD_BIND_LENGTHS.read() {
        if let Some(map) = guard.as_ref() {
            let mut cid = class_id;
            let mut depth = 0usize;
            while cid != 0 && depth < 32 {
                if let Some(&len) = map.get(&(cid, name.to_string())) {
                    return Some(len);
                }
                match get_parent_class_id(cid) {
                    Some(p) if p != 0 && p != cid => {
                        cid = p;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
    }
    // CLASS_STATIC_METHODS stores (func_ptr, param_count, has_rest).
    if let Some((_, param_count, has_rest)) = lookup_static_method_in_chain(class_id, name) {
        let mut len = param_count;
        if has_rest {
            len = len.saturating_sub(1);
        }
        return Some(len);
    }
    None
}

/// Call a static method func_ptr with `args` (no `this` prepend — static
/// methods resolve `this` through `js_static_this_resolve`).
/// Mirrors the arity dispatch of `call_vtable_method` minus the receiver arg.
pub(crate) unsafe fn call_static_method(
    func_ptr: usize,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
) -> f64 {
    // Missing trailing args pad with `undefined` (NOT NaN) so default
    // parameters fire — see `call_vtable_method::arg_or_undefined`.
    #[inline(always)]
    unsafe fn a(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len {
            *args_ptr.add(idx)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
    match param_count {
        0 => (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8;))(),
        1 => (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0))(a(
            args_ptr, args_len, 0,
        )),
        2 => (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0, a1))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
        ),
        3 => (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0, a1, a2))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
        ),
        4 => (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0, a1, a2, a3))(
            a(args_ptr, args_len, 0),
            a(args_ptr, args_len, 1),
            a(args_ptr, args_len, 2),
            a(args_ptr, args_len, 3),
        ),
        5 => {
            (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0, a1, a2, a3, a4))(
                a(args_ptr, args_len, 0),
                a(args_ptr, args_len, 1),
                a(args_ptr, args_len, 2),
                a(args_ptr, args_len, 3),
                a(args_ptr, args_len, 4),
            )
        }
        6 => {
            (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0, a1, a2, a3, a4, a5))(
                a(args_ptr, args_len, 0),
                a(args_ptr, args_len, 1),
                a(args_ptr, args_len, 2),
                a(args_ptr, args_len, 3),
                a(args_ptr, args_len, 4),
                a(args_ptr, args_len, 5),
            )
        }
        7 => {
            (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0, a1, a2, a3, a4, a5, a6))(
                a(args_ptr, args_len, 0),
                a(args_ptr, args_len, 1),
                a(args_ptr, args_len, 2),
                a(args_ptr, args_len, 3),
                a(args_ptr, args_len, 4),
                a(args_ptr, args_len, 5),
                a(args_ptr, args_len, 6),
            )
        }
        _ => {
            (crate::closure::body_call::js_bare_body_fn!(func_ptr as *const u8; a0, a1, a2, a3, a4, a5, a6, a7))(
                a(args_ptr, args_len, 0),
                a(args_ptr, args_len, 1),
                a(args_ptr, args_len, 2),
                a(args_ptr, args_len, 3),
                a(args_ptr, args_len, 4),
                a(args_ptr, args_len, 5),
                a(args_ptr, args_len, 6),
                a(args_ptr, args_len, 7),
            )
        }
    }
}

pub(crate) unsafe fn call_registered_static_method(
    func_ptr: usize,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
    has_rest: bool,
) -> f64 {
    if has_rest {
        let fixed = (param_count as usize).saturating_sub(1);
        let arr = crate::array::js_array_alloc(args_len.saturating_sub(fixed) as u32);
        let mut i = fixed;
        while i < args_len {
            crate::array::js_array_push_f64(arr, *args_ptr.add(i));
            i += 1;
        }
        let rest_box = crate::value::js_nanbox_pointer(arr as i64);
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
    }
}

unsafe fn try_native_static_method_in_proto_chain(
    class_id: u32,
    name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    // NOTE: deliberately NOT routed through `NmNamespaceOps` — a
    // `class X extends Buffer` registers the parent value before the lazy
    // callable-exports closure would arm the table (probe-verified: the
    // hooked form broke `MyBuf.from`), so this probe must stay static.
    nm_static_buffer_proto_chain(class_id, name, args_ptr, args_len)
}

/// Body of the Buffer-subclass static-dispatch probe (#1788 family).
pub(crate) unsafe fn nm_static_buffer_proto_chain(
    class_id: u32,
    name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0u32;
    while cid != 0 && depth < 64 {
        if let Some(parent_addr) = class_parent_closure(cid) {
            let parent_value = crate::value::js_nanbox_pointer(parent_addr as i64);
            if is_buffer_constructor_value(parent_value) {
                let module = b"buffer.Buffer";
                let ns = js_create_native_module_namespace(module.as_ptr(), module.len());
                let ns_obj = JSValue::from_bits(ns.to_bits()).as_pointer::<ObjectHeader>();
                let result = crate::object::native_module::call_native_module_dispatch_hook(
                    ns_obj, name, args_ptr, args_len,
                );
                if !JSValue::from_bits(result.to_bits()).is_undefined() {
                    return Some(result);
                }
            }
        }
        let proto_obj = class_prototype_object(cid);
        if !proto_obj.is_null()
            && (*proto_obj).class_id == NATIVE_MODULE_CLASS_ID
            && read_native_module_name(proto_obj as *const ObjectHeader).as_deref()
                == Some("buffer.Buffer")
        {
            let result = crate::object::native_module::call_native_module_dispatch_hook(
                proto_obj, name, args_ptr, args_len,
            );
            if !JSValue::from_bits(result.to_bits()).is_undefined() {
                return Some(result);
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
/// `this.<field>` resolves through the subclass's static-field chain) and calls
/// the method. On miss returns the
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
    let storage_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return receiver,
    };
    let private_hint = if storage_name.starts_with("#<perry:private-member:") {
        super::super::field_get_set::take_private_method_call_hint(storage_name)
    } else {
        None
    };
    let _owner = super::super::field_get_set::PrivateHintBrandScope::new(
        private_hint.as_ref().and_then(|(_, _, _, owner)| *owner),
    );
    let private_name =
        private_hint.and_then(|(_, is_static, name, _)| is_static.then(|| name.to_string()));
    let name = private_name.as_deref().unwrap_or(storage_name);
    // Resolve the receiver's class_id: INT32 ClassRef payload, or the
    // class_id stamped on a POINTER class object's ObjectHeader.
    let bits = receiver.to_bits();
    let top16 = bits >> 48;
    let _ = top16;
    let class_id = if let Some(cid) = crate::object::class_value::legacy_class_value_word(bits) {
        cid
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
        crate::object::static_private_owner_push(receiver);
        // Receiver-sensitive static `this`: arm the one-shot override so the
        // method prologue (`js_static_this_resolve`) sees the DYNAMIC receiver
        // (e.g. subclass `D` for an inherited `D.f()`). If an outer
        // call/apply already armed an explicit thisArg, that wins.
        crate::object::static_this_arm_if_unarmed(receiver);
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
        crate::object::static_this_disarm();
        crate::object::static_private_owner_pop();
        return result;
    }
    // #10893: not a static METHOD — a static ACCESSOR on the class-id chain
    // whose value is callable. See `try_static_accessor_value_call`.
    if let Some(result) =
        try_static_accessor_value_call(class_id, name, receiver, args_ptr, args_len)
    {
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
            let field_val = crate::object::class_value::class_static_get(cid, name);
            if let Some(v) = field_val {
                let fv = crate::value::JSValue::from_bits(v.to_bits());
                if !fv.is_undefined() && !fv.is_null() {
                    return crate::closure::js_native_call_value(
                        v,
                        crate::closure::JsThis::from_f64(receiver),
                        args_ptr,
                        args_len,
                    );
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
    // `Object.setPrototypeOf(Ctor, obj)` put a plain object on the
    // CONSTRUCTOR's prototype chain, so `Ctor.method(...)` resolves through it
    // (Effect's `Schema.Opaque`). Walk the registered parent chain the same way
    // the static FIELD lookup above does, reading each level's recorded
    // constructor prototype, and invoke the first callable found with `this`
    // bound to the original receiver.
    {
        let mut cid = class_id;
        let mut depth = 0u32;
        while cid != 0 && depth < 32 {
            let static_proto = super::class_static_prototype(cid);
            if !static_proto.is_null() {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let member = super::super::field_get_set::js_object_get_field_by_name(
                    static_proto as *const ObjectHeader,
                    key,
                );
                let member = f64::from_bits(member.bits());
                let mv = crate::value::JSValue::from_bits(member.to_bits());
                if !mv.is_undefined() && !mv.is_null() {
                    let result = crate::closure::native_call_value_this(
                        member,
                        crate::closure::JsThis::from_f64(receiver),
                        args_ptr,
                        args_len,
                    );
                    return result;
                }
            }
            match get_parent_class_id(cid) {
                Some(parent) if parent != 0 && parent != cid => cid = parent,
                _ => break,
            }
            depth += 1;
        }
    }
    // `class X extends Promise` — inherited builtin static (`X.all(...)`,
    // `X.resolve(...)`, …). Dispatch the spec static with `this` = the subclass
    // receiver so `NewPromiseCapability(X)` constructs the subclass. Resolves the
    // reified static value and calls it with `receiver` as its `this`.
    if super::promise_parent_in_chain(class_id)
        && crate::object::promise_static_function_spec(name).is_some()
    {
        let static_val = crate::object::js_promise_static_function_value(name.as_ptr(), name.len());
        if static_val.to_bits() != crate::value::TAG_UNDEFINED {
            // The reified static thunk reads its `this` constructor from its
            // `this` argument, so pass the subclass receiver —
            // `NewPromiseCapability(receiver)` then constructs the subclass.
            let result = crate::closure::native_call_value_this(
                static_val,
                crate::closure::JsThis::from_f64(receiver),
                args_ptr,
                args_len,
            );
            return result;
        }
    }
    // #7541: `class X extends Array` — inherited builtin static
    // (`X.from(...)`, `X.of(...)`, `X.isArray(...)`).
    //
    // `Array.from` / `Array.of` are folded in the HIR on the LITERAL identifier
    // `Array` (`lower/expr_call/array_only_methods.rs`), so a subclass receiver
    // never matched and `MyArr.from([1, 2, 3])` resolved to nothing. The
    // fallback at the end of this function returns the RECEIVER unchanged, so
    // the call evaluated to the class ref — and `[...MyArr.from([1,2,3])]` threw
    // `TypeError: value is not iterable` (#7541's report), which looked like a
    // spread bug but is a missing static.
    //
    // Both spec statics are already implemented constructor-aware
    // (`array::{array_from_full, array_of_full}` run `Construct(C, …)` when
    // `IsConstructor(this)`, and `class_ref_id` makes a class ref answer true),
    // so passing the subclass receiver as `this` builds a subclass instance —
    // matching `Array.from.call(MyArr, …)`. Mirrors the Promise arm above.
    if crate::array::is_array_subclass_class_id(class_id) {
        let arg = |i: usize| -> f64 {
            if i < args_len && !args_ptr.is_null() {
                *args_ptr.add(i)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            }
        };
        match name {
            "from" => {
                return crate::array::array_from_full(receiver, arg(0), arg(1), arg(2));
            }
            "of" => {
                let vals: &[f64] = if args_ptr.is_null() || args_len == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(args_ptr, args_len)
                };
                return crate::array::array_of_full(receiver, vals);
            }
            "isArray" => {
                return crate::array::js_array_is_array(arg(0));
            }
            _ => {}
        }
    }
    // Inherited function properties and constructor prototypes use the same
    // receiver-aware Get as a member read. A fresh class evaluation pins its
    // heritage on the class object, not in the shared template's parent-closure
    // metadata; a separate class-id walk therefore loses that edge. Keeping
    // Get here also preserves accessor receivers and ordinary shadowing.
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(receiver);
        let args = if args_ptr.is_null() || args_len == 0 {
            Vec::new()
        } else {
            scope.root_nanbox_f64_slice(std::slice::from_raw_parts(args_ptr, args_len))
        };
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let member = crate::object::js_object_get_property_key(
            receiver.get_nanbox_f64(),
            crate::value::js_nanbox_string(key as i64),
        );
        if crate::collection_iter::is_callable(member) {
            let member = scope.root_nanbox_f64(member);
            let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
            return crate::closure::native_call_value_this(
                member.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
                args.as_ptr(),
                args.len(),
            );
        }
    }
    // True miss: no static method and no callable static field resolved on the
    // class chain. Keep the two compatibility no-ops introduced for Effect's
    // schema initialization (#687), but otherwise follow JavaScript semantics:
    // calling an absent member throws instead of silently returning the class.
    // In particular, this is observable when code deliberately probes a class
    // with an unknown method inside `assert.throws`.
    // A class inherits `Object.prototype` through `Function.prototype`; with
    // no static of that name on its chain, these two are the builtins.
    if matches!(name, "hasOwnProperty" | "propertyIsEnumerable") {
        let key = if args_len >= 1 && !args_ptr.is_null() {
            *args_ptr
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        return if name == "hasOwnProperty" {
            crate::object::js_object_has_own(receiver, key)
        } else {
            crate::object::js_object_property_is_enumerable(receiver, key)
        };
    }
    report_dispatch_miss(
        "static-member-call",
        receiver,
        name,
        if matches!(name, "pipe" | "annotations") {
            "the receiver (compatibility no-op)"
        } else {
            "TypeError"
        },
    );
    if matches!(name, "pipe" | "annotations") {
        return receiver;
    }
    crate::error::js_throw_type_error_not_a_function(std::ptr::null(), 0, name.as_ptr(), name.len())
}

// `get_parent_class_id` now lives in `object::class_meta_registry` next to the
// dense parent table it reads; it is re-exported through `object::mod` unchanged.
pub(crate) use crate::object::class_meta_registry::get_parent_class_id;

/// Look up a method by name in the class vtable, walking the parent chain.
/// Returns `Some((func_ptr, param_count, has_synthetic_arguments, has_rest))`
/// if found, `None` otherwise.
/// Used by `js_assimilate_thenable` (refs #586) and other runtime callers
/// that need to probe a class for a method without invoking it.
///
/// A declared method removed from its class's materialized prototype object
/// (`delete C.prototype.m`) is not provided by that class: the prototype
/// object's own keys are the truth, the vtable entry only names the body.
/// The walk then continues to the parent, as the JS prototype chain does.
pub fn lookup_class_method_in_chain(class_id: u32, name: &str) -> Option<(usize, u32, bool, bool)> {
    let mut cur = class_id;
    for _ in 0..32 {
        let found = {
            let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
            let reg = registry.as_ref()?;
            reg.get(&cur)
                .and_then(|vt| vt.methods.get(name))
                .map(|entry| {
                    (
                        entry.func_ptr,
                        entry.param_count,
                        entry.has_synthetic_arguments,
                        entry.has_rest,
                    )
                })
        };
        if let Some(entry) = found {
            // Checked with the registry lock released: the deletedness probe
            // reads the class tables again.
            if !super::class_proto_key_deleted(cur, name) {
                return Some(entry);
            }
        }
        match instance_chain_parent_class_id(cur) {
            Some(pid) => cur = pid,
            None => return None,
        }
    }
    None
}

/// The next class on an INSTANCE chain after `cid`: the declared parent,
/// unless a user operation (`Object.setPrototypeOf(C.prototype, X)`,
/// `C.prototype.__proto__ = X`) replaced the `[[Prototype]]` of `cid`'s
/// prototype object. That prototype's recorded link is then the chain, and a
/// walk over declared class members must stop at `cid`: the parent's methods,
/// getters and setters are off the chain (the generic read continues on the
/// recorded link). The static side (`C.__proto__`) is a different object and
/// keeps the declared parent.
///
/// The relink check runs only when a declared parent exists, so a walk that
/// answers from the receiver's own class, or reaches a root class, pays
/// nothing for it.
#[inline]
pub(crate) fn instance_chain_parent_class_id(cid: u32) -> Option<u32> {
    match get_parent_class_id(cid) {
        Some(pid) if pid != 0 && !super::class_decl_prototype_relinked(cid) => Some(pid),
        _ => None,
    }
}

/// True when `ptr` is the prototype OBJECT of some registered class. Class
/// methods are installed as own fields on the prototype object, so a method-as-
/// value read whose receiver *is* the prototype must return the shared canonical
/// method value (for identity), not the raw stored field — i.e. the own-property
/// shadow rule applies to genuine instances, not to the prototype itself.
pub fn is_registered_class_prototype_object(ptr: usize) -> bool {
    if crate::value::addr_class::is_handle_band(ptr) {
        return false;
    }
    crate::object::class_registry::class_prototype_object_addr_index_contains(ptr)
}

/// Walk the prototype chain of `class_id` and return the id of the class that
/// actually OWNS the method `name` (the prototype where it is defined). Used to
/// make method-as-value identity stable: a class method is a single shared
/// function object, so every read of it — `c.m`, `C.prototype.m`, `c2.m` —
/// must resolve to the canonical value keyed by the OWNING class, not the
/// (possibly derived) class of the receiver. Returns `None` when no class in
/// the chain declares the method.
pub fn method_owner_class_id(class_id: u32, name: &str) -> Option<u32> {
    let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
    let reg = registry.as_ref()?;
    let mut cur = class_id;
    for _ in 0..32 {
        if let Some(vt) = reg.get(&cur) {
            if vt.methods.contains_key(name) {
                return Some(cur);
            }
        }
        match instance_chain_parent_class_id(cur) {
            Some(pid) => cur = pid,
            None => return None,
        }
    }
    None
}

#[cfg(test)]
#[path = "parent_static/unstamped_tests.rs"]
mod unstamped_tests;

#[cfg(test)]
#[path = "parent_static/shape_authority_tests_8067.rs"]
mod shape_authority_tests_8067;

include!("parent_static/private_and_dynamic.rs");
include!("parent_static/static_accessor_call.rs");

mod symbol_members;
pub(crate) use symbol_members::{
    class_has_symbol_member_in_chain, class_own_symbol_member_keys, class_symbol_getter_value,
    class_symbol_setter_apply, lookup_class_symbol_method_in_chain,
};
