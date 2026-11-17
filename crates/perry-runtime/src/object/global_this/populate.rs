use super::*;

/// Apply the builtin's declared identity and its members' declared permanent
/// images to the finished holder. No public name participates in either fact.
fn finish_builtin_prototype(
    realm: &crate::array::RealmIntrinsics,
    declaration: &BuiltinConstructorDeclaration,
    proto_obj: *mut ObjectHeader,
) {
    // These ordinary native receivers carry CLASS identities, just
    // like declared-class instances. Publish the intrinsic holder in
    // the identity's GC-traced word when it becomes available.
    if let Some(class) = declaration.prototype_class.filter(|_| crate::array::is_thread_realm(realm)) {
        crate::object::shapes::write_identity_word(
            crate::object::shapes::PROTO_ID_CLASS | u64::from(class),
            crate::value::js_nanbox_pointer(proto_obj as i64).to_bits(),
        );
    }
    // Every builtin prototype, after the fit (its live-bound
    // transition keeps no ConstFn lane): the shape names the body of
    // each member whose function info declares a permanent image
    // (`FN_PERMANENT_IMAGE`), as an ordinary key-add of that function
    // object would. The declaration on the member decides, never the
    // prototype's name; a store or delete of the member revokes its
    // lane (a new ShapeId).
    // A prototype born on its declared layout keeps that layout id: its
    // members are its layout's slots, never lanes of a new id.
    // SAFETY: the live, just-populated prototype.
    unsafe {
        let learn = |obj: *mut ObjectHeader| {
            if !crate::object::runtime_layouts::on_declared_prototype_layout(obj) {
                crate::object::shapes::learn_object_constfn_lanes(obj, |_, _| true);
            }
        };
        if crate::value::addr_class::try_read_gc_header(proto_obj as usize)
            .is_some_and(|h| crate::gc_type_is_object!(h.obj_type))
        {
            learn(proto_obj);
        } else {
            crate::object::shapes::learn_object_constfn_lanes(proto_obj, |_, _| true);
        }
        // Some native prototypes own symbol slots through an ordinary
        // descriptor holder. Its members carry the same declarations.
        if let Some(holder) = crate::object::shaped_symbols::owner(proto_obj as usize) {
            if holder != proto_obj {
                learn(holder);
            }
        }
    }
}

#[cfg(test)]
#[path = "declaration_tests.rs"]
mod declaration_tests;

#[cfg(test)]
#[path = "declaration_keys_tests.rs"]
mod declaration_keys_tests;
#[cfg(test)]
#[path = "lazy_family_tests.rs"]
mod lazy_family_tests;

/// Bind a realm's constructors and global members. Object and Function are
/// root families; each other family keeps an unborn constructor until its
/// first observation asks for its members.
pub(crate) fn populate_realm_global(singleton_at_entry: *mut ObjectHeader) {
    // Every install below is a builtin definition, which arms the own-override
    // guard only on a Map, Set or Date owner (#10697).
    super::super::own_override::as_builtin_definition(|| {
        populate_realm_global_inner(singleton_at_entry)
    })
}
crate::realm_decl! { Infinity, value = Infinity; NaN, value = NaN; crypto, attrs = 6, get = "crypto.<get>", births = [ArrayBuffer, Crypto, CryptoKey, Promise, SubtleCrypto]; global, attrs = 7, alias = "%Global%"; globalThis, alias = "%Global%"; localStorage, attrs = 4, get = "localStorage.<get>", set = "localStorage.<set>"; sessionStorage, attrs = 6, get = "sessionStorage.<get>", set = "sessionStorage.<set>"; undefined, value = undefined; }
// Compact the once-per-realm installer, not the methods it installs. Blanket
// Oz also shrinks/outlines hot array and iterator helpers (#13662).
#[optimize(size)]
fn populate_realm_global_inner(singleton_at_entry: *mut ObjectHeader) {
    if singleton_at_entry.is_null() {
        return;
    }
    // The realm is the runtime's, whichever init first asks for it.
    let _realm = crate::snap_census::runtime_scope();
    // Installers hold constructor, prototype and member pointers across
    // allocations. Keep the graph still until its edges are published in
    // the global and its intrinsic record; rooting the global alone cannot
    // rewrite those raw locals.
    let _no_move = crate::gc::GcSuppressScope::new();
    let bootstrap_started = crate::gc::diagnostic_hooks::enabled().then(std::time::Instant::now);
    let scope = crate::gc::RuntimeHandleScope::new();
    let singleton_handle = scope.root_raw_mut_ptr(singleton_at_entry);
    let singleton = || singleton_handle.get_raw_mut_ptr::<ObjectHeader>();
    let realm = unsafe { super::realm_of(singleton()) };
    {
        let name = b"globalThis";
        let key = super::global_name_key(name);
        let value = crate::value::js_nanbox_pointer(singleton() as i64);
        super::super::define_builtin_data_property(
            singleton(),
            key,
            value,
            "globalThis".to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    {
        // #4511: Node exposes the global object as `global` too
        // (`global === globalThis`). Install the same self-reference so bare
        // `global` / `(global as any).x` reads resolve to the real singleton
        // instead of the unknown-identifier sentinel.
        let name = b"global";
        let key = super::global_name_key(name);
        let value = crate::value::js_nanbox_pointer(singleton() as i64);
        super::super::define_builtin_data_property(
            singleton(),
            key,
            value,
            "global".to_string(),
            super::super::PropertyAttrs::new(true, true, true),
        );
    }
    // Setup binds each realm's constructors; their members are born on observation.
    for declaration in GLOBAL_THIS_BUILTIN_CONSTRUCTORS {
        let name = declaration.name;
        if name == "Object" {
            // Object is a root family. A global adopts the pair already in
            // its record, including the main pair born before its global.
            let (object_ctor, _) = object_intrinsics_for_realm(realm);
            if !object_ctor.is_null() {
                let name_key = super::global_name_key(b"Object");
                super::super::define_builtin_data_property(
                    singleton(),
                    name_key,
                    crate::value::js_nanbox_pointer(object_ctor as i64),
                    name.to_string(),
                    super::super::PropertyAttrs::new(true, false, true),
                );
            }
            continue;
        }
        // Bind the constructor its record already holds, including a family
        // born before the realm global.
        // Function is a root family, born by its own setup birth.
        if !declaration.lazy {
            crate::array::intrinsic_prototype_addr_in(realm, declaration.identity);
        }
        let bound = crate::array::intrinsic_constructor_addr_if_bound_in(realm, declaration.identity);
        let closure_ptr = if bound != 0 {
            bound as *mut crate::closure::ClosureHeader
        } else {
            super::super::global_this_tables::alloc_builtin_constructor(declaration.identity, realm)
        };
        if closure_ptr.is_null() {
            continue;
        }
        // A lazy family keeps only its constructor at setup: the realm's
        // [[Intrinsics]] entry holds it (prototype half 0) and the global
        // binds it below; `birth_lazy_family` births the rest on first ask.
        let ctor_value = if bound != 0 {
            crate::value::js_nanbox_pointer(closure_ptr as i64)
        } else {
            unborn_constructor(realm, declaration.identity, closure_ptr)
        };
        // A family %Global% does not bind (`global false`: %TypedArray%)
        // is held by the realm's [[Intrinsics]] entry only.
        if !declaration.global {
            continue;
        }
        if name == "Storage" {
            // Node: `localStorage` / `sessionStorage` are accessors whose
            // getters make the areas; bound here, ahead of `Storage`.
            crate::web_storage::install_global_accessors(singleton());
        }
        let name_bytes = name.as_bytes();
        let name_key = super::global_name_key(name_bytes);
        super::super::define_builtin_data_property(
            singleton(),
            name_key,
            ctor_value,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
        super::super::native_module::publish_worker_messaging_constructor(
            singleton(),
            name,
            ctor_value,
        );
    }
    // Interface inheritance carries prototype methods. DOMException has
    // Error.prototype as its instance parent, but no Error constructor parent.
    // A lazy child links in its own birth (`link_interface_parent`).
    // Callable global functions: ClosureHeader-backed values with real
    // dispatch so direct property reads and rebound calls match bare calls.
    // Each row is born only in a program that can observe it (GLOBAL_BINDING_ROWS); an unarmed
    // row keeps its key, value undefined.
    for row in GLOBAL_BINDING_ROWS.iter().filter(|row| !row.namespace) {
        define_global_binding(singleton(), row, armed_global_binding_value(row, singleton()));
    }
    // Namespaces: plain ObjectHeader so typeof is "object" per spec.
    for name in GLOBAL_THIS_BUILTIN_NAMESPACES.iter().copied() {
        let name_bytes = name.as_bytes();
        let name_key = super::global_name_key(name_bytes);
        let ns_value = if matches!(name, "console" | "process") {
            // #6230: dynamic method calls on the namespace *value* (`const p =
            // process; p.exit(1)`, `process["exit"](1)`, dynamic `console.log`)
            // need the module's runtime dispatch bucket. It is NOT installed
            // here: this function is statically reachable from core property
            // paths, so installing here linked both buckets (and `process`'s
            // stdio stream objects) into every binary. Codegen instead emits
            // `js_nm_install_process` / `js_nm_install_console` /
            // `js_install_global_value_surfaces` at every site that can yield
            // one of these values (see perry-codegen `global_value_installs`).
            js_create_native_module_namespace(name_bytes.as_ptr(), name_bytes.len())
        } else if name == "Math" {
            crate::value::js_nanbox_pointer(super::web_hooks::math_namespace() as i64)
        } else if name == "JSON" {
            crate::value::js_nanbox_pointer(super::web_hooks::json_namespace() as i64)
        } else if name == "Reflect" {
            crate::value::js_nanbox_pointer(super::web_hooks::reflect_namespace() as i64)
        } else if let Some(row) = global_binding_row(name) {
            armed_global_binding_value(row, singleton())
        } else {
            let ns_obj = crate::realm_decl! { birth js_object_alloc(0, 0); realm { Temporal, kind = namespace, members = { Duration; Instant; PlainDate; PlainTime; PlainDateTime; PlainYearMonth; PlainMonthDay; ZonedDateTime; Now; [Symbol.toStringTag] = "Temporal"; }, births = [Array, BigInt, Error, Number, RangeError, String, TypeError]; Intl, kind = namespace, members = { getCanonicalLocales; supportedValuesOf; Locale; NumberFormat; DateTimeFormat; Collator; Segmenter; ListFormat; RelativeTimeFormat; PluralRules; DurationFormat; DisplayNames; }, births = [Array, String]; Atomics, kind = namespace, members = { load; isLockFree; store; add; sub; and; or; xor; exchange; compareExchange; notify; wait; waitAsync; [Symbol.toStringTag] = "Atomics"; }, births = [ArrayBuffer, Int32Array, Promise]; } };
            if ns_obj.is_null() {
                continue;
            }
            // #4139 + #4149: reify each namespace's own members as real
            // properties so the reflection APIs (`getOwnPropertyDescriptor`,
            // `getOwnPropertyNames`) observe them. Call sites (`Math.max(...)`,
            // `JSON.stringify(...)`, `Reflect.get(...)`) are codegen intrinsics
            // gated on the AST shape and never read these fields. Complete
            // Math/JSON/Reflect namespaces are constructed above.
            match name {
                "Atomics" => {
                    super::web_hooks::atomics_members(ns_obj);
                    set_intrinsic_to_string_tag(ns_obj, "Atomics");
                }
                "Intl" => crate::intl::install_intl_namespace(ns_obj),
                // Members come from the `temporal` install (see
                // `crate::temporal::hooked`); without it the namespace stays a
                // plain empty object, as in a build without the feature.
                "Temporal" => {
                    if crate::temporal::hooked::install_namespace(ns_obj) {
                        set_intrinsic_to_string_tag(ns_obj, "Temporal");
                    }
                }
                _ => {}
            }
            crate::value::js_nanbox_pointer(ns_obj as i64)
        };
        super::super::define_builtin_data_property(
            singleton(),
            name_key,
            ns_value,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }
    // node: `performance` is an accessor (get + set, enumerable,
    // configurable). The getter reads perf_hooks' singleton on first read,
    // so nothing of perf_hooks runs at setup; the setter replaces the
    // property with a data property (node's replaceable attribute).
    {
        let getter = crate::closure::js_closure_alloc(
            crate::fn_info!(#[realm("performance.<get>", provider = closure_alloc)] export crate::object::global_this::ctor_thunks::global_this_performance_getter_thunk, 0; with_declared(0)),
            0,
        );
        let setter = crate::closure::js_closure_alloc(
            crate::fn_info!(#[realm("performance.<set>", provider = closure_alloc)] export crate::object::global_this::ctor_thunks::global_this_performance_setter_thunk, 1; with_declared(1)),
            0,
        );
        if !getter.is_null() && !setter.is_null() {
            super::super::set_builtin_accessor_descriptor(
                singleton() as usize,
                "performance".to_string(),
                super::super::AccessorDescriptor {
                    get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
                    set: crate::value::js_nanbox_pointer(setter as i64).to_bits(),
                },
                super::super::PropertyAttrs::new(false, true, true),
            );
        }
    }
    let getter = crate::closure::js_closure_alloc(
        crate::fn_info!(#[realm("crypto.<get>", provider = closure_alloc)] export crate::object::global_this::ctor_thunks::global_this_crypto_getter_thunk, 0; with_declared(0)),
        0,
    );
    let getter_bits = if getter.is_null() {
        0
    } else {
        crate::value::js_nanbox_pointer(getter as i64).to_bits()
    };
    super::super::set_builtin_accessor_descriptor(
        singleton() as usize,
        "crypto".to_string(),
        super::super::AccessorDescriptor {
            get: getter_bits,
            set: 0,
        },
        super::super::PropertyAttrs::new(true, true, true),
    );
    // #2923: `globalThis.navigator` — Node's browser-compatible runtime
    // metadata object. Node installs it as a lazy accessor (get only,
    // enumerable, configurable); so does Perry. The getter builds the object
    // on the first read and keeps it in its one capture, so `navigator.language`
    // reads LC_ALL / LANG on first use, not while the realm is populated.
    {
        let getter = crate::closure::js_closure_alloc(
            crate::fn_info!(#[realm("navigator.<get>", provider = closure_alloc, capture = [undefined])] export crate::navigator::global_this_navigator_getter_thunk, 0; with_declared(0)),
            1,
        );
        let getter_bits = if getter.is_null() {
            0
        } else {
            crate::closure::js_closure_set_capture_bits(getter, 0, crate::value::TAG_UNDEFINED);
            crate::value::js_nanbox_pointer(getter as i64).to_bits()
        };
        super::super::set_builtin_accessor_descriptor(
            singleton() as usize,
            "navigator".to_string(),
            super::super::AccessorDescriptor {
                get: getter_bits,
                set: 0,
            },
            super::super::PropertyAttrs::new(true, true, true),
        );
    }
    // ECMA-262 19.1/19.2/19.3: NaN, Infinity, and undefined are own data
    // properties of the global object with {writable:false, enumerable:false,
    // configurable:false}.  Install them so that
    // `Object.getOwnPropertyDescriptor(globalThis, "NaN")` returns a real
    // descriptor (test262 15.2.3.3-4-178/179/180) and
    // `Object.getOwnPropertyNames(globalThis)` includes them (15.2.3.4-4-1).
    {
        let non_writable = super::super::PropertyAttrs::new(false, false, false);
        for (name, value) in [("NaN", f64::NAN), ("Infinity", f64::INFINITY)] {
            let key = super::global_name_key(name);
            super::super::define_builtin_data_property(
                singleton(),
                key,
                value,
                name.to_string(),
                non_writable,
            );
        }
        let undef_key = super::global_name_key(b"undefined");
        let undef_val = f64::from_bits(crate::value::TAG_UNDEFINED);
        super::super::define_builtin_data_property(
            singleton(),
            undef_key,
            undef_val,
            "undefined".to_string(),
            super::super::PropertyAttrs::new(false, false, false),
        );
    }
    // #11915: every runtime-owned install and link above is done; from here a
    // mutation of an intrinsic prototype is the program's, and withdraws its
    // family's `Pristine` fact.
    crate::object::builtin_proto::seal_realm(singleton());
    if let Some(started) = bootstrap_started {
        eprintln!(
            "[gc-globalthis-bootstrap] elapsed_us={}",
            started.elapsed().as_micros()
        );
    }
}

/// (child, parent, the constructor also inherits) for the interface families.
/// `class C extends P` for a family `C` (its declaration's `extends`):
/// `C.prototype` inherits from `P.prototype` and, unless `C` is an interface
/// whose constructor does not inherit, `C` from `P`. Asking for `P`'s cells
/// births `P`.
fn link_extends_parent(
    realm: &crate::array::RealmIntrinsics,
    declaration: &BuiltinConstructorDeclaration,
    closure_ptr: *mut crate::closure::ClosureHeader,
    proto_obj: *mut ObjectHeader,
) {
    if let Some(parent) = declaration.constructor_extends {
        let parent_ctor = crate::array::intrinsic_constructor_addr_in(realm, parent);
        if parent_ctor != 0 {
            crate::closure::closure_set_static_prototype(
                closure_ptr as usize,
                crate::value::js_nanbox_pointer(parent_ctor as i64).to_bits(),
            );
        }
    }
    // A prototype with no explicit parent inherits its own realm's Object.
    // DEFAULT already represents the canonical Object parent; keep that shape
    // when it is the requested parent, rather than recording a redundant link.
    let parent_proto = if let Some(parent) = declaration.extends {
        crate::array::intrinsic_prototype_addr_in(realm, parent)
    } else {
        let bound = crate::array::intrinsic_prototype_addr_if_bound_in(
            realm, crate::object::BuiltinConstructorIdentity::Object,
        );
        if bound != 0 {
            bound
        } else {
            object_intrinsics_for_realm(realm).1 as usize
        }
    };
    let implicit_parent = if declaration.extends.is_none() {
        crate::array::object_prototype_addr_if_resolved()
    } else {
        0
    };
    if parent_proto != 0 && !proto_obj.is_null() && parent_proto != implicit_parent {
        super::super::prototype_chain::object_set_static_prototype(
            proto_obj as usize,
            crate::value::js_nanbox_pointer(parent_proto as i64).to_bits(),
        );
    }
}

#[inline(never)]
#[optimize(size)]
fn birth_family(
    realm: &crate::array::RealmIntrinsics,
    declaration: &BuiltinConstructorDeclaration,
    closure_ptr: *mut crate::closure::ClosureHeader,
) -> f64 {
    let name = declaration.name;
    // #2889: install static methods (`Object.keys`, `Array.isArray`, ...)
    // on the constructor closure so rebound usage like
    // `const O = Object; O.keys(x)` dispatches through the real helpers.
    // The prototype is an own static data property too. Include it in
    // the initial attributed layout so adding name/length cannot spill it.
    let proto_obj = if name == "Array" {
        // A runtime with no compiled entry (the Rust tests) registers the
        // static layouts at realm bootstrap, as `js_shape_run_static_seed`
        // does before any user code (§6a); `%Array.prototype%` is born on
        // one of them.
        #[cfg(test)]
        crate::object::layout_desc::register_static_layouts();
        let arr = crate::realm_decl! { birth crate::array::realm_array_alloc(0, crate::array::ArrayElements::Scalars, false); realm { Array.prototype, kind = prototype, extends = Object.prototype, members = { length = 0.0, attrs = writable; constructor; slice; toString; entries; flatMap; keys; toLocaleString; toReversed; toSorted; toSpliced; with; flat; values; pop; shift; reverse; push; unshift; splice; fill; copyWithin; sort; forEach; map; filter; some; every; find; findIndex; findLast; findLastIndex; reduce; reduceRight; indexOf; lastIndexOf; includes; at; join; concat; [Symbol.iterator] = ref(Array.prototype.values); }; } };
        // SAFETY: the fresh intrinsic, under this bootstrap's no-move scope.
        unsafe {
            crate::array::array_prototype_property_bag_ensure(
                arr,
                super::proto_room::BUILTIN_PROTOTYPE_ROOM,
            );
        }
        arr as *mut ObjectHeader
    } else if name == "Function" {
        // Born on its static layout (`runtime_layouts::FUNCTION_PROTOTYPE`,
        // a callable ordinary kind); the population fills its slots.
        // SAFETY: a registered runtime descriptor with the default prototype.
        unsafe {
            crate::realm_decl! { birth crate::object::layout_desc::alloc_layout_prototype(
                crate::object::runtime_layouts::FUNCTION_PROTOTYPE::desc(),
                0,
            ); realm { Function.prototype, members = { length = 0.0, attrs = configurable; name = "", attrs = configurable; constructor; apply; bind; call; toString; arguments = accessor(get = "Function.prototype.arguments.<get>", set = "Function.prototype.arguments.<get>"); caller = accessor(get = "Function.prototype.arguments.<get>", set = "Function.prototype.arguments.<get>"); [Symbol.hasInstance], attrs = writable | enumerable | configurable; }; } }
        }
    } else if name == "Iterator" {
        crate::object::iterator_prototypes::birth_iterator_prototype()
    } else {
        let proto = crate::realm_decl! { birth super::proto_room::alloc_builtin_prototype(); realm { AbortController.prototype, members = { constructor; signal = accessor(get = "AbortController.prototype.signal.<get>"), attrs = enumerable | configurable; abort, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "AbortController"; }; AbortSignal.prototype, extends = EventTarget.prototype, members = { constructor; aborted = accessor(get = "AbortSignal.prototype.aborted.<get>"), attrs = enumerable | configurable; reason = accessor(get = "AbortSignal.prototype.reason.<get>"); throwIfAborted; onabort = accessor(get = "AbortSignal.prototype.onabort.<get>", set = "AbortSignal.prototype.onabort.<set>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "AbortSignal"; }; AggregateError.prototype, extends = Error.prototype, members = { constructor; name = "AggregateError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; ArrayBuffer.prototype, members = { constructor; byteLength = accessor(get = "ArrayBuffer.prototype.byteLength.<get>"); slice; maxByteLength = accessor(get = "ArrayBuffer.prototype.maxByteLength.<get>"); resizable = accessor(get = "ArrayBuffer.prototype.resizable.<get>"); resize; transfer; transferToFixedLength; detached = accessor(get = "ArrayBuffer.prototype.detached.<get>"); [Symbol.toStringTag] = "ArrayBuffer"; }; AsyncDisposableStack.prototype, members = { constructor; disposed = accessor(get = "AsyncDisposableStack.prototype.disposed.<get>"); use; adopt; defer; disposeAsync; move; [Symbol.asyncDispose] = ref(AsyncDisposableStack.prototype.disposeAsync); [Symbol.toStringTag] = "AsyncDisposableStack"; }; BigInt.prototype, members = { constructor; toString; valueOf; toLocaleString; }; Blob.prototype, members = { constructor; arrayBuffer; bytes; slice; stream; text; [Symbol.toStringTag] = "Blob"; }; Boolean.prototype, members = { constructor; toString; valueOf; }; BroadcastChannel.prototype, extends = EventTarget.prototype, members = { constructor; name = undefined, attrs = writable | enumerable | configurable; close, attrs = writable | enumerable | configurable; postMessage, attrs = writable | enumerable | configurable; ref, attrs = writable | enumerable | configurable; unref, attrs = writable | enumerable | configurable; onmessage = accessor(get = "BroadcastChannel.prototype.onmessage.<get>", set = "BroadcastChannel.prototype.onmessage.<set>"), attrs = enumerable | configurable; onmessageerror = accessor(get = "BroadcastChannel.prototype.onmessageerror.<get>", set = "BroadcastChannel.prototype.onmessageerror.<set>"), attrs = enumerable | configurable; }; CompressionStream.prototype, members = { constructor; }; Crypto.prototype, members = { constructor; subtle = accessor(get = "Crypto.prototype.subtle.<get>"), attrs = enumerable | configurable; getRandomValues, attrs = writable | enumerable | configurable; randomUUID, attrs = writable | enumerable | configurable; }; CryptoKey.prototype, members = { constructor; algorithm = accessor(get = "CryptoKey.prototype.algorithm.<get>"), attrs = enumerable | configurable; extractable = accessor(get = "CryptoKey.prototype.extractable.<get>"), attrs = enumerable | configurable; type = accessor(get = "CryptoKey.prototype.type.<get>"), attrs = enumerable | configurable; usages = accessor(get = "CryptoKey.prototype.usages.<get>"), attrs = enumerable | configurable; }; MessageEvent.prototype, extends = Event.prototype, members = { constructor; data = accessor(get = "MessageEvent.prototype.data.<get>"), attrs = enumerable | configurable; origin = accessor(get = "MessageEvent.prototype.origin.<get>"), attrs = enumerable | configurable; lastEventId = accessor(get = "MessageEvent.prototype.lastEventId.<get>"), attrs = enumerable | configurable; source = accessor(get = "MessageEvent.prototype.source.<get>"), attrs = enumerable | configurable; ports = accessor(get = "MessageEvent.prototype.ports.<get>"), attrs = enumerable | configurable; initMessageEvent, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "MessageEvent"; }; CustomEvent.prototype, extends = Event.prototype, members = { constructor; detail = accessor(get = "CustomEvent.prototype.detail.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "CustomEvent"; }; DOMException.prototype, extends = Error.prototype, members = { constructor; name = accessor(get = "DOMException.prototype.name.<get>"), attrs = enumerable | configurable; message = accessor(get = "DOMException.prototype.message.<get>"), attrs = enumerable | configurable; code = accessor(get = "DOMException.prototype.code.<get>"), attrs = enumerable | configurable; INDEX_SIZE_ERR = 1.0, attrs = enumerable; DOMSTRING_SIZE_ERR = 2.0, attrs = enumerable; HIERARCHY_REQUEST_ERR = 3.0, attrs = enumerable; WRONG_DOCUMENT_ERR = 4.0, attrs = enumerable; INVALID_CHARACTER_ERR = 5.0, attrs = enumerable; NO_DATA_ALLOWED_ERR = 6.0, attrs = enumerable; NO_MODIFICATION_ALLOWED_ERR = 7.0, attrs = enumerable; NOT_FOUND_ERR = 8.0, attrs = enumerable; NOT_SUPPORTED_ERR = 9.0, attrs = enumerable; INUSE_ATTRIBUTE_ERR = 10.0, attrs = enumerable; INVALID_STATE_ERR = 11.0, attrs = enumerable; SYNTAX_ERR = 12.0, attrs = enumerable; INVALID_MODIFICATION_ERR = 13.0, attrs = enumerable; NAMESPACE_ERR = 14.0, attrs = enumerable; INVALID_ACCESS_ERR = 15.0, attrs = enumerable; VALIDATION_ERR = 16.0, attrs = enumerable; TYPE_MISMATCH_ERR = 17.0, attrs = enumerable; SECURITY_ERR = 18.0, attrs = enumerable; NETWORK_ERR = 19.0, attrs = enumerable; ABORT_ERR = 20.0, attrs = enumerable; URL_MISMATCH_ERR = 21.0, attrs = enumerable; QUOTA_EXCEEDED_ERR = 22.0, attrs = enumerable; TIMEOUT_ERR = 23.0, attrs = enumerable; INVALID_NODE_TYPE_ERR = 24.0, attrs = enumerable; DATA_CLONE_ERR = 25.0, attrs = enumerable; [Symbol.toStringTag] = "DOMException"; }; DataView.prototype, members = { constructor; buffer = accessor(get = "DataView.prototype.buffer.<get>"); byteLength = accessor(get = "DataView.prototype.byteLength.<get>"); byteOffset = accessor(get = "DataView.prototype.byteOffset.<get>"); getInt8; setInt8; getUint8; setUint8; getInt16; setInt16; getUint16; setUint16; getInt32; setInt32; getUint32; setUint32; getFloat32; setFloat32; getFloat64; setFloat64; getBigInt64; setBigInt64; getBigUint64; setBigUint64; getFloat16; setFloat16; [Symbol.toStringTag] = "DataView"; }; Date.prototype, members = { constructor; getDate; getDay; getFullYear; getHours; getMilliseconds; getMinutes; getMonth; getSeconds; getTime; getTimezoneOffset; getUTCDate; getUTCDay; getUTCFullYear; getUTCHours; getUTCMilliseconds; getUTCMinutes; getUTCMonth; getUTCSeconds; getYear; setDate; setFullYear; setHours; setMilliseconds; setMinutes; setMonth; setSeconds; setTime; setUTCDate; setUTCFullYear; setUTCHours; setUTCMilliseconds; setUTCMinutes; setUTCMonth; setUTCSeconds; setYear; toDateString; toISOString; toJSON; toLocaleDateString; toLocaleString; toLocaleTimeString; toTimeString; toUTCString = ref(Date.prototype.toGMTString); valueOf; toTemporalInstant; toGMTString; toString; [Symbol.toPrimitive], attrs = configurable; }; DecompressionStream.prototype, members = { constructor; }; DisposableStack.prototype, members = { constructor; disposed = accessor(get = "DisposableStack.prototype.disposed.<get>"); use; adopt; defer; dispose; move; [Symbol.dispose] = ref(DisposableStack.prototype.dispose); [Symbol.toStringTag] = "DisposableStack"; }; Error.prototype, members = { constructor; toString; name = "Error", attrs = writable | configurable; message = "", attrs = writable | configurable; }; EvalError.prototype, extends = Error.prototype, members = { constructor; name = "EvalError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; Event.prototype, members = { constructor; initEvent, attrs = writable | enumerable | configurable; stopImmediatePropagation, attrs = writable | enumerable | configurable; preventDefault, attrs = writable | enumerable | configurable; target = accessor(get = "Event.prototype.target.<get>"), attrs = enumerable | configurable; currentTarget = accessor(get = "Event.prototype.currentTarget.<get>"), attrs = enumerable | configurable; srcElement = accessor(get = "Event.prototype.srcElement.<get>"), attrs = enumerable | configurable; type = accessor(get = "Event.prototype.type.<get>"), attrs = enumerable | configurable; cancelable = accessor(get = "Event.prototype.cancelable.<get>"), attrs = enumerable | configurable; defaultPrevented = accessor(get = "Event.prototype.defaultPrevented.<get>"), attrs = enumerable | configurable; timeStamp = accessor(get = "Event.prototype.timeStamp.<get>"), attrs = enumerable | configurable; composedPath, attrs = writable | enumerable | configurable; returnValue = accessor(get = "Event.prototype.returnValue.<get>", set = "Event.prototype.returnValue.<set>"), attrs = enumerable | configurable; bubbles = accessor(get = "Event.prototype.bubbles.<get>"), attrs = enumerable | configurable; composed = accessor(get = "Event.prototype.composed.<get>"), attrs = enumerable | configurable; eventPhase = accessor(get = "Event.prototype.eventPhase.<get>"), attrs = enumerable | configurable; cancelBubble = accessor(get = "Event.prototype.cancelBubble.<get>", set = "Event.prototype.cancelBubble.<set>"), attrs = enumerable | configurable; stopPropagation, attrs = writable | enumerable | configurable; isTrusted = accessor(get = "Event.prototype.isTrusted.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "Event"; }; EventTarget.prototype, members = { constructor; addEventListener, attrs = writable | enumerable | configurable; removeEventListener, attrs = writable | enumerable | configurable; dispatchEvent, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "EventTarget"; }; File.prototype, members = { constructor; arrayBuffer; bytes; slice; stream; text; [Symbol.toStringTag] = "File"; }; FinalizationRegistry.prototype, members = { constructor; register; unregister; }; FormData.prototype, members = { constructor; append; delete; entries; forEach; get; getAll; has; keys; set; values; [Symbol.iterator] = ref(FormData.prototype.entries); [Symbol.toStringTag] = "FormData"; }; Headers.prototype, members = { constructor; entries; append; delete; forEach; get; getSetCookie; has; keys; set; values; [Symbol.iterator] = ref(Headers.prototype.entries); [Symbol.toStringTag] = "Headers"; }; Map.prototype, members = { constructor; size = accessor(get = "Map.prototype.size.<get>"); clear; delete; entries; forEach; get; has; keys; set; values; [Symbol.iterator] = ref(Map.prototype.entries); [Symbol.toStringTag] = "Map"; }; MessageChannel.prototype, members = { constructor; }; MessagePort.prototype, members = { constructor; postMessage, attrs = writable | enumerable | configurable; start, attrs = writable | enumerable | configurable; ref, attrs = writable | enumerable | configurable; unref, attrs = writable | enumerable | configurable; hasRef, attrs = writable | enumerable | configurable; onmessage = null, attrs = writable | enumerable | configurable; onmessageerror = null, attrs = writable | enumerable | configurable; close, attrs = writable | enumerable | configurable; }; Navigator.prototype, members = { constructor; hardwareConcurrency = accessor(get = "Navigator.prototype.hardwareConcurrency.<get>"), attrs = enumerable | configurable; locks = accessor(get = "Navigator.prototype.locks.<get>"), attrs = enumerable | configurable; language = accessor(get = "Navigator.prototype.language.<get>"), attrs = enumerable | configurable; languages = accessor(get = "Navigator.prototype.languages.<get>"), attrs = enumerable | configurable; userAgent = accessor(get = "Navigator.prototype.userAgent.<get>"), attrs = enumerable | configurable; platform = accessor(get = "Navigator.prototype.platform.<get>"), attrs = enumerable | configurable; }; Number.prototype, members = { constructor; toExponential; toFixed; toLocaleString; toPrecision; toString; valueOf; }; Promise.prototype, members = { constructor; catch; finally; then; [Symbol.toStringTag] = "Promise"; }; Proxy.prototype, members = { constructor; }; RangeError.prototype, extends = Error.prototype, members = { constructor; name = "RangeError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; ReadableByteStreamController.prototype, members = { constructor; byobRequest = accessor(get = "ReadableByteStreamController.prototype.byobRequest.<get>"), attrs = enumerable | configurable; desiredSize = accessor(get = "ReadableByteStreamController.prototype.desiredSize.<get>"), attrs = enumerable | configurable; close, attrs = writable | enumerable | configurable; enqueue, attrs = writable | enumerable | configurable; error, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "ReadableByteStreamController"; }; ReadableStream.prototype, members = { constructor; locked = accessor(get = "ReadableStream.prototype.locked.<get>"), attrs = enumerable | configurable; cancel, attrs = writable | enumerable | configurable; getReader, attrs = writable | enumerable | configurable; pipeThrough, attrs = writable | enumerable | configurable; pipeTo, attrs = writable | enumerable | configurable; tee, attrs = writable | enumerable | configurable; values, attrs = writable | enumerable | configurable; [Symbol.asyncIterator] = ref(ReadableStream.prototype.values); [Symbol.toStringTag] = "ReadableStream"; }; ReadableStreamBYOBReader.prototype, members = { constructor; closed = accessor(get = "ReadableStreamBYOBReader.prototype.closed.<get>"), attrs = enumerable | configurable; cancel, attrs = writable | enumerable | configurable; read, attrs = writable | enumerable | configurable; releaseLock, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "ReadableStreamBYOBReader"; }; ReadableStreamBYOBRequest.prototype, members = { constructor; view = accessor(get = "ReadableStreamBYOBRequest.prototype.view.<get>"), attrs = enumerable | configurable; respond, attrs = writable | enumerable | configurable; respondWithNewView, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "ReadableStreamBYOBRequest"; }; ReadableStreamDefaultController.prototype, members = { constructor; desiredSize = accessor(get = "ReadableStreamDefaultController.prototype.desiredSize.<get>"), attrs = enumerable | configurable; close, attrs = writable | enumerable | configurable; enqueue, attrs = writable | enumerable | configurable; error, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "ReadableStreamDefaultController"; }; ReadableStreamDefaultReader.prototype, members = { constructor; closed = accessor(get = "ReadableStreamDefaultReader.prototype.closed.<get>"), attrs = enumerable | configurable; cancel, attrs = writable | enumerable | configurable; read, attrs = writable | enumerable | configurable; releaseLock, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "ReadableStreamDefaultReader"; }; ReferenceError.prototype, extends = Error.prototype, members = { constructor; name = "ReferenceError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; RegExp.prototype, members = { constructor; flags = accessor(get = "RegExp.prototype.flags.<get>"); source = accessor(get = "RegExp.prototype.source.<get>"); global = accessor(get = "RegExp.prototype.global.<get>"); ignoreCase = accessor(get = "RegExp.prototype.ignoreCase.<get>"); multiline = accessor(get = "RegExp.prototype.multiline.<get>"); dotAll = accessor(get = "RegExp.prototype.dotAll.<get>"); sticky = accessor(get = "RegExp.prototype.sticky.<get>"); unicode = accessor(get = "RegExp.prototype.unicode.<get>"); unicodeSets = accessor(get = "RegExp.prototype.unicodeSets.<get>"); hasIndices = accessor(get = "RegExp.prototype.hasIndices.<get>"); exec; test; compile; toString; [Symbol.match]; [Symbol.search]; [Symbol.matchAll]; [Symbol.split]; [Symbol.replace]; }; Request.prototype, members = { constructor; arrayBuffer; blob; bytes; clone; formData; json; text; body = accessor(get = "Request.prototype.body.<get>"); bodyUsed = accessor(get = "Request.prototype.bodyUsed.<get>"); cache = accessor(get = "Request.prototype.cache.<get>"); credentials = accessor(get = "Request.prototype.credentials.<get>"); destination = accessor(get = "Request.prototype.destination.<get>"); duplex = accessor(get = "Request.prototype.duplex.<get>"); headers = accessor(get = "Request.prototype.headers.<get>"); integrity = accessor(get = "Request.prototype.integrity.<get>"); keepalive = accessor(get = "Request.prototype.keepalive.<get>"); method = accessor(get = "Request.prototype.method.<get>"); mode = accessor(get = "Request.prototype.mode.<get>"); redirect = accessor(get = "Request.prototype.redirect.<get>"); referrer = accessor(get = "Request.prototype.referrer.<get>"); referrerPolicy = accessor(get = "Request.prototype.referrerPolicy.<get>"); signal = accessor(get = "Request.prototype.signal.<get>"); url = accessor(get = "Request.prototype.url.<get>"); [Symbol.toStringTag] = "Request"; }; Response.prototype, members = { constructor; arrayBuffer; blob; bytes; clone; formData; json; text; body = accessor(get = "Response.prototype.body.<get>"); bodyUsed = accessor(get = "Response.prototype.bodyUsed.<get>"); headers = accessor(get = "Response.prototype.headers.<get>"); ok = accessor(get = "Response.prototype.ok.<get>"); redirected = accessor(get = "Response.prototype.redirected.<get>"); status = accessor(get = "Response.prototype.status.<get>"); statusText = accessor(get = "Response.prototype.statusText.<get>"); type = accessor(get = "Response.prototype.type.<get>"); url = accessor(get = "Response.prototype.url.<get>"); [Symbol.toStringTag] = "Response"; }; Set.prototype, members = { constructor; size = accessor(get = "Set.prototype.size.<get>"); add; clear; delete; entries; forEach; has; keys; values = ref(Set.prototype.keys); [Symbol.iterator] = ref(Set.prototype.keys); [Symbol.toStringTag] = "Set"; }; SharedArrayBuffer.prototype, members = { constructor; byteLength = accessor(get = "SharedArrayBuffer.prototype.byteLength.<get>"); slice; maxByteLength = accessor(get = "SharedArrayBuffer.prototype.maxByteLength.<get>"); growable = accessor(get = "SharedArrayBuffer.prototype.growable.<get>"); grow; [Symbol.toStringTag] = "SharedArrayBuffer"; }; Storage.prototype, members = { constructor; clear, attrs = writable | enumerable | configurable; getItem, attrs = writable | enumerable | configurable; key, attrs = writable | enumerable | configurable; removeItem, attrs = writable | enumerable | configurable; setItem, attrs = writable | enumerable | configurable; length = accessor(get = "Storage.prototype.length.<get>"), attrs = enumerable; }; String.prototype, members = { constructor; at; charAt; charCodeAt; codePointAt; anchor; big; blink; bold; fixed; fontcolor; fontsize; italics; link; small; strike; sub; sup; concat; endsWith; includes; indexOf; isWellFormed; lastIndexOf; localeCompare; match; matchAll; normalize; padEnd; padStart; repeat; replace; replaceAll; search; slice; split; startsWith; substr; substring; toLocaleLowerCase; toLocaleUpperCase; toLowerCase; toUpperCase; toWellFormed; trim; trimEnd; trimStart = ref(String.prototype.trimLeft); trimLeft; trimRight = ref(String.prototype.trimEnd); valueOf; toString; [Symbol.iterator]; }; SubtleCrypto.prototype, members = { constructor; encapsulateBits, attrs = writable | enumerable | configurable; decapsulateBits, attrs = writable | enumerable | configurable; encapsulateKey, attrs = writable | enumerable | configurable; decapsulateKey, attrs = writable | enumerable | configurable; }; SuppressedError.prototype, extends = Error.prototype, members = { constructor; name = "SuppressedError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; Symbol.prototype, members = { constructor; toString; valueOf; }; SyntaxError.prototype, extends = Error.prototype, members = { constructor; name = "SyntaxError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; TextDecoder.prototype, members = { constructor; decode, attrs = writable | enumerable | configurable; encoding = accessor(get = "TextDecoder.prototype.encoding.<get>"), attrs = enumerable | configurable; fatal = accessor(get = "TextDecoder.prototype.fatal.<get>"), attrs = enumerable | configurable; ignoreBOM = accessor(get = "TextDecoder.prototype.ignoreBOM.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "TextDecoder"; }; TextDecoderStream.prototype, members = { constructor; }; TextEncoder.prototype, members = { constructor; encode, attrs = writable | enumerable | configurable; encodeInto, attrs = writable | enumerable | configurable; encoding = accessor(get = "TextEncoder.prototype.encoding.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "TextEncoder"; }; TextEncoderStream.prototype, members = { constructor; }; TransformStream.prototype, members = { constructor; }; TypeError.prototype, extends = Error.prototype, members = { constructor; name = "TypeError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; URIError.prototype, extends = Error.prototype, members = { constructor; name = "URIError", attrs = writable | configurable; message = "", attrs = writable | configurable; }; URL.prototype, members = { constructor; toString, attrs = writable | enumerable | configurable; href = accessor(get = "URL.prototype.href.<get>", set = "URL.prototype.href.<set>"), attrs = enumerable | configurable; origin = accessor(get = "URL.prototype.origin.<get>"), attrs = enumerable | configurable; protocol = accessor(get = "URL.prototype.protocol.<get>", set = "URL.prototype.protocol.<set>"), attrs = enumerable | configurable; username = accessor(get = "URL.prototype.username.<get>", set = "URL.prototype.username.<set>"), attrs = enumerable | configurable; password = accessor(get = "URL.prototype.password.<get>", set = "URL.prototype.password.<set>"), attrs = enumerable | configurable; host = accessor(get = "URL.prototype.host.<get>", set = "URL.prototype.host.<set>"), attrs = enumerable | configurable; hostname = accessor(get = "URL.prototype.hostname.<get>", set = "URL.prototype.hostname.<set>"), attrs = enumerable | configurable; port = accessor(get = "URL.prototype.port.<get>", set = "URL.prototype.port.<set>"), attrs = enumerable | configurable; pathname = accessor(get = "URL.prototype.pathname.<get>", set = "URL.prototype.pathname.<set>"), attrs = enumerable | configurable; search = accessor(get = "URL.prototype.search.<get>", set = "URL.prototype.search.<set>"), attrs = enumerable | configurable; searchParams = accessor(get = "URL.prototype.searchParams.<get>"), attrs = enumerable | configurable; hash = accessor(get = "URL.prototype.hash.<get>", set = "URL.prototype.hash.<set>"), attrs = enumerable | configurable; toJSON, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "URL"; }; URLPattern.prototype, members = { constructor; exec; test; hasRegExpGroups = undefined, attrs = configurable; hash = undefined, attrs = configurable; hostname = undefined, attrs = configurable; password = undefined, attrs = configurable; pathname = undefined, attrs = configurable; port = undefined, attrs = configurable; protocol = undefined, attrs = configurable; search = undefined, attrs = configurable; username = undefined, attrs = configurable; }; URLSearchParams.prototype, members = { constructor; append; delete; entries; forEach; get; getAll; has; keys; set; sort; toString; values; size = accessor(get = "URLSearchParams.prototype.size.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "URLSearchParams"; }; WeakMap.prototype, members = { constructor; delete; get; has; set; [Symbol.toStringTag] = "WeakMap"; }; WeakRef.prototype, members = { constructor; deref; }; WeakSet.prototype, members = { constructor; add; delete; has; [Symbol.toStringTag] = "WeakSet"; }; WebSocket.prototype, members = { constructor; close, attrs = writable | enumerable | configurable; send, attrs = writable | enumerable | configurable; CONNECTING = 0.0, attrs = enumerable; OPEN = 1.0, attrs = enumerable; CLOSING = 2.0, attrs = enumerable; CLOSED = 3.0, attrs = enumerable; }; WritableStream.prototype, members = { constructor; }; BigInt64Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 8.0; }; BigUint64Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 8.0; }; Float16Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 2.0; }; Float32Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 4.0; }; Float64Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 8.0; }; Int16Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 2.0; }; Int32Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 4.0; }; Int8Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 1.0; }; Uint16Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 2.0; }; Uint32Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 4.0; }; Uint8Array.prototype, extends = "%TypedArray%.prototype", members = { constructor; toBase64; toHex; setFromBase64; setFromHex; BYTES_PER_ELEMENT = 1.0; }; Uint8ClampedArray.prototype, extends = "%TypedArray%.prototype", members = { constructor; BYTES_PER_ELEMENT = 1.0; }; Performance.prototype, extends = EventTarget.prototype, members = { constructor; clearMarks, attrs = writable | enumerable | configurable; clearMeasures, attrs = writable | enumerable | configurable; clearResourceTimings, attrs = writable | enumerable | configurable; getEntries, attrs = writable | enumerable | configurable; getEntriesByName, attrs = writable | enumerable | configurable; getEntriesByType, attrs = writable | enumerable | configurable; mark, attrs = writable | enumerable | configurable; measure, attrs = writable | enumerable | configurable; now, attrs = writable | enumerable | configurable; setResourceTimingBufferSize, attrs = writable | enumerable | configurable; toJSON, attrs = writable | enumerable | configurable; eventLoopUtilization = ref(performance.eventLoopUtilization); markResourceTiming; timerify = ref(performance.timerify); timeOrigin = accessor(get = "Performance.prototype.timeOrigin.<get>"), attrs = enumerable | configurable; [Symbol.toStringTag] = "Performance"; }; PerformanceEntry.prototype, members = { constructor; name = accessor(get = "PerformanceEntry.prototype.name.<get>"), attrs = enumerable | configurable; entryType = accessor(get = "PerformanceEntry.prototype.entryType.<get>"), attrs = enumerable | configurable; startTime = accessor(get = "PerformanceEntry.prototype.startTime.<get>"), attrs = enumerable | configurable; duration = accessor(get = "PerformanceEntry.prototype.duration.<get>"), attrs = enumerable | configurable; toJSON, attrs = writable | enumerable | configurable; }; PerformanceMark.prototype, extends = PerformanceEntry.prototype, members = { constructor; detail = accessor(get = "PerformanceMark.prototype.detail.<get>"), attrs = enumerable | configurable; toJSON; [Symbol.toStringTag] = "PerformanceMark"; }; PerformanceMeasure.prototype, extends = PerformanceEntry.prototype, members = { constructor; detail = accessor(get = "PerformanceMeasure.prototype.detail.<get>"), attrs = enumerable | configurable; toJSON; [Symbol.toStringTag] = "PerformanceMeasure"; }; PerformanceObserver.prototype, members = { constructor; observe, attrs = writable | enumerable | configurable; disconnect, attrs = writable | enumerable | configurable; takeRecords, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "PerformanceObserver"; }; PerformanceObserverEntryList.prototype, members = { constructor; getEntries, attrs = writable | enumerable | configurable; getEntriesByType, attrs = writable | enumerable | configurable; getEntriesByName, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "PerformanceObserverEntryList"; }; PerformanceResourceTiming.prototype, extends = PerformanceEntry.prototype, members = { constructor; initiatorType = accessor(get = "PerformanceResourceTiming.prototype.initiatorType.<get>"), attrs = enumerable | configurable; workerStart = accessor(get = "PerformanceResourceTiming.prototype.workerStart.<get>"), attrs = enumerable | configurable; redirectStart = accessor(get = "PerformanceResourceTiming.prototype.redirectStart.<get>"), attrs = enumerable | configurable; redirectEnd = accessor(get = "PerformanceResourceTiming.prototype.redirectEnd.<get>"), attrs = enumerable | configurable; fetchStart = accessor(get = "PerformanceResourceTiming.prototype.fetchStart.<get>"), attrs = enumerable | configurable; domainLookupStart = accessor(get = "PerformanceResourceTiming.prototype.domainLookupStart.<get>"), attrs = enumerable | configurable; domainLookupEnd = accessor(get = "PerformanceResourceTiming.prototype.domainLookupEnd.<get>"), attrs = enumerable | configurable; connectStart = accessor(get = "PerformanceResourceTiming.prototype.connectStart.<get>"), attrs = enumerable | configurable; connectEnd = accessor(get = "PerformanceResourceTiming.prototype.connectEnd.<get>"), attrs = enumerable | configurable; secureConnectionStart = accessor(get = "PerformanceResourceTiming.prototype.secureConnectionStart.<get>"), attrs = enumerable | configurable; nextHopProtocol = accessor(get = "PerformanceResourceTiming.prototype.nextHopProtocol.<get>"), attrs = enumerable | configurable; requestStart = accessor(get = "PerformanceResourceTiming.prototype.requestStart.<get>"), attrs = enumerable | configurable; responseStart = accessor(get = "PerformanceResourceTiming.prototype.responseStart.<get>"), attrs = enumerable | configurable; responseEnd = accessor(get = "PerformanceResourceTiming.prototype.responseEnd.<get>"), attrs = enumerable | configurable; encodedBodySize = accessor(get = "PerformanceResourceTiming.prototype.encodedBodySize.<get>"), attrs = enumerable | configurable; decodedBodySize = accessor(get = "PerformanceResourceTiming.prototype.decodedBodySize.<get>"), attrs = enumerable | configurable; transferSize = accessor(get = "PerformanceResourceTiming.prototype.transferSize.<get>"), attrs = enumerable | configurable; deliveryType = accessor(get = "PerformanceResourceTiming.prototype.deliveryType.<get>"), attrs = enumerable | configurable; responseStatus = accessor(get = "PerformanceResourceTiming.prototype.responseStatus.<get>"), attrs = enumerable | configurable; toJSON, attrs = writable | enumerable | configurable; [Symbol.toStringTag] = "PerformanceResourceTiming"; }; Buffer.prototype, extends = Uint8Array.prototype, members = { constructor; toString, attrs = writable | enumerable | configurable; inspect, attrs = writable | enumerable | configurable; slice, attrs = writable | enumerable | configurable; subarray, attrs = writable | enumerable | configurable; copy, attrs = writable | enumerable | configurable; write, attrs = writable | enumerable | configurable; toJSON, attrs = writable | enumerable | configurable; fill, attrs = writable | enumerable | configurable; equals, attrs = writable | enumerable | configurable; compare, attrs = writable | enumerable | configurable; indexOf, attrs = writable | enumerable | configurable; lastIndexOf, attrs = writable | enumerable | configurable; includes, attrs = writable | enumerable | configurable; swap16, attrs = writable | enumerable | configurable; swap32, attrs = writable | enumerable | configurable; swap64, attrs = writable | enumerable | configurable; toLocaleString, attrs = writable | enumerable | configurable; readUInt8, attrs = writable | enumerable | configurable; readUint8, attrs = writable | enumerable | configurable; readInt8, attrs = writable | enumerable | configurable; readUInt16BE, attrs = writable | enumerable | configurable; readUint16BE, attrs = writable | enumerable | configurable; readUInt16LE, attrs = writable | enumerable | configurable; readUint16LE, attrs = writable | enumerable | configurable; readInt16BE, attrs = writable | enumerable | configurable; readInt16LE, attrs = writable | enumerable | configurable; readUInt32BE, attrs = writable | enumerable | configurable; readUint32BE, attrs = writable | enumerable | configurable; readUInt32LE, attrs = writable | enumerable | configurable; readUint32LE, attrs = writable | enumerable | configurable; readInt32BE, attrs = writable | enumerable | configurable; readInt32LE, attrs = writable | enumerable | configurable; readFloatBE, attrs = writable | enumerable | configurable; readFloatLE, attrs = writable | enumerable | configurable; readDoubleBE, attrs = writable | enumerable | configurable; readDoubleLE, attrs = writable | enumerable | configurable; readBigInt64BE, attrs = writable | enumerable | configurable; readBigInt64LE, attrs = writable | enumerable | configurable; readBigUInt64BE, attrs = writable | enumerable | configurable; readBigUint64BE, attrs = writable | enumerable | configurable; readBigUInt64LE, attrs = writable | enumerable | configurable; readBigUint64LE, attrs = writable | enumerable | configurable; readUIntBE, attrs = writable | enumerable | configurable; readUintBE, attrs = writable | enumerable | configurable; readUIntLE, attrs = writable | enumerable | configurable; readUintLE, attrs = writable | enumerable | configurable; readIntBE, attrs = writable | enumerable | configurable; readIntLE, attrs = writable | enumerable | configurable; writeUInt8, attrs = writable | enumerable | configurable; writeUint8, attrs = writable | enumerable | configurable; writeInt8, attrs = writable | enumerable | configurable; writeUInt16BE, attrs = writable | enumerable | configurable; writeUint16BE, attrs = writable | enumerable | configurable; writeUInt16LE, attrs = writable | enumerable | configurable; writeUint16LE, attrs = writable | enumerable | configurable; writeInt16BE, attrs = writable | enumerable | configurable; writeInt16LE, attrs = writable | enumerable | configurable; writeUInt32BE, attrs = writable | enumerable | configurable; writeUint32BE, attrs = writable | enumerable | configurable; writeUInt32LE, attrs = writable | enumerable | configurable; writeUint32LE, attrs = writable | enumerable | configurable; writeInt32BE, attrs = writable | enumerable | configurable; writeInt32LE, attrs = writable | enumerable | configurable; writeFloatBE, attrs = writable | enumerable | configurable; writeFloatLE, attrs = writable | enumerable | configurable; writeDoubleBE, attrs = writable | enumerable | configurable; writeDoubleLE, attrs = writable | enumerable | configurable; writeBigInt64BE, attrs = writable | enumerable | configurable; writeBigInt64LE, attrs = writable | enumerable | configurable; writeBigUInt64BE, attrs = writable | enumerable | configurable; writeBigUint64BE, attrs = writable | enumerable | configurable; writeBigUInt64LE, attrs = writable | enumerable | configurable; writeBigUint64LE, attrs = writable | enumerable | configurable; writeUIntBE, attrs = writable | enumerable | configurable; writeUintBE, attrs = writable | enumerable | configurable; writeUIntLE, attrs = writable | enumerable | configurable; writeUintLE, attrs = writable | enumerable | configurable; writeIntBE, attrs = writable | enumerable | configurable; writeIntLE, attrs = writable | enumerable | configurable; asciiSlice, attrs = writable | enumerable | configurable; asciiWrite, attrs = writable | enumerable | configurable; base64Slice, attrs = writable | enumerable | configurable; base64Write, attrs = writable | enumerable | configurable; base64urlSlice, attrs = writable | enumerable | configurable; base64urlWrite, attrs = writable | enumerable | configurable; hexSlice, attrs = writable | enumerable | configurable; hexWrite, attrs = writable | enumerable | configurable; latin1Slice, attrs = writable | enumerable | configurable; latin1Write, attrs = writable | enumerable | configurable; ucs2Slice, attrs = writable | enumerable | configurable; ucs2Write, attrs = writable | enumerable | configurable; utf8Slice, attrs = writable | enumerable | configurable; utf8Write, attrs = writable | enumerable | configurable; parent = accessor(get = "Buffer.prototype.parent.<get>"), attrs = enumerable; offset = accessor(get = "Buffer.prototype.offset.<get>"), attrs = enumerable; }; "%TypedArray%.prototype", members = { constructor; buffer = accessor(get = "%TypedArray%.prototype.buffer.<get>"); byteLength = accessor(get = "%TypedArray%.prototype.byteLength.<get>"); byteOffset = accessor(get = "%TypedArray%.prototype.byteOffset.<get>"); length = accessor(get = "%TypedArray%.prototype.length.<get>"); entries; keys; values; at; copyWithin; every; fill; filter; find; findIndex; findLast; findLastIndex; forEach; includes; indexOf; join; lastIndexOf; map; reverse; reduce; reduceRight; set; slice; some; sort; subarray; toReversed; toSorted; with; toLocaleString; toString = ref(Array.prototype.toString); [Symbol.toStringTag] = accessor(get = "%TypedArray%.prototype[\"@@toStringTag\"].<get>"); [Symbol.iterator] = ref("%TypedArray%.prototype.values"); }, births = ["%ArrayIteratorPrototype%", ArrayBuffer]; } };
        // The realm seals this family prototype before any program code
        // can reach it (`seal_realm`): it is born on the born-prototype
        // lineage, so that seal's mark mints no shape.
        if crate::object::builtin_proto::family_prototype_is_sealed(declaration) {
            // SAFETY: the keyless newborn, unexposed, in this no-move window.
            unsafe { crate::object::shapes::stamp_born_prototype_root(proto) };
        }
        proto
    };
    // The family cell is the one record of this prototype from its
    // allocation on: every read during the birth (a member install walking
    // Array.prototype's chain) gets the object being born, and
    // `birth_lazy_family` returns at once instead of re-entering.
    crate::array::note_realm_intrinsic_in(
        realm,
        declaration.identity,
        proto_obj as usize,
        closure_ptr as usize,
    );
    if declaration.prototype_callable {
        // The declaration is immutable birth metadata, independent of the
        // public name/property. Publish the callable shape and header fact.
        unsafe {
            crate::object::shapes::publish_callable_object_kind(
                proto_obj,
                crate::object::shapes::ShapeObjectKind::CallableOrdinary,
            );
        }
    }
    install_builtin_constructor_statics(
        name,
        closure_ptr,
        (!proto_obj.is_null()).then(|| crate::value::js_nanbox_pointer(proto_obj as i64)),
    );
    // The constructor is described from its bag now that its family is
    // born (an unborn one is FunctionDictionary, `unborn_constructor`).
    unsafe { crate::closure::shape::closure_describe_born_constructor(closure_ptr as usize) };
    if declaration.species {
        install_builtin_species_accessor(closure_ptr);
    }
    let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
    // #4533: `Object.getPrototypeOf(TypeError) === Error`. The constructor's
    // `[[Prototype]]` is `Error` itself (not `Function.prototype`).
    // A thread singleton: only the thread realm's birth writes it.
    if name == "Error" && crate::array::is_thread_realm(realm) {
        ERROR_CONSTRUCTOR_PTR.with(|cell| cell.set(closure_ptr as usize));
    }
    if name == "Iterator" {
        crate::object::iterator_ctor::bind_constructor(proto_obj, ctor_value);
        link_extends_parent(realm, declaration, closure_ptr, proto_obj);
        finish_builtin_prototype(realm, declaration, proto_obj);
        crate::object::iterator_ctor::note_birth_keys(proto_obj);
        unsafe {
            crate::object::builtin_proto::seal_object_family(
                proto_obj, crate::object::builtin_proto::ITERATOR_FAMILY,
            );
        }
    } else if !proto_obj.is_null() {
        let ctor_key = crate::string::js_string_from_bytes(
            b"constructor".as_ptr(),
            "constructor".len() as u32,
        );
        super::super::define_builtin_data_property(
            proto_obj,
            ctor_key,
            ctor_value,
            "constructor".to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
        if is_web_fetch_constructor(name) {
            super::super::define_builtin_data_property(
                proto_obj,
                ctor_key,
                ctor_value,
                "constructor".to_string(),
                super::super::PropertyAttrs::new(true, false, true),
            );
        }
        if name == "Array" {
            let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
            super::super::define_builtin_data_property(
                proto_obj,
                constructor_key,
                ctor_value,
                "constructor".to_string(),
                super::super::PropertyAttrs::new(true, false, true),
            );
        }
        if matches!(
            name,
            "Navigator"
                | "ReadableStreamDefaultController"
                | "ReadableStreamDefaultReader"
                | "ReadableByteStreamController"
                | "ReadableStreamBYOBReader"
                | "ReadableStreamBYOBRequest"
                | "TextEncoderStream"
                | "TextDecoderStream"
                | "CompressionStream"
                | "DecompressionStream"
        ) {
            let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
            js_object_set_field_by_name(proto_obj, constructor_key, ctor_value);
        }
        // Populate well-known method properties on the prototype
        // (currently just `Array.prototype.slice`). Methods are
        // ClosureHeader-backed thunks that take their receiver as
        // `this` and dispatch to the corresponding native entry
        // point — works in tandem with `.call`/`.apply` since those
        // arms (#970) pass the explicit receiver when forwarding.
        if declaration.external {
            crate::external_family::birth(declaration.identity, proto_obj);
        }
        populate_builtin_prototype_methods(realm, name, proto_obj);
        if name == "Buffer" {
            super::super::native_module::birth_buffer_own_parts(closure_ptr, proto_obj);
        }
        crate::event_target_hook_slots::prototype_install_constructor_constants(
            name,
            closure_ptr.cast(),
        );
        // A perf_hooks family's members are perf_hooks' own (a no-op for
        // every other family and when perf_hooks is not linked).
        unsafe {
            crate::perf_hook_slots::attach_perf_hooks_constructor(
                name,
                ctor_value,
                closure_ptr as usize,
                proto_obj,
            );
        }
        if let Some(serial) = declaration.prototype_serial.filter(|_| crate::array::is_thread_realm(realm)) {
            // SAFETY: `proto_obj` is the live, just-populated prototype.
            unsafe {
                crate::object::proto_validity::assign_intrinsic_prototype_serial(
                    proto_obj as usize,
                    serial,
                );
                name_first_realm_prototype(proto_obj, serial);
            }
        }
        if let Some(serial) = declaration.named_serial.filter(|_| crate::array::is_thread_realm(realm)) {
            // SAFETY: `proto_obj` is the live intrinsic array; its holder
            // of named properties is the object the serial names.
            unsafe {
                let holder =
                    crate::array::array_property_bag(proto_obj as *const crate::array::ArrayHeader);
                if !holder.is_null() {
                    name_first_realm_prototype(holder, serial);
                }
            }
        }
        if let Some(serial) = declaration.sealed_serial.filter(|_| crate::array::is_thread_realm(realm)) {
            // SAFETY: as above; runtime layouts name it by this serial.
            unsafe { crate::object::builtin_proto::seal_runtime_prototype(proto_obj, serial) };
        }
        if name == "Function" && crate::array::is_thread_realm(realm) {
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR
                .store(proto_obj as i64, std::sync::atomic::Ordering::Release);
        }
        install_error_prototype_data_properties(name, proto_obj);
        // ECMA-262 20.5.6.3: the [[Prototype]] of each NativeError prototype
        // object is %Error.prototype% (not %Object.prototype%). `Error` is
        // listed before its subclasses, so its prototype object is stashed
        // here and linked into each subclass prototype's chain. Without this
        // `Object.getPrototypeOf(TypeError.prototype) !== Error.prototype`
        // (test262 NativeErrors/*/prototype/proto.js).
        if super::super::native_module::is_worker_messaging_constructor(name) {
            crate::messaging_hook_slots::populate_messaging_prototype(name, proto_obj, ctor_value);
            if name == "BroadcastChannel" {
                crate::event_target_hook_slots::install_web_event_proto_methods(name, proto_obj);
            }
        }
        link_extends_parent(realm, declaration, closure_ptr, proto_obj);
        if name == "Storage" {
            crate::web_storage::install_storage_prototype(proto_obj, ctor_value);
        }
        if matches!(name, "Crypto" | "CryptoKey" | "SubtleCrypto") {
            super::super::native_module::install_webcrypto_constructor_proto(proto_obj, ctor_value);
        }
        if name == "WebSocket" {
            websocket_global::install_constructor_shape(closure_ptr, proto_obj);
        }
        // #2145: a `%TypedArray%` child's prototype carries the typed-array
        // prototype flag (`ValidateTypedArray`'s receiver test); its
        // [[Prototype]] edges are its declared `extends` (link_extends_parent).
        if declaration.extends == Some(crate::object::BuiltinConstructorIdentity::TypedArray) {
            unsafe {
                let gc = (proto_obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE)
                    as *mut crate::gc::GcHeader;
                (*gc)._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
                // Charter step 3: a store-kind input.
                crate::object::shapes::store_kind::restamp_object_store_kind(
                    proto_obj as *mut crate::object::ObjectHeader,
                );
            }
        }
        // #4140: per-kind `BYTES_PER_ELEMENT` own data property on BOTH the
        // constructor and its prototype, matching Node's descriptor
        // `{ value, writable:false, enumerable:false, configurable:false }`.
        // On the prototype it precedes `Uint8Array`'s base64/hex methods,
        // node's own-key order.
        // The bare `Uint8Array.BYTES_PER_ELEMENT` read folds at compile time
        // (#2902), but the reflective forms — `getOwnPropertyDescriptor`,
        // `hasOwnProperty`, and the chained `Float64Array.prototype
        // .BYTES_PER_ELEMENT` — resolve off these installed own properties.
        let ta_bytes_per_element = match name {
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" => Some(1.0),
            "Int16Array" | "Uint16Array" | "Float16Array" => Some(2.0),
            "Int32Array" | "Uint32Array" | "Float32Array" => Some(4.0),
            "Float64Array" | "BigInt64Array" | "BigUint64Array" => Some(8.0),
            _ => None,
        };
        if let Some(bytes) = ta_bytes_per_element {
            let bpe_attrs = super::super::PropertyAttrs::new(false, false, false);
            for target in [closure_ptr as *mut ObjectHeader, proto_obj] {
                let bpe_key = crate::string::js_string_from_bytes(
                    b"BYTES_PER_ELEMENT".as_ptr(),
                    b"BYTES_PER_ELEMENT".len() as u32,
                );
                super::super::define_builtin_data_property(
                    target,
                    bpe_key,
                    bytes,
                    "BYTES_PER_ELEMENT".to_string(),
                    bpe_attrs,
                );
            }
        }
        // #6674: the TC39 base64/hex conversion methods
        // (`toBase64`/`toHex`/`setFromBase64`/`setFromHex`) are Uint8Array-ONLY
        // (they don't exist on other typed arrays), so install them as own
        // methods on the `Uint8Array` per-kind prototype — not the shared
        // `%TypedArray%.prototype` intrinsic. This materializes the value read
        // `typeof Uint8Array.prototype.toBase64 === "function"`; the static
        // factories `fromBase64`/`fromHex` are installed on the constructor by
        // `install_builtin_constructor_statics`.
        if name == "Uint8Array" {
            super::super::typed_array_proto_thunks::install_uint8array_base64_hex_proto_methods(
                proto_obj,
            );
        }
        if name != "Array" {
            // Its own keys are in: return the room it did not use.
            // SAFETY: fresh, unexposed, under this bootstrap's no-move scope.
            unsafe { super::proto_room::fit_builtin_prototype(proto_obj) };
        }
        finish_builtin_prototype(realm, declaration, proto_obj);
    }
    // The realm's [[Intrinsics]] entry for this family: the constructor
    // and what its instances inherit from, whatever `globalThis.<name>`
    // later holds.
    let ctor = JSValue::from_bits(ctor_value.to_bits());
    crate::array::note_realm_intrinsic_in(
        realm,
        declaration.identity,
        proto_obj as usize,
        if ctor.is_pointer() {
            ctor.as_pointer::<u8>() as usize
        } else {
            0
        },
    );
    ctor_value
}

crate::perry_thread_local! {
    /// The lazy family this thread is birthing: (its realm's record address,
    /// its identity), (0, 0) when none. The birth's own bag install must not
    /// ask for the birth again.
    static BIRTHING: std::cell::Cell<(usize, u8)> = const { std::cell::Cell::new((0, 0)) };
}

/// Birth the `lazy` family `identity` (`BuiltinConstructorDeclaration::lazy`)
/// in this thread's realm: everything realm setup did not do, by the same
/// `birth_family` as an eager family, so the members are in declaration
/// order whatever observation came first. Entered only from
/// `array::realm_intrinsic_addr`, when the family's intrinsic prototype is
/// still 0 after the realm exists. No-op while the realm has not bound the
/// constructor yet (realm setup itself).
#[cold]
#[inline(never)]
/// The still unborn constructor `closure_ptr` of `identity` in `realm`:
/// noted in [[Intrinsics]] (prototype half 0). Its declared statics are own
/// keys its base Function shape does not list: FunctionDictionary, so no read
/// site proves one absent (the read reaches the bag and births the family).
fn unborn_constructor(
    realm: &crate::array::RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
    closure_ptr: *mut crate::closure::ClosureHeader,
) -> f64 {
    crate::array::note_realm_intrinsic_in(realm, identity, 0, closure_ptr as usize);
    unsafe { crate::closure::shape::closure_become_dictionary(closure_ptr.cast()) };
    crate::value::js_nanbox_pointer(closure_ptr as i64)
}

/// A family whose birth is `birth_lazy_family` (its first observation):
/// `lazy`, and not one of the two made by their own builders (`Object`,
/// `Buffer`). Its prototype and its constructor namespace are sealed by that
/// birth (`seal_born_family`), never by `seal_realm`.
pub(crate) fn born_by_its_observation(declaration: &BuiltinConstructorDeclaration) -> bool {
    declaration.lazy
}

pub(crate) fn birth_lazy_family(identity: crate::object::BuiltinConstructorIdentity) {
    birth_lazy_family_in(crate::array::thread_realm(), identity);
}

/// [`birth_lazy_family`] in `realm`.
pub(crate) fn birth_lazy_family_in(
    realm: &crate::array::RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
) {
    debug_assert!(identity.declaration().lazy);
    birth_own_family(realm, identity);
}

/// `%Function%`'s setup birth. Function is a root family like Object: every
/// closure's [[Prototype]] is %Function.prototype%, so the first closure the
/// realm setup allocates observes it; "born at setup" is its true state, and
/// it is not declared `lazy`. This is the one writer of its cells.
pub(crate) fn ensure_function_intrinsics_in(realm: &crate::array::RealmIntrinsics) {
    birth_own_family(realm, crate::object::BuiltinConstructorIdentity::Function);
}

/// A family's birth: the constructor (made here when it is born before
/// %Global%), then `birth_family` on it.
#[optimize(size)]
fn birth_own_family(
    realm: &crate::array::RealmIntrinsics,
    identity: crate::object::BuiltinConstructorIdentity,
) {
    let declaration = identity.declaration();
    if crate::array::intrinsic_prototype_addr_if_bound_in(realm, identity) != 0 {
        return;
    }
    let mut ctor = crate::array::intrinsic_constructor_addr_if_bound_in(realm, identity);
    if ctor == 0 {
        // A constructor observed before %Global% uses the same allocation
        // as realm setup. Object's pair is made by its own builder.
        let _no_move = crate::gc::GcSuppressScope::new();
        let closure_ptr = super::super::global_this_tables::alloc_builtin_constructor(identity, realm);
        if closure_ptr.is_null() {
            return;
        }
        unborn_constructor(realm, identity, closure_ptr);
        ctor = closure_ptr as usize;
    }
    // A member install that asks for its own family's prototype is a
    // programming error: it would read the unfinished birth.
    let birthing = (realm as *const crate::array::RealmIntrinsics as usize, identity as u8);
    debug_assert_ne!(
        BIRTHING.with(|c| c.get()),
        birthing,
        "{} asked for its own prototype during its birth",
        declaration.name
    );
    let outer = BIRTHING.with(|c| c.replace(birthing));
    // The same work whichever init asks first: the runtime's (SNAP §2.10.20).
    let _realm = crate::snap_census::realm_build();
    let _no_move = crate::gc::GcSuppressScope::new();
    super::super::own_override::as_builtin_definition(|| {
        birth_family(
            realm,
            declaration,
            ctor as *mut crate::closure::ClosureHeader,
        );
        unsafe { crate::object::builtin_proto::seal_born_family(realm, declaration.name, identity) };
    });
    BIRTHING.with(|c| c.set(outer));
}

/// Birth the family of the closure at `ptr` when `ptr` is its realm's still
/// unborn `lazy` constructor (which has no own property bag until then);
/// true when it did. Every path that needs that constructor's bag or its
/// `prototype` value asks this first; presence and key listing need neither.
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[inline(always)]
pub(crate) unsafe fn birth_unborn_family_of(ptr: usize) -> bool {
    let word = (*(ptr as *const crate::closure::ClosureHeader)).capture_count;
    word & crate::codegen_abi::CLOSURE_CONSTRUCT_MASK != 0 && birth_unborn_family_slow(ptr, word)
}

#[cold]
#[inline(never)]
fn birth_unborn_family_slow(ptr: usize, word: u32) -> bool {
    unborn_lazy_identity(ptr, word)
        .is_some_and(|(realm, identity)| crate::array::intrinsic_prototype_addr_in(realm, identity) != 0)
}

/// The declared static keys of the closure at `ptr` while it is its realm's
/// still unborn `lazy` constructor (`BuiltinConstructorDeclaration::statics`),
/// else none. Presence and key listing answer them without a birth; reading
/// one's value births the family ([`birth_unborn_family_of`]).
///
/// # Safety
/// `ptr` is a proven, live closure cell.
#[cold]
#[inline(never)]
pub(crate) unsafe fn unborn_declared_statics_of(ptr: usize) -> &'static [&'static str] {
    let word = (*(ptr as *const crate::closure::ClosureHeader)).capture_count;
    if word & crate::codegen_abi::CLOSURE_CONSTRUCT_MASK == 0 {
        return &[];
    }
    unborn_lazy_identity(ptr, word).map_or(&[], |(_, identity)| identity.declaration().statics)
}

/// The identity of the closure at `ptr` (header word `word`) while it is its
/// realm's still unborn `lazy` constructor, outside that family's own birth.
fn unborn_lazy_identity(ptr: usize, word: u32) -> Option<(&'static crate::array::RealmIntrinsics, crate::object::BuiltinConstructorIdentity)> {
    let identity = crate::object::BuiltinConstructorIdentity::from_header(word)?;
    let realm = unsafe { super::super::global_this_tables::builtin_constructor_realm(ptr as *const _) };
    // The record cells first: a born family stops the test there, so the
    // BIRTHING pair is read only for a still unborn constructor.
    (identity.declaration().lazy
        && crate::array::intrinsic_prototype_addr_if_bound_in(realm, identity) == 0
        && crate::array::intrinsic_constructor_addr_if_bound_in(realm, identity) == ptr
        && BIRTHING.with(|c| c.get())
            != (realm as *const crate::array::RealmIntrinsics as usize, identity as u8))
        .then_some((realm, identity))
}

/// `%TypedArray%.prototype.toString` is `Array.prototype.toString` (ECMA-262
/// §23.2.3.33): `%TypedArray%`'s birth installs it, reading Array's own
/// family cell (which births Array), never %Global%.
pub(super) fn alias_typed_array_proto_to_string(realm: &crate::array::RealmIntrinsics, ta_proto_addr: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ta_proto_handle = scope.root_raw_mut_ptr(ta_proto_addr);
    let arr_proto_ptr = crate::array::intrinsic_prototype_addr_in(
        realm,
        crate::object::BuiltinConstructorIdentity::Array,
    ) as *mut ObjectHeader;
    if arr_proto_ptr.is_null() {
        return;
    }
    let arr_proto_handle = scope.root_raw_mut_ptr(arr_proto_ptr);

    let ts_key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    let to_string_fn =
        js_object_get_field_by_name(arr_proto_handle.get_raw_mut_ptr::<ObjectHeader>(), ts_key);
    if to_string_fn.bits() == crate::value::TAG_UNDEFINED {
        return;
    }
    let to_string_handle = scope.root_nanbox_u64(to_string_fn.bits());

    let ts_key2 = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    super::super::define_builtin_data_property(
        ta_proto_handle.get_raw_mut_ptr::<ObjectHeader>(),
        ts_key2,
        f64::from_bits(to_string_handle.get_nanbox_u64()),
        "toString".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}


/// The info and spec length of the function intrinsic `which`.
fn realm_parse_function_info(
    which: crate::array::RealmFunctionIntrinsic,
) -> (*const crate::closure::JsFunctionInfo, u32) {
    match which {
        crate::array::RealmFunctionIntrinsic::ParseInt => (
            crate::fn_info!(#[realm(parseInt)] export crate::object::global_this::builtin_thunks::global_this_parse_int_thunk, 2; with_declared(2)),
            2,
        ),
        crate::array::RealmFunctionIntrinsic::ParseFloat => (
            crate::fn_info!(#[realm(parseFloat)] export crate::object::global_this::builtin_thunks::global_this_parse_float_thunk, 1; with_declared(1)),
            1,
        ),
    }
}

/// A built-in global helper function (`parseInt`, `isNaN`, ...): its closure,
/// name, length, and non-constructor mark.
fn global_helper_closure(
    info: *const crate::closure::JsFunctionInfo,
    name: &str,
    arity: u32,
) -> *mut crate::closure::ClosureHeader {
    let closure_ptr = crate::closure::js_closure_alloc(info, 0);
    if closure_ptr.is_null() {
        return closure_ptr;
    }
    unsafe {
        crate::builtins::register_function_name(&*info, name.as_bytes());
    }
    super::super::native_module::set_builtin_closure_length(closure_ptr as usize, arity);
        // Every global helper installed here (parseInt/parseFloat/isNaN/
        // isFinite/{en,de}codeURI{,Component}/escape/unescape/setTimeout/…) is a
        // built-in *non-constructor* function: per spec it has no `.prototype`
        // (reads back `undefined`) and `new fn()` throws a TypeError. Mark the
        // closure so the `new`/`.prototype` paths honor that — otherwise these
        // functions defaulted to an ordinary `.prototype` object and silently
        // accepted `new` (test262 built-ins/{decodeURI,isNaN,parseFloat,…}
        // */A5.6/A5.7/A2.6/A2.7/A7.6/A7.7).
    super::super::native_module::set_builtin_closure_non_constructable(closure_ptr as usize);
    closure_ptr
}

/// `%parseFloat%` / `%parseInt%` of `realm`: its cell when born,
/// else its birth (first observation: the global binding or `Number`'s
/// statics). One closure is both `parseFloat` and `Number.parseFloat`.
pub(crate) fn realm_parse_function(realm: &crate::array::RealmIntrinsics, which: crate::array::RealmFunctionIntrinsic) -> f64 {
    let bound = crate::array::realm_function_intrinsic_if_bound_in(realm, which);
    if bound != 0 {
        return crate::value::js_nanbox_pointer(bound as i64);
    }
    let (info, arity) = realm_parse_function_info(which);
    let name = match which {
        crate::array::RealmFunctionIntrinsic::ParseInt => "parseInt",
        crate::array::RealmFunctionIntrinsic::ParseFloat => "parseFloat",
    };
    let closure_ptr = global_helper_closure(info, name, arity);
    if closure_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::array::note_realm_function_intrinsic_in(realm, which, closure_ptr as usize);
    crate::value::js_nanbox_pointer(closure_ptr as i64)
}
crate::perry_thread_local! {
    /// Raw address of THIS thread's `Error` constructor closure, captured at
    /// install. Read by `error::error_prepare_stack_trace_override` so
    /// `captureStackTrace` / `error.stack` can honor a user-set
    /// `Error.prepareStackTrace`. Thread-local, not a process-global: each
    /// `perry/thread` agent has its own arena + realm, and an `Error`
    /// constructor / `prepareStackTrace` from another thread's arena can be a
    /// foreign or freed pointer — the same reason `globalThis` is per-thread.
    ///
    /// **This is a GC root, and must stay one (#7231).** The address is a RAW
    /// `*mut ClosureHeader` from `js_closure_alloc` — a nursery allocation.
    /// The canonical `Error` closure is also a field of `globalThis`, so the
    /// structural trace keeps it alive and rewrites THAT reference; this
    /// duplicate lives outside the object graph, so an evacuating collection
    /// leaves it naming from-space and `error_prepare_stack_trace_override`
    /// then reads `prepareStackTrace` off an abandoned closure. Rooted by
    /// `scan_error_constructor_root_mut`.
    pub(crate) static ERROR_CONSTRUCTOR_PTR: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// Root + rewrite the raw `Error` constructor address.
pub(crate) fn scan_error_constructor_root_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ERROR_CONSTRUCTOR_PTR.with(|cell| {
        let mut addr = cell.get();
        if addr != 0 && visitor.visit_usize_slot(&mut addr) {
            cell.set(addr);
        }
    });
}

/// The default `Error.prepareStackTrace` thunk's address — used to tell a
/// user override apart from Perry's built-in default.
pub(crate) fn default_prepare_stack_trace_func_ptr() -> usize {
    global_this_error_prepare_stack_trace_thunk as *const u8 as usize
}

// =====================================================================
// #2889: static methods on rebound global built-in constructor values.
//
// `const O = Object; O.keys(x)` reads `keys` off the `Object` constructor
// closure's dynamic-prop side table, then calls it. Pre-fix nothing was
// installed there, so the read returned `undefined`. These thunks delegate
// to the same runtime helpers the direct `Object.keys(x)` lowering uses.
// =====================================================================

/// Give the agent's first realm's builtin prototype `proto` its reserved
/// intrinsic `serial` and write the serial's identity word, which names it
/// for the agent's life (`builtin_proto::name_runtime_prototype`, the
/// encoding of runtime prototype families): the unknown-receiver probe reaches
/// `%Function.prototype%` and `%Array.prototype%`'s holder through it, as the
/// slow reads reach the first realm's intrinsics (`array::prototype_addr`).
///
/// # Safety
/// `proto` is a live ordinary object whose serial is 0 or `serial`.
unsafe fn name_first_realm_prototype(proto: *mut ObjectHeader, serial: u64) {
    if crate::object::shapes::identity_word_slot(serial).is_some_and(|slot| *slot != 0) {
        return;
    }
    crate::object::proto_validity::assign_intrinsic_prototype_serial(proto as usize, serial);
    let _ = crate::object::builtin_proto::name_runtime_prototype(proto, serial);
}

// Global bindings born only in a program that can observe them (RB6N26).
//
// Each row of GLOBAL_BINDING_ROWS is one global function or namespace whose value only a site that
// names it (or holds %Global% as a value) can read. The row holds its birth cell: 0 until codegen's
// `js_install_global_<name>` (emitted once per module, in `__perry_prepare_literals_<module>`, for
// every binding the module's HIR can observe: perry-codegen global_bindings.rs) arms it. %Global%'s
// population births armed rows into their layout slots in declaration order; an unarmed row keeps
// its key with value undefined, which no code of that program can read. Arming after %Global% is
// born births the row into its slot at once (a value store to an existing key). Only the installer
// references the birth, so a program that cannot observe the binding links none of its code.

pub(crate) type GlobalBirth = fn(*mut ObjectHeader) -> f64;

pub(crate) struct GlobalBindingRow {
    pub(crate) name: &'static str,
    pub(crate) enumerable: bool,
    /// A namespace is defined at its place in GLOBAL_THIS_BUILTIN_NAMESPACES; a function in the
    /// global-functions block.
    pub(crate) namespace: bool,
    /// The armed `GlobalBirth` (as usize); 0 = the program cannot observe the binding.
    pub(crate) birth: std::sync::atomic::AtomicUsize,
}

macro_rules! global_bindings {
    (@one $x:literal) => { 1usize };
    ($( $name:literal, $install:ident, namespace = $ns:literal, enumerable = $en:literal, $birth:path; )*) => {
        pub(crate) static GLOBAL_BINDING_ROWS: [GlobalBindingRow; 0 $(+ global_bindings!(@one $name))*] = [
            $( GlobalBindingRow { name: $name, enumerable: $en, namespace: $ns, birth: std::sync::atomic::AtomicUsize::new(0) }, )*
        ];
        $(
            #[no_mangle]
            #[allow(non_snake_case)]
            pub extern "C" fn $install() {
                arm_global_binding($name, $birth);
            }
        )*
        /// A site that can observe ANY global (dyn-eval / vm entry) arms every row.
        pub(crate) fn arm_all_global_bindings() {
            $( $install(); )*
        }
    };
}

// The rows, derived by build.rs from the %Global% declaration's `observed(X)` members
// (perry-runtime-manifest realm_global_bindings.rs), in declaration order.
include!(concat!(env!("OUT_DIR"), "/global_bindings.rs"));

pub(crate) fn global_binding_row(name: &str) -> Option<&'static GlobalBindingRow> {
    GLOBAL_BINDING_ROWS.iter().find(|row| row.name == name)
}

/// The row's value for a %Global% being populated: its birth if armed, else undefined.
fn armed_global_binding_value(row: &GlobalBindingRow, global: *mut ObjectHeader) -> f64 {
    match row.birth.load(std::sync::atomic::Ordering::Relaxed) {
        0 => f64::from_bits(crate::value::TAG_UNDEFINED),
        f => unsafe { std::mem::transmute::<usize, GlobalBirth>(f)(global) },
    }
}

fn define_global_binding(global: *mut ObjectHeader, row: &GlobalBindingRow, value: f64) {
    super::super::define_builtin_data_property(
        global,
        super::global_name_key(row.name.as_bytes()),
        value,
        row.name.to_string(),
        super::super::PropertyAttrs::new(true, row.enumerable, true),
    );
}

fn arm_global_binding(name: &'static str, birth: GlobalBirth) {
    let Some(row) = global_binding_row(name) else {
        return;
    };
    if row.birth.swap(birth as usize, std::sync::atomic::Ordering::Relaxed) != 0 {
        return;
    }
    if let Some(global) = super::fetch_globals::born_thread_global() {
        let _no_move = crate::gc::GcSuppressScope::new();
        super::super::own_override::as_builtin_definition(|| define_global_binding(global, row, birth(global)));
    }
}

fn global_helper_value(info: *const crate::closure::JsFunctionInfo, name: &'static str, arity: u32) -> f64 {
    let closure_ptr = global_helper_closure(info, name, arity);
    if closure_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::value::js_nanbox_pointer(closure_ptr as i64)
}

/// The global binding and `Number` statics read the same function cell in
/// their realm, whichever one observes the intrinsic first.
fn birth_global_parse(global: *mut ObjectHeader, which: crate::array::RealmFunctionIntrinsic) -> f64 {
    let realm = unsafe { super::realm_of(global) };
    realm_parse_function(realm, which)
}

fn birth_global_parse_int(global: *mut ObjectHeader) -> f64 {
    birth_global_parse(global, crate::array::RealmFunctionIntrinsic::ParseInt)
}

fn birth_global_parse_float(global: *mut ObjectHeader) -> f64 {
    birth_global_parse(global, crate::array::RealmFunctionIntrinsic::ParseFloat)
}

fn birth_global_web_assembly(_global: *mut ObjectHeader) -> f64 {
    super::global_this_webassembly::create_webassembly_namespace()
}

fn birth_global_eval(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(eval)] export crate::object::global_this::fetch_globals::global_this_eval_thunk, 1; with_declared(1)),
        "eval",
        1,
    )
}

fn birth_global_fetch(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(fetch, effects = { coerce(arg(0), ToPrimitive); reads(field(arg(1), method), field(arg(1), headers), field(arg(1), body), field(arg(1), referrer), field(arg(1), referrerPolicy), field(arg(1), mode), field(arg(1), credentials), field(arg(1), cache), field(arg(1), redirect), field(arg(1), integrity), field(arg(1), keepalive), field(arg(1), signal), field(arg(1), duplex), field(arg(1), priority), field(arg(1), window), field(arg(1), dispatcher)); coerce(field(arg(1), method), ToPrimitive); coerce(field(arg(1), referrer), ToPrimitive); coerce(field(arg(1), referrerPolicy), ToPrimitive); coerce(field(arg(1), mode), ToPrimitive); coerce(field(arg(1), credentials), ToPrimitive); coerce(field(arg(1), cache), ToPrimitive); coerce(field(arg(1), redirect), ToPrimitive); coerce(field(arg(1), integrity), ToPrimitive); coerce(field(arg(1), duplex), ToPrimitive); coerce(field(arg(1), priority), ToPrimitive); coerce(field(arg(1), headers), Iterate); coerce(elems(field(arg(1), headers)), Iterate); coerce(field(arg(1), headers), OwnKeys); coerce(elems(field(arg(1), headers)), ToPrimitive); escapes(field(arg(1), body)); stores(fresh); returns(promise(fresh(Response.prototype, slots = {bodyError: islot(field(arg(1), signal), reason) | islot(islot(arg(0), signal), reason)}), reject = fresh(TypeError.prototype) | fresh(DOMException.prototype) | islot(field(arg(1), signal), reason) | islot(islot(arg(0), signal), reason))) })] export crate::object::global_fetch::global_this_fetch_thunk, 2; with_rest(1)),
        "fetch",
        1,
    )
}

fn birth_global_structured_clone(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(structuredClone, births = [ArrayBuffer, DOMException, Uint8Array])] export crate::object::global_this::builtin_thunks::global_this_structured_clone_thunk, 2; with_declared(2)),
        "structuredClone",
        2,
    )
}

fn birth_global_atob(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(atob, births = [DOMException])] export crate::object::global_this::builtin_thunks::global_this_atob_thunk, 1; with_declared(1)),
        "atob",
        1,
    )
}

fn birth_global_btoa(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(btoa, births = [DOMException])] export crate::object::global_this::builtin_thunks::global_this_btoa_thunk, 1; with_declared(1)),
        "btoa",
        1,
    )
}

fn birth_global_set_timeout(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(setTimeout)] export crate::object::global_this::array_error::global_this_set_timeout_thunk, 3; with_rest(2)),
        "setTimeout",
        2,
    )
}

fn birth_global_clear_timeout(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(clearTimeout)] export crate::object::global_this::array_error::global_this_clear_timeout_thunk, 1; with_declared(1)),
        "clearTimeout",
        1,
    )
}

fn birth_global_set_interval(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(setInterval)] export crate::object::global_this::array_error::global_this_set_interval_thunk, 3; with_rest(2)),
        "setInterval",
        2,
    )
}

fn birth_global_clear_interval(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(clearInterval)] export crate::object::global_this::array_error::global_this_clear_interval_thunk, 1; with_declared(1)),
        "clearInterval",
        1,
    )
}

fn birth_global_set_immediate(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(setImmediate)] export crate::object::global_this::array_error::global_this_set_immediate_thunk, 2; with_rest(1)),
        "setImmediate",
        1,
    )
}

fn birth_global_clear_immediate(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(clearImmediate)] export crate::object::global_this::array_error::global_this_clear_immediate_thunk, 1; with_declared(1)),
        "clearImmediate",
        1,
    )
}

fn birth_global_queue_microtask(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(queueMicrotask)] export crate::object::global_this::array_error::global_this_queue_microtask_thunk, 1; with_declared(1)),
        "queueMicrotask",
        1,
    )
}

fn birth_global_gc(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(gc)] export crate::object::global_this::builtin_thunks::global_this_gc_thunk, 1; with_declared(1)),
        "gc",
        1,
    )
}

fn birth_global_is_na_n(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(isNaN)] export crate::object::global_this::builtin_thunks::global_this_is_nan_thunk, 1; with_declared(1)),
        "isNaN",
        1,
    )
}

fn birth_global_is_finite(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(isFinite)] export crate::object::global_this::builtin_thunks::global_this_is_finite_thunk, 1; with_declared(1)),
        "isFinite",
        1,
    )
}

fn birth_global_encode_u_r_i(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(encodeURI)] export crate::object::global_this::builtin_thunks::global_this_encode_uri_thunk, 1; with_declared(1)),
        "encodeURI",
        1,
    )
}

fn birth_global_decode_u_r_i(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(decodeURI)] export crate::object::global_this::builtin_thunks::global_this_decode_uri_thunk, 1; with_declared(1)),
        "decodeURI",
        1,
    )
}

fn birth_global_encode_u_r_i_component(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(encodeURIComponent)] export crate::object::global_this::builtin_thunks::global_this_encode_uri_component_thunk, 1; with_declared(1)),
        "encodeURIComponent",
        1,
    )
}

fn birth_global_decode_u_r_i_component(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(decodeURIComponent)] export crate::object::global_this::builtin_thunks::global_this_decode_uri_component_thunk, 1; with_declared(1)),
        "decodeURIComponent",
        1,
    )
}

fn birth_global_escape(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(escape)] export crate::object::global_this::builtin_thunks::global_this_escape_thunk, 1; with_declared(1)),
        "escape",
        1,
    )
}

fn birth_global_unescape(_global: *mut ObjectHeader) -> f64 {
    global_helper_value(
        crate::fn_info!(#[realm(unescape)] export crate::object::global_this::builtin_thunks::global_this_unescape_thunk, 1; with_declared(1)),
        "unescape",
        1,
    )
}
