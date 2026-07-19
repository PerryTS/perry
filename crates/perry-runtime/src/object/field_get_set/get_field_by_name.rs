//! `js_object_get_field_by_name` inline-cache hot path: leading receiver
//! guards. The object-deref tail lives in `get_field_by_name_tail.rs`.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

/// Wall 10 — read a property a framework attached to a native registry handle
/// via `Object.setPrototypeOf(handle, proto)` (Express's `res`/`req`). The link
/// is keyed by the handle id in `OBJECT_PROTOTYPES`. Returns `None` (so the
/// caller yields `undefined`) when no recorded prototype carries the key.
/// Cheap when no prototype was recorded (the common handle case).
fn handle_proto_inherited_field(
    handle_id: usize,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    crate::object::prototype_chain::object_static_prototype(handle_id)?;
    let v = crate::object::prototype_chain::resolve_inherited_field(handle_id, key)?;
    if v.bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(v)
    }
}

#[no_mangle]
pub extern "C" fn js_object_get_field_by_name(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    // #5972: a null key reaches here when the property-key expression didn't
    // yield a usable string handle — e.g. `js_get_string_pointer_unified`
    // returned 0 for a NaN/number key that fell through its coercion branches.
    // Several arms below deref `(*key).byte_len` without a null check, so a
    // null key would SIGSEGV at offset 4 (KERN_INVALID_ADDRESS at 0x4). Per JS
    // semantics such a lookup simply misses → undefined. Same defensive shape
    // as the #2128 invalid-key guard further down. Every in-runtime caller
    // passes an interned non-null key, so this only affects the codegen
    // computed-access path.
    if key.is_null() {
        return JSValue::undefined();
    }
    // #2846: the receiver may be a Proxy value that arrived through a generic
    // property read (e.g. `rec.proxy.a` where `rec = Proxy.revocable(...)`).
    // Proxies are encoded as small fake pointers; deref-ing one as an
    // ObjectHeader would read unmapped memory. Route to the proxy get dispatch,
    // which forwards to the target (or throws on a revoked proxy) — matching
    // Node. `js_proxy_is_proxy` validates the value is a *registered* proxy so a
    // real heap object whose address happens to be small isn't misrouted.
    {
        // Proxy ids live in the proxy id band; `js_proxy_is_proxy` confirms
        // it is a *registered* proxy before we route to the proxy getter.
        //
        // #6699: the receiver arrives in two encodings. A generic property read
        // passes the raw small proxy-id pointer (`top16 == 0`). The class-field
        // IC-miss fallback (`js_object_get_field_by_name_f64`, reached when the
        // typed-`this` field guard rejects an off-shape receiver) instead
        // forwards the *full NaN-box* value with the `0x7FFD` heap-pointer tag
        // still set — exactly the tagged encoding the FAST LANE below already
        // strips. A tagged proxy value (`0x7FFD_0000_000F_xxxx`) is not itself
        // in the proxy id band, so the un-normalized band test missed it and the
        // read fell through to `undefined` instead of the get trap. This bit a
        // typed-`this` field read whose `this` is a Proxy — e.g. pi's TUI theme
        // is a `new Proxy({}, …)` whose `get` trap forwards to the real Theme;
        // `theme.fg()` runs with `this === proxy`, and `this.fgColors` must hit
        // the trap. Normalize the tag first so both encodings route identically.
        let addr = obj as u64;
        let raw_addr = if (addr >> 48) == 0x7FFD {
            addr & 0x0000_FFFF_FFFF_FFFF
        } else {
            addr
        };
        if crate::value::addr_class::is_proxy_id_band(raw_addr as usize) && !key.is_null() {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (raw_addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                let v = crate::proxy::js_proxy_get(boxed, key_f64);
                return JSValue::from_bits(v.to_bits());
            }
        }
    }
    // FAST LANE (store-plan-cache follow-up): resolve an OWN data field on a
    // provably-plain arena class instance with no rooting scope, no
    // exotic-registry probes, and no key hashing. Every gate proves a property
    // the skipped slow-path checks would have tested:
    //  - band/tag checks: not a proxy (above) / handle / stream encoding;
    //  - `classify_heap_generation != Unknown`: the address is inside a
    //    registered arena page, so its GcHeader is real — and no malloc-backed
    //    exotic (BufferHeader / TypedArrayHeader / DateCell / RegExpHeader /
    //    Temporal cell, all mi- or gc-malloc'd) can classify as arena;
    //  - `GC_TYPE_OBJECT`: not a closure / array / error / Map / Set;
    //  - `class_id != 0` (and not the native-module id): not an arguments
    //    object (allocated with class 0), URL-shape object, builtin prototype
    //    host, or plain literal — those keep their existing paths;
    //  - `OBJ_FLAG_HAS_DESCRIPTORS` clear: no own accessor can shadow the
    //    slot (an own data property shadows inherited accessors per [[Get]]);
    //    `OBJ_FLAG_TYPED_ARRAY_PROTO` clear: not the per-kind TypedArray
    //    prototype host (its reflection accessors have empty backing fields).
    // The (keys_array, interned key) → index mapping comes from the
    // epoch-guarded read-plan cache (flushed on GC, descriptor / prototype /
    // vtable mutations, and property deletes); a lane-local bounded scan
    // populates it. An absent own key falls through — prototype and getter
    // resolution stay on the existing path.
    unsafe {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let raw = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        if raw >= crate::gc::GC_HEADER_SIZE + 0x1000
            && !crate::value::addr_class::is_small_handle(raw)
            && !crate::value::addr_class::is_stream_id_band(raw)
            && crate::value::addr_class::is_above_handle_band(key as usize)
        {
            let key_gc =
                (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*key_gc).gc_flags & crate::gc::GC_FLAG_INTERNED != 0
                && crate::arena::classify_heap_generation(raw)
                    != crate::arena::HeapGeneration::Unknown
            {
                let gc_hdr =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                const LANE_BLOCKING: u16 =
                    crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
                if (*gc_hdr).obj_type == crate::gc::GC_TYPE_OBJECT
                    && (*gc_hdr)._reserved & LANE_BLOCKING == 0
                {
                    let o = raw as *const ObjectHeader;
                    let class_id = (*o).class_id;
                    if class_id != 0
                        && class_id != super::super::native_module::NATIVE_MODULE_CLASS_ID
                    {
                        let keys = (*o).keys_array;
                        if !keys.is_null()
                            && ((keys as u64) >> 48) == 0
                            && crate::value::addr_class::is_above_handle_band(keys as usize)
                        {
                            let alloc_limit = std::cmp::max((*o).field_count, 8) as usize;
                            if let Some(idx) = super::super::prop_plan::read_plan_lookup(
                                keys as usize,
                                key as usize,
                            ) {
                                return if (idx as usize) < alloc_limit {
                                    super::accessors::js_object_get_field(o, idx)
                                } else {
                                    match super::super::overflow_get(raw, idx as usize) {
                                        Some(b) => JSValue::from_bits(b),
                                        None => JSValue::undefined(),
                                    }
                                };
                            }
                            let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                                as *const crate::gc::GcHeader;
                            if (*keys_gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                                let key_count =
                                    crate::array::keys_array_len_capped_to_capacity(keys);
                                if key_count <= 4096 {
                                    for i in 0..key_count {
                                        let kv = crate::array::js_array_get(keys, i as u32);
                                        if crate::string::js_string_key_matches(kv, key) {
                                            super::super::prop_plan::read_plan_record(
                                                keys as usize,
                                                key as usize,
                                                i as u32,
                                            );
                                            return if i < alloc_limit {
                                                super::accessors::js_object_get_field(o, i as u32)
                                            } else {
                                                match super::super::overflow_get(raw, i) {
                                                    Some(b) => JSValue::from_bits(b),
                                                    None => JSValue::undefined(),
                                                }
                                            };
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // A receiver that LOOKS like a bare heap pointer (top 16 bits clear) but does
    // not land in the platform heap range is a MIS-decoded primitive, not an
    // object. The common case is a `number` whose raw f64 bits alias a sub-heap
    // address: a dynamic `arr[i]` read (`js_dyn_index_get`) returns the element's
    // JSValue bits, codegen forwards them straight to the object field-read ABI
    // on the type-erased path, and a denormal such as `0x0000_0090_8000_0201`
    // (~620 GB) arrives here as `obj`. It clears the `>> 48 == 0` check and sits
    // ABOVE the 1 MB handle band, so the `is_above_handle_band`-only guards on the
    // special-case reads below (and the `own_key_present` / `js_object_get_class_id`
    // ObjectHeader derefs they call) passed it straight through — and the read
    // dereferenced it as a GcHeader → KERN_INVALID_ADDRESS (real macOS allocations
    // sit at ~3–5 TB, never 620 GB). Pair the band check with `is_valid_obj_ptr`
    // (the canonical heap-range predicate) and treat a non-heap receiver as a
    // property miss: reading any data property off a primitive is `undefined`, and
    // the primitive-prototype methods are resolved on the by-name f64 wrapper's own
    // path, which never reaches this pointer dereference.
    if !key.is_null()
        && ((obj as u64) >> 48) == 0
        && crate::value::addr_class::is_above_handle_band(obj as usize)
        && !crate::value::addr_class::is_valid_obj_ptr(obj as *const u8)
    {
        return JSValue::undefined();
    }
    // `class X extends Map | Set` instance — `.size` reads the hidden backing
    // collection's size. A subclass CAN still define an own `size` (class field
    // or `Object.defineProperty`), so check own-property precedence first and
    // only fall back to the backing size when no own key exists. Other backed
    // reads (`.has`/`.get`/… as METHODS) route through `js_native_call_method`.
    // Guarded by `is_above_handle_band` (like the class-object probe below) so a
    // native handle id in [0x10000, 0x100000) never reaches `own_key_present`'s
    // ObjectHeader deref; real subclass instances are ordinary heap objects
    // above the band.
    if !key.is_null()
        && ((obj as u64) >> 48) == 0
        && crate::value::addr_class::is_above_handle_band(obj as usize)
    {
        unsafe {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            if std::slice::from_raw_parts(name_ptr, name_len) == b"size"
                && !super::super::own_key_present(obj as *mut ObjectHeader, key)
            {
                // A subclass may also OVERRIDE `size` on its prototype
                // (`class M extends Map { get size() { return 42 } }`). Such an
                // inherited getter lives in the class vtable, not as an own key,
                // so check the class chain first and fall through to the normal
                // class/prototype resolution when it shadows the backing size.
                let class_id = super::super::js_object_get_class_id(obj);
                let has_inherited_size = class_id != 0
                    && super::super::native_module::class_instance_has_member(class_id, "size");
                if !has_inherited_size {
                    let boxed = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
                    match crate::object::map_set_subclass::subclass_backing_of(boxed) {
                        Some(crate::object::map_set_subclass::CollectionBacking::Map(m)) => {
                            return JSValue::number(crate::map::js_map_size(m) as f64);
                        }
                        Some(crate::object::map_set_subclass::CollectionBacking::Set(s)) => {
                            return JSValue::number(crate::set::js_set_size(s) as f64);
                        }
                        None => {}
                    }
                }
            }
        }
    }
    // WeakMap / WeakSet instance — a VALUE read of the collection methods
    // (`w.add`, `wm.set`, `typeof w.has`; react-server-dom's chunk-preload
    // dedup does `u.add.bind(u, a)` — #5989) must resolve the brand-checking
    // prototype thunk. Method CALLS dispatch via js_native_call_method's weak
    // arms, but this by-name read path had no equivalent, so the read yielded
    // `undefined` and the subsequent `.bind` threw. Own keys keep precedence
    // (fresh instances only carry the `__perry_wk_entries` sentinel).
    if !key.is_null()
        && ((obj as u64) >> 48) == 0
        && crate::value::addr_class::is_above_handle_band(obj as usize)
    {
        unsafe {
            let boxed = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
            if let Some(cid) = crate::weakref::weak_class_id_from_receiver(boxed) {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::slice::from_raw_parts(name_ptr, name_len);
                let (builtin, known) = if cid == crate::weakref::CLASS_ID_WEAKMAP {
                    (
                        "WeakMap",
                        matches!(name, b"set" | b"get" | b"has" | b"delete"),
                    )
                } else {
                    ("WeakSet", matches!(name, b"add" | b"has" | b"delete"))
                };
                if known && !super::super::own_key_present(obj as *mut ObjectHeader, key) {
                    if let Ok(method_name) = std::str::from_utf8(name) {
                        if let Some(v) =
                            super::super::collection_proto_thunks::collection_proto_method_value(
                                builtin,
                                method_name,
                            )
                        {
                            return JSValue::from_bits(v.to_bits());
                        }
                    }
                }
            }
        }
    }
    // `class X extends Promise` instance — a value read of `then`/`catch`/
    // `finally` (`p.then` / `typeof p.finally`, and codegen's `p.finally(cb)`
    // which reads the property first) must resolve the reified Promise prototype
    // method. The generic prototype walk does not surface these builtin
    // `Promise.prototype` methods for a subclass instance, so hook them here when
    // no own key shadows them. The method thunks unwrap the backing cell from the
    // implicit-this receiver (see `promise_prototype_receiver`).
    if !key.is_null()
        && ((obj as u64) >> 48) == 0
        && crate::value::addr_class::is_above_handle_band(obj as usize)
    {
        unsafe {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name =
                std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)).unwrap_or("");
            if matches!(name, "then" | "catch" | "finally")
                && !super::super::own_key_present(obj as *mut ObjectHeader, key)
            {
                let boxed = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
                if crate::promise::subclass_backing_promise(boxed).is_some() {
                    if let Some(m) = crate::promise::promise_proto_method(name) {
                        return JSValue::from_bits(m.to_bits());
                    }
                }
            }
        }
    }
    // A per-evaluation class object (`ClassExprFresh`, #1772/#1787) reaches
    // here as a RAW heap pointer (a real ObjectHeader, so its top 16 address
    // bits are 0 — distinguishing it from a `0x7FFE` class-ref value or any
    // NaN-boxed value). Its static METHODS / static ACCESSORS live in the class
    // registry keyed by the header class_id, never as own properties, so a read
    // like `C.staticMethod` returned `undefined` (the class-ref form resolves
    // these via the registry; this pointer-tagged class-object form did not).
    // That is NestJS's `Logger.error` when the Logger takes the fresh path
    // (captures `DEFAULT_LOGGER`), which the tslib `__decorate` chain then reads
    // `.value` off → "reading 'value'". Resolve own fields first (own-property
    // precedence), then fall back to the registry. The `(obj >> 48) == 0` guard
    // ensures `is_class_object_ptr` only ever sees a real heap pointer (it
    // back-reads a GcHeader), never a tagged value — which previously SIGSEGV'd.
    if !key.is_null()
        && ((obj as u64) >> 48) == 0
        // Must be ABOVE the whole small-handle band (>= 0x100000), not just
        // >= 0x10000: native handle ids in [0x10000, 0x100000) (fetch/http/…)
        // would otherwise reach `is_class_object_ptr`, which back-reads a
        // GcHeader and SIGSEGVs on the non-heap handle id.
        && crate::value::addr_class::is_above_handle_band(obj as usize)
        && crate::object::class_registry::is_class_object_ptr(obj as *const u8)
    {
        // #6438: precedence for a per-evaluation class object is
        //   own  ->  THIS object's pinned parent  ->  generic tail.
        //
        // The generic tail folds the own lookup together with a class_id-keyed
        // prototype-chain walk (`resolve_proto_chain_field_with_receiver`). That
        // chain goes through the TEMPLATE's parent edge, which is last-wins
        // across evaluations, so for a factory called twice it answers with the
        // SIBLING's inherited value and never reports undefined — which would
        // silently pre-empt the pinned walk below. effect:
        //
        //   make(ast)                -> class SchemaClass { static ast = ast }
        //   makeTypeLiteralClass(..) -> class TypeLiteralClass extends make(ast) {…}
        //
        // `Struct(a)` then `Struct(b)` left `structA.ast === astB`, because
        // `structA.ast` resolved through TypeLiteralClass's TEMPLATE parent edge
        // (last registered = b's SchemaClass) instead of structA's own parent.
        // Check the object's OWN fields first, then ITS pinned parent, and only
        // then fall through to the tail.
        unsafe {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let want = std::slice::from_raw_parts(name_ptr, name_len);
                if let Some(v) =
                    crate::object::class_registry::class_object_own_field_bytes(obj, want)
                {
                    return JSValue::from_bits(v.to_bits());
                }
                // #6530: `name` and `prototype` are OWN properties of every
                // constructor — never inherited through the parent edge. Skip
                // the pinned-parent recursion for them so the generic tail
                // below synthesizes both from THIS object's class_id (the
                // recursion otherwise answered with the BASE class's `.name`
                // for every capture-carrying subclass — bundled zod's
                // identity collapse to "ZodType").
                if want != b"name" && want != b"prototype" {
                    if let Some(parent) =
                        crate::object::class_registry::class_object_pinned_parent(obj)
                    {
                        let pbits = parent.to_bits();
                        if (pbits >> 48) == 0x7FFD {
                            let praw = (pbits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
                            if praw as usize != obj as usize
                                && crate::value::addr_class::is_above_handle_band(praw as usize)
                                && crate::object::is_valid_obj_ptr(praw as *const u8)
                            {
                                let v = js_object_get_field_by_name(praw, key);
                                if !v.is_undefined() {
                                    return v;
                                }
                            }
                        }
                    }
                }
            }
        }
        let own = get_field_by_name_object_tail(obj, key);
        if !own.is_undefined() {
            return own;
        }
        unsafe {
            // Re-box the raw class-object pointer as a POINTER-tagged JS value
            // so `js_class_method_bind` (which expects a value, like the
            // class-ref path) binds the static method to the right receiver.
            let class_value = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
            let class_id = super::super::js_object_get_class_id(obj);
            if class_id != 0 {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("");
                if !name.is_empty()
                    && !super::super::class_registry::class_is_key_deleted(class_id, name)
                {
                    if super::super::class_registry::lookup_static_method_in_chain(class_id, name)
                        .is_some()
                    {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(name_len.max(1), 1).unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(name_ptr, ptr, name_len);
                            ptr
                        };
                        let result = js_class_method_bind(class_value, heap_name, name_len);
                        return JSValue::from_bits(result.to_bits());
                    }
                    if let Some(v) =
                        super::super::class_registry::class_static_accessor_getter_value(
                            class_id,
                            name,
                            class_value,
                        )
                    {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
            }
        }
        return own;
    }
    if let Some(addr) =
        crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
    {
        if !key.is_null() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let ta = addr as *const crate::typedarray::TypedArrayHeader;
                if let Some(value) = crypto_key_property_value(addr, key_bytes) {
                    return value;
                }
                if let Some(value) =
                    crate::typedarray_props::typed_array_get_own_property_value(ta, key)
                {
                    return JSValue::from_bits(value.to_bits());
                }
                if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
                    let elem_size = crate::typedarray::elem_size_for_kind(kind);
                    match key_bytes {
                        b"length" => {
                            let len = crate::typedarray::js_typed_array_length(ta);
                            return JSValue::number(len as f64);
                        }
                        b"byteLength" => {
                            let len = crate::typedarray::js_typed_array_length(ta);
                            return JSValue::number((len as usize * elem_size) as f64);
                        }
                        b"buffer" => {
                            let buf = crate::typedarray_view::js_typed_array_backing_buffer(ta);
                            if buf.is_null() {
                                return JSValue::undefined();
                            }
                            return JSValue::from_bits(
                                crate::value::js_nanbox_pointer(buf as i64).to_bits(),
                            );
                        }
                        b"byteOffset" => {
                            return JSValue::number(
                                crate::typedarray_view::js_typed_array_byte_offset(ta) as f64,
                            )
                        }
                        b"BYTES_PER_ELEMENT" => return JSValue::number(elem_size as f64),
                        // `(new Int8Array(…)).constructor === Int8Array`. The
                        // instance never carries an own `constructor`; it is
                        // inherited from the per-kind prototype. Resolve it to
                        // the global per-kind constructor value so identity holds
                        // (matches the buffer branch below and the `Number`
                        // auto-box path). Custom-prototype views (set via the
                        // `Reflect.construct` newTarget path) record their own
                        // prototype and resolve `.constructor` through that
                        // chain instead — handled before this native fallback.
                        b"constructor" => {
                            // A custom-`[[Prototype]]` view (Reflect.construct
                            // with a newTarget whose `.prototype` is an object)
                            // inherits `.constructor` through that prototype
                            // chain, NOT from the per-kind constructor.
                            if let Some(proto_bits) =
                                super::super::prototype_chain::object_static_prototype(addr)
                            {
                                if proto_bits != crate::value::TAG_NULL {
                                    let proto = JSValue::from_bits(proto_bits);
                                    if proto.is_pointer() {
                                        let p = proto.as_pointer::<ObjectHeader>();
                                        return super::super::js_object_get_field_by_name(p, key);
                                    }
                                }
                            }
                            // A user patch on the per-kind prototype
                            // (`Object.defineProperty(TA.prototype,
                            // "constructor", { get })` or a data overwrite)
                            // shadows the intrinsic — run the getter with
                            // `this` = the view (observable; test262
                            // speciesctor-get-ctor-inherited reads
                            // `result.constructor` and counts calls).
                            if let Some(v) =
                                crate::typedarray::species::prototype_constructor_patch(kind, addr)
                            {
                                return JSValue::from_bits(v.to_bits());
                            }
                            let name = crate::typedarray::name_for_kind(kind);
                            let ctor = super::super::js_get_global_this_builtin_value(
                                name.as_ptr(),
                                name.len(),
                            );
                            return JSValue::from_bits(ctor.to_bits());
                        }
                        _ => {}
                    }
                } else {
                    let buf = addr as *const crate::buffer::BufferHeader;
                    match key_bytes {
                        b"length" | b"byteLength" => {
                            return JSValue::number(crate::buffer::js_buffer_length(buf) as f64);
                        }
                        b"buffer" | b"parent" => {
                            let alias = crate::buffer::buffer_backing_array_buffer(addr);
                            return JSValue::from_bits(
                                crate::value::js_nanbox_pointer(alias as i64).to_bits(),
                            );
                        }
                        b"byteOffset" | b"offset" => {
                            let offset = crate::buffer::buffer_byte_offset(addr);
                            return JSValue::number(offset as f64);
                        }
                        b"BYTES_PER_ELEMENT" => return JSValue::number(1.0),
                        b"constructor" => {
                            // An ArrayBuffer / SharedArrayBuffer cell answers
                            // with ITS constructor — only the Uint8Array
                            // (Buffer-backed view) representation reports
                            // `Uint8Array` (`ta.buffer.constructor ===
                            // ArrayBuffer`, test262 ctors/buffer-arg/
                            // typedarray-backed-by-sharedarraybuffer).
                            let name: &[u8] = if crate::buffer::is_shared_array_buffer(addr) {
                                b"SharedArrayBuffer"
                            } else if crate::buffer::is_any_array_buffer(addr) {
                                b"ArrayBuffer"
                            } else {
                                b"Uint8Array"
                            };
                            let ctor = super::super::js_get_global_this_builtin_value(
                                name.as_ptr(),
                                name.len(),
                            );
                            return JSValue::from_bits(ctor.to_bits());
                        }
                        _ => {}
                    }
                }
            }
        }
        // #4363 regression fix: a secret-key Uint8Array (KeyObject backing
        // buffer) exposes `type` / `symmetricKeySize` / `asymmetricKey*`
        // through the KeyObject metadata block later in this function. The
        // typed-array own-property fallback must not shadow those with
        // `undefined` — fall through for a secret-key buffer so the metadata
        // block resolves them. Plain typed arrays keep the `undefined` result.
        if !crate::buffer::is_secret_key(addr) {
            return JSValue::undefined();
        }
    }
    // #2128: a plain JS number value (a finite double or canonical NaN —
    // anything `JSValue::is_number` returns true for *minus* the raw-I64
    // pointer convention where top16 == 0) reaches this generic property-get
    // when codegen lacks static type info — e.g. drizzle's
    // `buildQueryFromSourceParams` mapping a chunk that happens to be a
    // bound-param number (`1` row-id, `31` age). Without this guard the
    // receiver's f64 bits get bit-cast to a pointer and the first downstream
    // helper that reads a GC header (`is_registered_set` here, `(*obj).field_*`
    // elsewhere) derefs unmapped memory and SIGSEGVs. Spec: property access
    // on a primitive number returns undefined for unknown keys (we don't
    // auto-box to Number.prototype here; that's handled by the method-dispatch
    // path, not this property-getter slow path). Heap pointers stored as raw
    // I64 (module-level objects) have top16 == 0 and are preserved by this
    // check.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        // Two shapes of primitive-number receiver reach this generic slow
        // path: (a) a finite double whose top16 is neither a NaN-box tag
        // nor zero — most numbers (1.0 has top16 0x3FF0, -3.14 has
        // 0xC008...), and (b) the f64 +0.0 whose full bit pattern is
        // `0` — distinguishable from a raw heap pointer because real
        // ObjectHeader allocations live above 0x10000 and from null /
        // undefined because both are NaN-boxed with top16 == 0x7FFC.
        let is_primitive_number =
            (top16 != 0 && !(0x7FF9..=0x7FFF).contains(&top16)) || (top16 == 0 && bits == 0);
        if is_primitive_number {
            // #5989: a live Web Stream handle is itself a finite positive float
            // (`id as f64`, stream band [0x100000, 0x200000)), so it classifies
            // as a primitive number HERE and returned `undefined` for every
            // property — the dedicated stream arm further down never ran.
            // React's `renderToReadableStream` reads back the `allReady`
            // expando it attached (`stream.allReady`), got `undefined`, and the
            // Next.js dynamic render 500'd on `undefined.finally`. Route a
            // registered stream id to the handle property dispatcher (getter /
            // bound-method / expando arms) before the primitive-number return.
            {
                let f = f64::from_bits(bits);
                if !key.is_null() && f.is_finite() && f > 0.0 && f.fract() == 0.0 {
                    let id = f as usize;
                    if crate::value::addr_class::is_stream_id_band(id) {
                        if let Some(probe) = crate::object::stream_handle_probe() {
                            unsafe {
                                if probe(id) {
                                    if let Some(dispatch) = handle_property_dispatch() {
                                        let key_ptr = (key as *const u8)
                                            .add(std::mem::size_of::<crate::StringHeader>());
                                        let key_len = (*key).byte_len as usize;
                                        let v = dispatch(id as i64, key_ptr, key_len);
                                        return JSValue::from_bits(v.to_bits());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // #2138: auto-box the primitive number for the inherited
            // `.constructor` read so `n.constructor === Number` (and the
            // duck-type `value.constructor.name === "Number"` lodash/date-fns
            // use to discriminate primitives). Route through the same
            // `js_get_global_this_builtin_value` helper that backs bare-`Number`
            // identifier resolution so identity comparison holds. Other unknown
            // keys still return undefined per #2128 (was SIGSEGV pre-#2128).
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        if let Some(v) =
                            primitive_object_prototype_accessor(name, f64::from_bits(bits))
                        {
                            return v;
                        }
                    }
                    if let Some(v) =
                        primitive_builtin_prototype_property(b"Number", key, f64::from_bits(bits))
                    {
                        return v;
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // A primitive string receiver inherits `.constructor` from String.prototype:
    // `"x".constructor === String` (test262 language/types/string/S8.4_A9/A12).
    // The common string members (`.length`, indices, methods) are served by the
    // codegen fast paths and never reach this generic slow path, so only the
    // inherited `constructor` read needs routing here; resolve it to the same
    // global `String` value bare-`String` yields so identity holds.
    {
        let bits = obj as u64;
        if !key.is_null() && crate::value::JSValue::from_bits(bits).is_any_string() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                if std::slice::from_raw_parts(key_ptr, key_len) == b"constructor" {
                    let ctor =
                        super::super::js_get_global_this_builtin_value(b"String".as_ptr(), 6);
                    return JSValue::from_bits(ctor.to_bits());
                }
            }
        }
    }
    // Native module registry handles can arrive here either as raw small
    // integers or as POINTER_TAG-boxed small integers. Route them before any
    // GC-header probes such as Date/Promise checks.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let raw = if top16 == 0 {
            bits as usize
        } else if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            0
        };
        if crate::value::addr_class::is_small_handle(raw) {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if is_timer_handle_method_key(key_bytes)
                        && crate::timer::is_known_timer_id(raw as i64)
                    {
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(raw as i64).to_bits());
                        let result = super::super::js_class_method_bind(this_f64, key_ptr, key_len);
                        return JSValue::from_bits(result.to_bits());
                    }
                    // TextDecoder/TextEncoder registry handles — see
                    // `text_handle_property` (text.rs).
                    if let Some(v) =
                        crate::text::text_handle_property(raw, key_bytes, key_ptr, key_len)
                    {
                        return v;
                    }
                    if key_bytes == b"constructor" {
                        let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                        return JSValue::from_bits(JSValue::pointer(null_obj_ptr).bits());
                    }
                    if let Some(dispatch) = handle_property_dispatch() {
                        let bits = dispatch(raw as i64, key_ptr, key_len);
                        // Wall 10 — fall back to a `setPrototypeOf(handle, proto)`
                        // member (Express's augmented `res`/`req`) when the native
                        // dispatch doesn't know the key. See
                        // `handle_proto_inherited_field`.
                        if bits.to_bits() == crate::value::TAG_UNDEFINED {
                            if let Some(v) = handle_proto_inherited_field(raw, key) {
                                return v;
                            }
                        }
                        return JSValue::from_bits(bits.to_bits());
                    }
                    if let Some(v) = handle_proto_inherited_field(raw, key) {
                        return v;
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // #2089: a `Date` is a NaN-boxed pointer to an 8-byte `DateCell`. A
    // generic property read on it (`date.constructor`, `date[k]`, a method
    // read as a value) must NOT fall through to the object-deref path below —
    // the cell is far smaller than an `ObjectHeader`, so reading its
    // `keys_array`/field slots would deref unmapped memory. Resolve the few
    // meaningful reads here and return `undefined` for everything else
    // (matching property reads on the old value-type Date). `obj` may arrive
    // NaN-boxed (top16 == 0x7FFD) or as a raw-I64 pointer (top16 == 0).
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let addr = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        if addr != 0 && crate::date::is_date_cell_addr(addr) {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    // User expando / defineProperty'd own properties first.
                    if let Ok(name) = std::str::from_utf8(key_bytes) {
                        let receiver = f64::from_bits(
                            crate::value::JSValue::pointer(addr as *const u8).bits(),
                        );
                        if let Some(v) = super::super::exotic_expando::exotic_get_own_property(
                            addr,
                            super::super::exotic_expando::ExoticKind::Date,
                            name,
                            receiver,
                        ) {
                            return JSValue::from_bits(v.to_bits());
                        }
                    }
                    if key_bytes == b"constructor" {
                        let v = js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
                        return JSValue::from_bits(v.to_bits());
                    }
                    // A Date method read as a *value* (`const f = d.getTime`,
                    // `typeof d.toISOString`, `d.toJSON === Date.prototype.toJSON`)
                    // resolves to the same thunk installed on `Date.prototype`.
                    // The `d.method()` call form is handled by codegen's fast
                    // path and never reaches here, so this only affects value
                    // reads. Unknown keys still return undefined.
                    let date_ctor = js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
                    let cv = JSValue::from_bits(date_ctor.to_bits());
                    if cv.is_pointer() {
                        let ctor_ptr = cv.as_pointer::<crate::closure::ClosureHeader>() as usize;
                        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
                        let pv = JSValue::from_bits(proto.to_bits());
                        if pv.is_pointer() {
                            let proto_ptr = pv.as_pointer::<ObjectHeader>();
                            if !proto_ptr.is_null() {
                                let m = js_object_get_field_by_name(proto_ptr, key);
                                if !m.is_undefined() {
                                    return JSValue::from_bits(m.bits());
                                }
                            }
                        }
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // Temporal cell (#4686): like Date, a `Temporal.*` value is a NaN-boxed
    // pointer to a small cell that must NOT fall through to the object-deref
    // path. Resolve its getters (`duration.years`, `plainDate.month`, …) here
    // and return `undefined` for anything else (a Temporal method read as a
    // bare value is rare; the `value.method()` call form is handled in
    // `js_native_call_method`). `obj` may be NaN-boxed (top16 0x7FFD) or a
    // raw-I64 pointer (top16 0).
    #[cfg(feature = "temporal")]
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let addr = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        if addr != 0 && crate::temporal::is_temporal_cell_addr(addr) {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    let name = String::from_utf8_lossy(key_bytes);
                    let boxed = f64::from_bits(JSValue::pointer(addr as *const u8).bits());
                    // A user-defined own expando property (`Object.defineProperty`
                    // / plain assignment) shadows the built-in prototype getters,
                    // per OrdinaryGet walking own properties before the prototype.
                    if let Some(v) = super::super::exotic_expando::exotic_get_own_property(
                        addr,
                        super::super::exotic_expando::ExoticKind::Temporal,
                        &name,
                        boxed,
                    ) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    if let Some(v) = crate::temporal::dispatch::get_property(boxed, &name) {
                        return JSValue::from_bits(v.to_bits());
                    }
                    // A prototype METHOD read as a value (`d.abs`, not `d.abs()`):
                    // return a bound method that re-dispatches through
                    // `js_native_call_method`. Needed because codegen lowers a
                    // spread/dynamic call `d[m](...args)` to a property read + apply,
                    // so the read must yield a callable. Only bind genuine method
                    // names so an unknown property still reads as `undefined`. (#5587)
                    if crate::temporal::dispatch::has_method(boxed, &name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        let bound = js_class_method_bind(boxed, heap_name, key_bytes.len());
                        return JSValue::from_bits(bound.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // Issue #818 (Effect class-instance pattern): a V8 handle (JS_HANDLE_TAG
    // = 0x7FFB) reaches here when codegen routes a generic `PropertyGet`
    // through this slow path — e.g. `Effect.succeed(42).value` where the
    // call return was a JS handle but the HIR `js_transform` pass didn't
    // rewrite the consumer-side `.value` into `JsGetProperty` (because the
    // call lowered as a `StaticMethodCall`, not as a `JsCallMethod`). The
    // method-call counterpart in `js_call_method` already routes
    // JS_HANDLE_TAG values to V8 via JS_HANDLE_CALL_METHOD; do the same
    // here via JS_HANDLE_OBJECT_GET_PROPERTY so subsequent property reads
    // on a returned class instance reach the live V8 object instead of
    // falling to the small-handle dispatch (which only knows about
    // Fastify/axios/sqlite, not generic V8 handles).
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFB && !key.is_null() {
            let func_ptr = crate::value::JS_HANDLE_OBJECT_GET_PROPERTY
                .load(std::sync::atomic::Ordering::SeqCst);
            if !func_ptr.is_null() {
                let func: unsafe extern "C" fn(f64, *const i8, usize) -> f64 =
                    unsafe { std::mem::transmute(func_ptr) };
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let result = func(f64::from_bits(bits), key_ptr as *const i8, key_len);
                    return JSValue::from_bits(result.to_bits());
                }
            }
            return JSValue::undefined();
        }
    }
    // Issue #618-followup: read INT32-tagged class ref's dynamic property
    // from the side-table (mirror of the set-side intercept). For drizzle's
    // `SQL.Aliased` lookup pattern.
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFE && !key.is_null() {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            let class_value = f64::from_bits(bits);
            let is_prototype_ref = super::super::class_prototype_ref_id(class_value).is_some();
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("");
                // v0.5.752: class_ref.constructor synthesizes back to the
                // same class ref so drizzle's
                // `Object.getPrototypeOf(value).constructor === Class` chain
                // collapses correctly (with v0.5.751's getPrototypeOf
                // returning the class ref for instance receivers). Refs
                // #420 / #618 followup.
                if is_prototype_ref
                    && name == "constructor"
                    && class_id != 0
                    && class_has_own_method(class_id, name)
                {
                    let value = class_prototype_method_value_for_name(class_id, name);
                    return JSValue::from_bits(value.to_bits());
                }
                if name == "constructor" && class_id != 0 && is_class_id_registered(class_id) {
                    let value = if is_prototype_ref {
                        super::super::class_constructor_ref_value(class_id)
                    } else {
                        class_value
                    };
                    return JSValue::from_bits(value.to_bits());
                }
                if name == "prototype"
                    && class_id != 0
                    && is_class_id_registered(class_id)
                    && !is_prototype_ref
                {
                    let value = super::super::class_registry::class_decl_prototype_value(class_id);
                    if value.to_bits() == crate::value::TAG_UNDEFINED {
                        let value = super::super::class_prototype_ref_value(class_id);
                        return JSValue::from_bits(value.to_bits());
                    }
                    return JSValue::from_bits(value.to_bits());
                }
                // Instance (prototype) methods must only resolve when reading
                // off the prototype ref (`C.prototype.m`), NOT off the class ref
                // itself (`C.m`). In JS a class object does not expose its
                // prototype methods as static members: `class C { m(){} }` has
                // `C.m === undefined` (the method lives on `C.prototype`). The
                // earlier unconditional lookup leaked instance methods onto the
                // class ref, so `C.m` returned a (mis-bound) function. This
                // broke NestJS interceptor/guard/pipe resolution: its
                // `getInterceptorInstance` duck-types `!!metatype.intercept` to
                // decide "is this a class or an already-built instance"; a
                // truthy `Class.intercept` made it treat the CLASS as the
                // instance, so `intercept()` ran with a broken receiver and
                // returned `{}`, which rxjs `innerFrom` then rejected. Real
                // static methods are resolved below via
                // `lookup_static_method_in_chain`.
                if is_prototype_ref && class_id != 0 && class_has_own_method(class_id, name) {
                    let value = class_prototype_method_value_for_name(class_id, name);
                    return JSValue::from_bits(value.to_bits());
                }
                if is_prototype_ref {
                    if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                        if let Some(ref reg) = *registry {
                            let mut cid = class_id;
                            let mut depth = 0usize;
                            while depth < 32 {
                                if let Some(vtable) = reg.get(&cid) {
                                    if let Some(&getter_ptr) = vtable.getters.get(name) {
                                        let f: extern "C" fn(f64) -> f64 =
                                            std::mem::transmute(getter_ptr);
                                        return JSValue::from_bits(f(class_value).to_bits());
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
                    return JSValue::undefined();
                }
                // Empty-string is a legal static member key (`static get ''()`);
                // the `!name.is_empty()` guard below skips it, so resolve a
                // static accessor named "" here (Test262 accessor-name-static
                // literal-string-empty).
                if name.is_empty() {
                    if let Some(v) =
                        super::super::class_registry::class_static_accessor_getter_value(
                            class_id,
                            name,
                            class_value,
                        )
                    {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
                if !name.is_empty() {
                    if super::super::class_registry::class_is_key_deleted(class_id, name) {
                        return JSValue::undefined();
                    }
                    let result = CLASS_DYNAMIC_PROPS.with(|m| {
                        m.borrow()
                            .get(&class_id)
                            .and_then(|props| props.get(name).copied())
                    });
                    if let Some(v) = result {
                        return JSValue::from_bits(v.to_bits());
                    }
                    // Static DATA fields are INHERITED by subclasses, exactly like
                    // static methods: `class D {}; D.kind = "x"; class G extends D {}`
                    // makes `G.kind === "x"` (the class-object proto chain
                    // `G.__proto__ === D` carries statics). The own-field read above
                    // only consulted `class_id`; walk the parent class_id chain here
                    // so an inherited static field (or runtime `Parent.x = …`
                    // assignment — both live in CLASS_DYNAMIC_PROPS) resolves. Static
                    // METHODS are handled by `lookup_static_method_in_chain` below;
                    // this covers the data-field case that was returning `undefined`
                    // (Auth.js sets `SignInError.kind = "signIn"` and reads it off a
                    // `CredentialsSignin` subclass to pick the sign-in vs error page).
                    //
                    // #6530: `name` is an OWN property of every constructor — a
                    // subclass never inherits its parent's `.name` (spec:
                    // ClassDefinitionEvaluation installs it per class). Skip the
                    // chain walk so the #2059 own-name synthesis below answers
                    // with THIS class's registered name instead of an ancestor's.
                    if name != "name" {
                        // Walk the class-object proto chain for an inherited static
                        // DATA field. At EACH level the class's pinned
                        // per-evaluation parent OBJECT is consulted BEFORE the
                        // parent's registry props (`CLASS_DYNAMIC_PROPS`).
                        //
                        // #6552: a subclass of a class-EXPRESSION value evaluated
                        // more than once (`function make(a){return class{static
                        // ast=a}}`, then `class Number$ extends make(x) {}` /
                        // `class Widget$ extends make(y) {}`) records THIS
                        // evaluation's parent object as its static prototype
                        // (`class_prototype_object`, #1788), but the parent's
                        // `CLASS_DYNAMIC_PROPS` are keyed by the class-expression
                        // TEMPLATE id — shared, last-wins across every evaluation.
                        // Reading the registry entry for such a parent collapses
                        // sibling subclasses to the LAST `make(...)` (effect Schema:
                        // `Number$.ast`/`Widget$.ast` both read the last parent's
                        // `ast`). The pinned object carries this evaluation's own
                        // edge, so it is authoritative; the registry read remains
                        // the fallback for a plain declaration parent (#6443:
                        // Auth.js `SignInError.kind`), which has no pinned object.
                        let mut child = class_id;
                        let mut depth = 0usize;
                        while depth < 32 {
                            let proto = super::super::class_registry::class_prototype_object(child);
                            if !proto.is_null() {
                                let v = js_object_get_field_by_name(proto as *const _, key);
                                // Return a value present on the pinned object even
                                // when it is `null` — a static explicitly set to
                                // `null` on THIS evaluation is authoritative and
                                // must not fall through to the last-wins registry
                                // entry (a sibling evaluation's value). Only
                                // `undefined` means "absent here", which continues
                                // the walk to the parent's registry props / a higher
                                // ancestor.
                                if !v.is_undefined() {
                                    return v;
                                }
                            }
                            let p = match get_parent_class_id(child) {
                                Some(p) if p != 0 && p != child => p,
                                _ => break,
                            };
                            // A key deleted on THIS ancestor is not provided by it,
                            // but a higher ancestor may still define it — `delete
                            // Mid.foo` must let `Sub.foo` inherit `Base.foo`, not
                            // resolve to undefined. Skip the registry read for the
                            // deleted level and keep walking up.
                            if !super::super::class_registry::class_is_key_deleted(p, name) {
                                let inherited = CLASS_DYNAMIC_PROPS.with(|m| {
                                    m.borrow()
                                        .get(&p)
                                        .and_then(|props| props.get(name).copied())
                                });
                                if let Some(v) = inherited {
                                    return JSValue::from_bits(v.to_bits());
                                }
                            }
                            child = p;
                            depth += 1;
                        }
                    }
                    if super::super::class_registry::lookup_static_method_in_chain(class_id, name)
                        .is_some()
                    {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(name_len.max(1), 1).unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(name_ptr, ptr, name_len);
                            ptr
                        };
                        let result = js_class_method_bind(class_value, heap_name, name_len);
                        return JSValue::from_bits(result.to_bits());
                    }
                    // `class X extends Promise` — a value read of an inherited
                    // builtin static (`X.resolve`, `X.all`, …) resolves to the
                    // reified Promise static (so `X.resolve.bind(X)` works). Only
                    // fires when no user static shadowed it above.
                    if super::super::promise_parent_in_chain(class_id)
                        && super::super::promise_static_function_spec(name).is_some()
                    {
                        let v = super::super::js_promise_static_function_value(name_ptr, name_len);
                        if v.to_bits() != crate::value::TAG_UNDEFINED {
                            return JSValue::from_bits(v.to_bits());
                        }
                    }
                    if let Some(v) =
                        super::super::class_registry::class_static_accessor_getter_value(
                            class_id,
                            name,
                            class_value,
                        )
                    {
                        return JSValue::from_bits(v.to_bits());
                    }
                    // #1788: a subclass of a class-expression value
                    // (`class Sub extends make("A") {}`) inherits the parent
                    // class OBJECT's OWN per-evaluation static fields. The
                    // parent object was recorded as `class_id`'s static
                    // prototype at `extends` time; walk that chain (also
                    // covering multi-level `class Leaf extends Mid {}`).
                    // #6530: except `name` — an own property of every
                    // constructor, never inherited; without the guard a
                    // subclass of a per-evaluation class object reported its
                    // BASE's synthesized `.name` (bundled zod:
                    // `z.string().constructor.name` gave "ZodType").
                    if name != "name" {
                        if let Some(v) =
                            super::super::class_registry::resolve_proto_chain_field(class_id, key)
                        {
                            if !v.is_undefined() && !v.is_null() {
                                return v;
                            }
                        }
                    }
                    // #36 / #321: the subclass extends a FUNCTION value
                    // (`class Svc extends Context.Tag(id)<...>() {}`). Read the
                    // named static off the parent closure — its OWN props
                    // (`Svc.key` → "Svc") plus, via the closure getter, its
                    // static prototype (`Svc._op` → "Tag" on TagProto).
                    if let Some(closure_ptr) =
                        super::super::class_registry::class_parent_closure(class_id)
                    {
                        let v = crate::closure::closure_get_dynamic_prop(closure_ptr, name);
                        let vb = JSValue::from_bits(v.to_bits());
                        if !vb.is_undefined() && !vb.is_null() {
                            return vb;
                        }
                    }
                    // #2059: the constructor's built-in `name` own property —
                    // the class name. Checked last so an explicit static
                    // `name` member (method/field, handled above) still wins.
                    // This is what `assert.throws` reads via
                    // `thrown.constructor.name` to label the thrown error.
                    if name == "name"
                        && class_id != 0
                        && !super::super::class_registry::class_is_key_deleted(class_id, name)
                    {
                        if let Some(cname) =
                            super::super::class_registry::class_name_for_id(class_id)
                        {
                            let s = crate::string::js_string_from_bytes(
                                cname.as_ptr(),
                                cname.len() as u32,
                            );
                            return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                        }
                    }
                    // No own static / inherited entry resolved the name. A class
                    // constructor is a function, so a bare read of `.caller` or
                    // `.arguments` hits the poison-pill %ThrowTypeError% accessor
                    // on `Function.prototype` — strict-mode throws (Perry only
                    // compiles strict code). Placed last so any own static field,
                    // accessor, or `defineProperty`-installed data prop of that
                    // name takes precedence. Prototype-refs (`C.prototype`) are
                    // plain objects and are excluded.
                    if !is_prototype_ref && matches!(name, "caller" | "arguments") {
                        crate::fs::validate::throw_type_error_with_code(
                            "Restricted function property access",
                            "ERR_INVALID_ARG_TYPE",
                        );
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // #1545: Promise `then`/`catch`/`finally` value-reads return a bound
    // function so `typeof p.then === "function"`, `const f = p.then`, and
    // passing `p.then` as a deferred callback all work. (The call form
    // `p.then(cb)` is lowered directly to `js_promise_then` by codegen.)
    // `obj` arrives NaN-boxed POINTER-tagged here; mask to the raw promise
    // pointer and confirm via the GC header before treating it as a promise.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        // Callers reach this helper with either a NaN-boxed POINTER-tagged
        // value (0x7FFD, e.g. the `_f64` wrapper) or an already-masked raw
        // heap pointer (top16 == 0, e.g. the PIC miss handler), so accept both.
        let raw = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        // Native-module registry handles live in the handle band and can also
        // be POINTER_TAG-boxed; do not walk back to a GcHeader for those.
        if crate::value::addr_class::is_plausible_heap_addr(raw) && !key.is_null() {
            {
                unsafe {
                    let gc_header = (raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                    // Buffers / typed arrays are `std::alloc`-backed and carry
                    // NO GcHeader, so the byte at `raw - 8` is unrelated memory
                    // that can read as `GC_TYPE_PROMISE` (5) by coincidence on
                    // an IC-miss read. Exclude them before acting — otherwise a
                    // genuine buffer metadata read would early-return undefined.
                    if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE
                        && !crate::buffer::is_registered_buffer(raw)
                        && crate::typedarray::lookup_typed_array_kind(raw).is_none()
                    {
                        let name_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let name_len = (*key).byte_len as usize;
                        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                        let prop = std::str::from_utf8_unchecked(name_bytes);
                        // #5142: a user-attached own expando (`p.status = …`,
                        // `Object.assign(p, …)`) wins over the inherited
                        // prototype method. @tanstack/query-core's
                        // `pendingThenable()` stores `status`/`value` on the
                        // promise and gates its retryer on `thenable.status`;
                        // without this the read came back `undefined`,
                        // `isResolved()` was permanently true, and the fetch
                        // never resolved.
                        if let Some(v) = super::super::exotic_expando::exotic_get_own_property(
                            raw,
                            super::super::exotic_expando::ExoticKind::Promise,
                            prop,
                            f64::from_bits(obj as u64),
                        ) {
                            return JSValue::from_bits(v.to_bits());
                        }
                        if matches!(name_bytes, b"then" | b"catch" | b"finally") {
                            if let Some(v) = crate::promise::js_promise_bound_method(
                                raw as *mut crate::promise::Promise,
                                prop,
                            ) {
                                return JSValue::from_bits(v.to_bits());
                            }
                        }
                        // `promise.constructor` is the global `Promise`
                        // (inherited from `Promise.prototype.constructor`). Any
                        // own expando (`p.constructor = X`) already returned via
                        // `exotic_get_own_property` above. execa
                        // (`(async () => {})().constructor.prototype`) reads it
                        // to capture the native promise prototype — without this
                        // arm it fell through to `undefined` and
                        // `.prototype` threw `Cannot read properties of
                        // undefined`.
                        if name_bytes == b"constructor" {
                            let v = crate::object::js_get_global_this_builtin_value(
                                b"Promise".as_ptr(),
                                7,
                            );
                            return JSValue::from_bits(v.to_bits());
                        }
                        // A Promise is a `GC_TYPE_PROMISE` cell, not an
                        // `ObjectHeader`; never fall through to the field/vtable
                        // path below (it would reinterpret the promise's bytes).
                        return JSValue::from_bits(crate::value::TAG_UNDEFINED);
                    }
                }
            }
        }
    }
    // SSO property access (v0.5.213 Step 1 gate). The codegen inline
    // `.length` path routes SHORT_STRING_TAG receivers here because
    // it doesn't yet know about the SSO tag. Handle `.length` by
    // reading the length byte directly from the NaN-box payload.
    // Other property accesses on an SSO string (e.g. `.charAt` via
    // `[0]`, `.slice`) aren't yet routed here — handled by the
    // string method dispatch in a future migration step; today they
    // fall through to "undefined" which matches the behavior for
    // string-valued property access on untyped locals in general.
    {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if key_bytes == b"length" {
                        let len = (obj_bits & crate::value::SHORT_STRING_LEN_MASK)
                            >> crate::value::SHORT_STRING_LEN_SHIFT;
                        return JSValue::number(len as f64);
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // #1670: Web Streams handles are returned as `id as f64` (a normal
    // float, NOT NaN-boxed) just above the pointer-tagged small-handle band, so
    // an inline `res.body.locked` reaches this generic field-get with `obj`
    // carrying the IEEE-754 bits of the stream id.
    // The NaN-box-strip + small-handle branches below don't recognise it
    // (top16 is an ordinary exponent, not a tag; the value as a pointer is
    // far above 0x100000), so it would be dereferenced as a heap pointer →
    // segfault. Decode the float; when the stdlib probe confirms a live
    // stream handle, route the property read through the handle property
    // dispatcher (which carries the #1670 stream getter/method arms).
    // Mirrors the method-dispatch path in `native_call_method.rs` (#1545).
    // The typed-local path (`const b = res.body; b.locked`) lowers as a
    // 0-arg NativeMethodCall getter and never reaches here.
    {
        let f = f64::from_bits(obj as u64);
        if !key.is_null() && f.is_finite() && f > 0.0 && f.fract() == 0.0 {
            let id = f as usize;
            if crate::value::addr_class::is_stream_id_band(id) {
                if let Some(probe) = crate::object::stream_handle_probe() {
                    unsafe {
                        if probe(id) {
                            if let Some(dispatch) = handle_property_dispatch() {
                                let key_ptr = (key as *const u8)
                                    .add(std::mem::size_of::<crate::StringHeader>());
                                let key_len = (*key).byte_len as usize;
                                let bits = dispatch(id as i64, key_ptr, key_len);
                                return JSValue::from_bits(bits.to_bits());
                            }
                        }
                    }
                }
            }
        }
    }
    // #2058: a raw, unboxed finite f64 NUMBER receiver (e.g. `(5).toString`,
    // or `n.isPrototypeOf` where `n: number`) reaches here with its float
    // bits intact — numbers are NOT NaN-boxed in Perry, so `5.0` arrives as
    // 0x4014_0000_0000_0000. That is neither a NaN-box tag (top16 >= 0x7FF8)
    // nor a masked heap pointer (those have top16 == 0), so the generic
    // pointer logic below would dereference the float bits as an
    // `ObjectHeader` → SIGSEGV. Detect the primitive number first: return a
    // bound-method closure for the inherited Number/Object prototype methods
    // (so `typeof n.toString === "function"` holds and the value is
    // callable), and `undefined` for any other key (matching property reads
    // on primitives). Date timestamps and Web-Stream handles are raw f64 too,
    // but both are special-cased above, so they never reach this branch.
    {
        let bits = obj as u64;
        let f = f64::from_bits(bits);
        // A Date is now a NaN-boxed `DateCell` pointer (non-finite bit
        // pattern), intercepted earlier in this function, so it never reaches
        // this finite-number branch.
        if !key.is_null() && f.is_finite() && (bits >> 48) != 0 {
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                if let Ok(name) = std::str::from_utf8(name_bytes) {
                    if let Some(v) = primitive_object_prototype_accessor(name, f) {
                        return v;
                    }
                }
                if let Some(v) = primitive_builtin_prototype_property(b"Number", key, f) {
                    return v;
                }
                if is_primitive_proto_method(name_bytes) {
                    let result = super::super::js_class_method_bind(f, name_ptr, name_len);
                    return JSValue::from_bits(result.to_bits());
                }
            }
            return JSValue::undefined();
        }
    }
    get_field_by_name_object_tail(obj, key)
}

#[cfg(test)]
mod null_key_guard_5972 {
    use super::*;

    /// #5972 part 2: a null key (produced when a NaN/number property-key
    /// coerces to no usable string handle) must miss → `undefined`, never
    /// SIGSEGV by dereferencing `(*key).byte_len` at offset 4.
    #[test]
    fn null_key_returns_undefined_not_segfault() {
        unsafe {
            let obj = crate::object::js_object_alloc(0, 0);
            let key = crate::string::js_string_from_bytes(b"present".as_ptr(), 7);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);

            // Sanity: the real key resolves.
            let hit = js_object_get_field_by_name(obj, key);
            assert_eq!(f64::from_bits(hit.bits()), 42.0);

            // The regression: a null key must not crash and must read undefined.
            let miss = js_object_get_field_by_name(obj, std::ptr::null());
            assert_eq!(
                miss.bits(),
                crate::value::TAG_UNDEFINED,
                "null key should miss → undefined"
            );
        }
    }
}
