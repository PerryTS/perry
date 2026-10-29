//! A method call `recv.m(args)` (object model step 5, phase 2,
//! OBJECT_MODEL_2026-10-13.md §3.7): a [[Get]] of `m` and a call with `recv`
//! as `this`. Nothing here holds site state.
//!
//! The emitted site (perry-codegen `expr/method_site.rs`) asks the
//! unknown-receiver probe (`layout_probe.rs`) for the key, own then along the
//! prototype chain, and calls a plain function value it answers with the
//! receiver as `this` (`js_closure_call{N}`). Everything the probe declines
//! (an accessor, a Proxy, an exotic receiver, a primitive, a missing method)
//! and a value the site does not call itself takes [`js_method_call_slow`]:
//! the universal by-name dispatcher.
//!
//! A call whose arguments can observe WHEN the method is read (#11910,
//! ECMA-262 13.3.6.1) reads it first: [`js_method_lookup`] answers the
//! method value, or one of the by-name answers ([`METHOD_CALL_BY_NAME`],
//! [`METHOD_CALL_BY_NAME_DIRECT`]) for a receiver whose read nothing can
//! observe, and [`js_method_call_split`] makes the call after the arguments.
use crate::object::ObjectHeader;

/// "Dispatch by name after the arguments": no JS value has this pattern
/// (`TAG_HOLE`).
pub const METHOD_CALL_BY_NAME: u64 = crate::codegen_abi::METHOD_SITE_BY_NAME;
const _: () = assert!(METHOD_CALL_BY_NAME == crate::value::TAG_HOLE);
/// "Dispatch by name after the arguments, through the universal dispatcher".
pub const METHOD_CALL_BY_NAME_DIRECT: u64 = crate::codegen_abi::METHOD_SITE_BY_NAME_DIRECT;
const _: () = assert!(
    METHOD_CALL_BY_NAME_DIRECT & !0xFF == crate::value::TAG_HOLE & !0xFF
        && METHOD_CALL_BY_NAME_DIRECT != crate::value::TAG_HOLE
        && METHOD_CALL_BY_NAME_DIRECT != crate::value::TAG_TDZ
        && METHOD_CALL_BY_NAME_DIRECT > crate::value::TAG_TRUE
);
const METHOD_SITE_BY_NAME: u64 = METHOD_CALL_BY_NAME;
const METHOD_SITE_BY_NAME_DIRECT: u64 = METHOD_CALL_BY_NAME_DIRECT;

/// The emitted fused receiver test: a POINTER-tagged value whose payload is
/// above the native-handle band, i.e. a heap object.
#[inline(always)]
fn is_site_receiver(recv: f64) -> bool {
    const BIAS: u64 = crate::value::POINTER_TAG | perry_abi::RECEIVER_HANDLE_FLOOR as u64;
    const SPAN: u64 = (1u64 << 48) - perry_abi::RECEIVER_HANDLE_FLOOR as u64;
    recv.to_bits().wrapping_sub(BIAS) < SPAN
}

/// A method site's call whenever the site did not call the probe's answer
/// itself: the universal by-name dispatch. `key_bits` is the site's key atom
/// (the probe verify mode checks a held answer against it).
///
/// # Safety
/// `args_ptr` holds `argc` values.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn js_method_call_slow(
    site_id: u64,
    recv: f64,
    key_bits: u64,
    method_id: i64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    if is_site_receiver(recv) {
        let addr = (recv.to_bits() & crate::value::POINTER_MASK) as usize;
        super::layout_probe::verify::check(addr, key_bits);
        // Diagnostics: explain a probe decline (not a value the site chose
        // not to call itself).
        if super::layout_probe::diag_enabled()
            && key_bits & crate::value::TAG_MASK == crate::value::STRING_TAG
            && super::layout_probe::perry_probe_get_value(
                super::shapes::ordinary_dir_addr(),
                recv.to_bits(),
                key_bits,
                crate::object::key_bytes_hash(
                    crate::string::string_data(
                        (key_bits & crate::value::POINTER_MASK) as *const crate::StringHeader,
                    ),
                    (*((key_bits & crate::value::POINTER_MASK) as *const crate::StringHeader))
                        .byte_len as usize,
                ),
            )
            .to_bits()
                == crate::value::TAG_HOLE
        {
            super::layout_probe::diag_explain(
                addr as i64,
                (key_bits & crate::value::POINTER_MASK) as *const crate::StringHeader,
            );
        }
        if let Some(h) = crate::value::addr_class::try_read_gc_header(addr) {
            if crate::gc::gc_type_is_object(h.obj_type)
                && crate::native_class_ids::is_native_payload_class_id(
                    (*(addr as *const ObjectHeader)).class_id,
                )
            {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if let Some(name_ref) =
                    crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
                {
                    let name = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
                    if let Some(result) = crate::native_payload::try_payload_method_fast_dispatch(
                        recv, name, args_ptr, argc,
                    ) {
                        return result;
                    }
                }
            }
        }
    }
    crate::typed_feedback::js_typed_feedback_native_call_method_by_id(
        site_id, recv, method_id, args_ptr, argc,
    )
}

/// The lookup half of a split method call (#11910): the probe's answer for
/// an ordinary receiver when it is a function value, else the one [[Get]]
/// that can run code now (a getter, a Proxy trap, a nullish receiver's
/// TypeError), or a by-name answer when nothing can observe the read.
///
/// # Safety
/// `dir` is this agent's ordinary directory or `PERRY_EMPTY_SHAPE_DIR`;
/// `key_bits`/`hash` are the site's key atom and its hash.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_lookup(
    dir: *const u8,
    recv: f64,
    key_bits: u64,
    hash: u64,
    method_id: i64,
) -> f64 {
    if is_site_receiver(recv) {
        let biased =
            (recv.to_bits() & crate::value::POINTER_MASK) - perry_abi::RECEIVER_HANDLE_FLOOR as u64;
        let obj = ((recv.to_bits() & crate::value::POINTER_MASK) as usize) as *const ObjectHeader;
        if super::layout_desc::methods::may_have_methods((*obj).parent_class_id) {
            let family = super::layout_desc::methods::perry_probe_family_method(
                dir,
                biased as i64,
                key_bits,
                hash,
                u32::MAX,
            );
            if family != 0 {
                return f64::from_bits(crate::value::POINTER_TAG | family as u64);
            }
        }
        let v = super::layout_probe::perry_probe_get_value(dir, recv.to_bits(), key_bits, hash);
        let vb = v.to_bits();
        if vb != crate::value::TAG_HOLE
            && vb & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
            && crate::closure::is_closure_ptr((vb & crate::value::POINTER_MASK) as usize)
        {
            return v;
        }
        super::layout_probe::verify::check(
            (recv.to_bits() & crate::value::POINTER_MASK) as usize,
            key_bits,
        );
    }
    prepare(recv, method_id)
}

/// The lookup half of a method call that has no method site (a computed key,
/// a Proxy receiver, a patched builtin name, the spread forms): `id` is a
/// static dispatch id or a key value. See [`prepare`].
///
/// # Safety
/// `id` is a dispatch id or a NaN-boxed key.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_prepare(recv: f64, id: i64) -> f64 {
    prepare(recv, id)
}

/// [`js_method_lookup`] past the probe: the method value to call after the
/// arguments, or one of the by-name answers when the read cannot be observed
/// and the receiver keeps its by-name dispatch after the arguments:
///
/// * an ordinary object performs its one [[Get]] here (any getter or trap on
///   its chain runs now) and hands back a plain closure as the value, so an
///   argument that reassigns `o.m` cannot change which function runs; any
///   other value is called by name unless the read ran code, in which case
///   the value itself is returned;
/// * a native cell (a collection, an array, a buffer) has its methods
///   answered by name; the one read the dispatcher performs that can run
///   code is the receiver's OWN accessor, which is answered here;
/// * a primitive's chain is its wrapper prototype and `Object.prototype`:
///   their shapes say whether either holds the key as an accessor.
#[inline(never)]
unsafe fn prepare(recv: f64, method_id: i64) -> f64 {
    let direct = f64::from_bits(METHOD_SITE_BY_NAME_DIRECT);
    let bits = recv.to_bits();
    let nullish = bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL;
    // A native id (Web Fetch) is an object receiver with no header, and a
    // Proxy's header names no own storage: both take the full walk below.
    let is_pointer = bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        || bits & crate::value::TAG_MASK == crate::value::HANDLE_TAG;
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    // One header read classifies a heap receiver; only an ordinary object
    // or a function object needs more than the accessor facts below.
    let header = crate::value::addr_class::pointer_value_header(bits);
    // A pre-growth alias the program still holds (an array's forwarding
    // stub) is answered from the live head: growth carried the descriptor
    // bit to its header and re-keyed the array's own properties to it.
    let (header, addr) = match header {
        Some(h)
            if h.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                && h.obj_type == crate::gc::GC_TYPE_ARRAY =>
        {
            let live =
                crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader) as usize;
            if live != 0 && live != addr {
                (crate::value::addr_class::try_read_gc_header(live), live)
            } else {
                (Some(h), addr)
            }
        }
        h => (h, addr),
    };
    let kind = header.map(|h| {
        if h.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            u8::MAX
        } else {
            h.obj_type
        }
    });
    let cell = matches!(kind, Some(t) if !crate::gc::gc_type_is_object(t)
        && t != crate::gc::GC_TYPE_CLOSURE && t != crate::gc::GC_TYPE_PROXY && t != u8::MAX);
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return direct;
    };
    let name = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
    if !is_pointer {
        return if nullish || primitive_chain_has_accessor(recv, name) {
            spec_get(recv, name)
        } else {
            direct
        };
    }
    match kind {
        Some(crate::gc::GC_TYPE_OBJECT) => prepare_ordinary(recv, addr, name),
        // A function method is read before its arguments can change it.
        Some(crate::gc::GC_TYPE_CLOSURE) => spec_get(recv, name),
        _ if cell => {
            // An array owns properties beyond its elements only in its named
            // layout (`ArrayHeader.named`, carried by growth; a pre-growth
            // alias was resolved to the live head above).
            let own = if kind == Some(crate::gc::GC_TYPE_ARRAY)
                && !crate::array::array_has_named_properties_resolved(
                    addr as *const crate::array::ArrayHeader,
                ) {
                None
            } else {
                own_getter_value(addr, recv, name)
            };
            match own {
                Some(value) => value,
                None if cell_chain_has_accessor(recv, kind, name) => spec_get(recv, name),
                None => direct,
            }
        }
        // A Proxy, a native handle or a forwarded alias: the full walk (rare).
        _ => {
            if lookup_is_observable(recv, name) {
                spec_get(recv, name)
            } else {
                direct
            }
        }
    }
}

/// The call half of a split method call: `value` is what
/// [`js_method_lookup`] returned. Either by-name answer dispatches through
/// the universal dispatcher; any other value is the method itself, called
/// with `recv` as `this`.
///
/// # Safety
/// `args_ptr` holds `argc` values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_call_split(
    value: f64,
    site_id: u64,
    recv: f64,
    method_id: i64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    match value.to_bits() {
        METHOD_SITE_BY_NAME | METHOD_SITE_BY_NAME_DIRECT => {
            crate::typed_feedback::js_typed_feedback_native_call_method_by_id(
                site_id, recv, method_id, args_ptr, argc,
            )
        }
        _ => js_method_call_value(value, recv, args_ptr, argc),
    }
}

/// Does a native cell's chain hold `name` as an accessor the dispatcher would
/// run? An array's chain is `Array.prototype` (whose shape pins its own
/// [[Prototype]]) and `Object.prototype`, unless some array has had its
/// prototype replaced; any other cell kind is answered for `Object.prototype`
/// and, only when that holds the key as an accessor, by the full walk.
unsafe fn cell_chain_has_accessor(recv: f64, kind: Option<u8>, name: &[u8]) -> bool {
    let object = crate::array::object_prototype_addr_if_resolved();
    if object == 0 {
        return lookup_is_observable(recv, name);
    }
    if kind == Some(crate::gc::GC_TYPE_ARRAY) {
        // An array whose [[Prototype]] was replaced sets the process latch.
        let array = crate::array::array_prototype_addr();
        if super::prototype_chain::array_static_proto_recorded() {
            return lookup_is_observable(recv, name);
        }
        return match builtin_prototype_has_accessor(array, object, name) {
            Some(held) => {
                held || super::key_attrs::object_key_is_accessor(
                    object as *const ObjectHeader,
                    name,
                )
            }
            None => lookup_is_observable(recv, name),
        };
    }
    super::key_attrs::object_key_is_accessor(object as *const ObjectHeader, name)
        && lookup_is_observable(recv, name)
}

/// Does the builtin prototype `proto` (`Array.prototype`, a primitive's
/// wrapper prototype, `%Function.prototype%`) hold `name` as an accessor,
/// given that its own [[Prototype]] is the realm's `Object.prototype`
/// (`object`)? `None` when that link is not known (the caller walks):
///
/// * an ordinary object answers from its shape: the link it pins is the realm
///   default, or a recorded link naming `object`, and its key summary holds
///   the accessor bit;
/// * an array exotic (`Array.prototype` is one) inherits the realm default
///   unless the array-prototype latch is set (checked by the caller); its
///   named-property holder shape answers whether `name` is an accessor.
unsafe fn builtin_prototype_has_accessor(proto: usize, object: usize, name: &[u8]) -> Option<bool> {
    if proto == 0 {
        return None;
    }
    match crate::value::addr_class::try_read_gc_header(proto) {
        Some(h) if h.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 => None,
        Some(h) if crate::gc::gc_type_is_object(h.obj_type) => {
            let p = proto as *const ObjectHeader;
            let linked = match super::method_site::read_holder::admitted_proto_id(p) {
                Some(super::shapes::PROTO_ID_DEFAULT) => true,
                Some(_) => {
                    let (_, word) = super::method_site::read_holder::stated_link(p);
                    word & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
                        && (word & crate::value::POINTER_MASK) as usize == object
                }
                None => false,
            };
            linked.then(|| super::key_attrs::object_key_is_accessor(p, name))
        }
        Some(h)
            if h.obj_type == crate::gc::GC_TYPE_ARRAY
                && !super::prototype_chain::array_static_proto_recorded() =>
        {
            Some(super::descriptor_state::owner_key_is_accessor(proto, name))
        }
        _ => None,
    }
}

/// [`prepare`] for a heap object whose header says object.
unsafe fn prepare_ordinary(recv: f64, addr: usize, name: &[u8]) -> f64 {
    let by_name = f64::from_bits(METHOD_SITE_BY_NAME);
    // Namespace objects, class objects, dictionaries and other exotic readers
    // keep their dispatcher; only an observable read is performed here.
    if !plain_ordinary(addr) {
        return if lookup_is_observable(recv, name) {
            spec_get(recv, name)
        } else {
            by_name
        };
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    // The one [[Get]]: a getter or a Proxy trap on the chain runs here,
    // before the arguments.
    let value = spec_get(recv_h.get_nanbox_f64(), name);
    let vb = value.to_bits();
    if vb & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr((vb & crate::value::POINTER_MASK) as usize)
    {
        return value;
    }
    // An own data property (the receiver's shape lists the key and no
    // descriptor makes it an accessor) ran nothing: a heap value is called
    // as read, anything else by name.
    let recv = recv_h.get_nanbox_f64();
    let obj = addr_of(recv) as *const ObjectHeader;
    // A classless ordinary record has no virtual class methods. Its [[Get]]
    // above is the selected value, including undefined after a delete and a
    // non-callable own primitive. Preserve it through the arguments so the
    // value-call path raises TypeError instead of trying name dispatch again.
    if (*obj).class_id == 0 {
        return value;
    }
    if own_data_key(obj, name) {
        return if vb & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
            value
        } else {
            by_name
        };
    }
    // Anything else (an inherited intrinsic, a missing method) is answered
    // by name after the arguments, with the dispatcher's own TypeError for a
    // missing one. That re-reads the key, so a read that ran code keeps the
    // value it produced.
    if lookup_is_observable(recv, name) {
        value
    } else {
        by_name
    }
}

/// A heap object whose own keys and attributes its layout (or its overflow)
/// states: not a namespace, class object, `Object.prototype`, URL, element
/// store or other exotic reader, which keep their dispatcher.
unsafe fn plain_ordinary(addr: usize) -> bool {
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    if !crate::gc::gc_type_is_object(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
    {
        return false;
    }
    let obj = addr as *const ObjectHeader;
    if !super::object_is_regular(obj)
        || (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID
        || super::class_registry::is_class_object_ptr(obj.cast())
        || crate::array::object_prototype_addr_matches(addr)
        || ((*obj).class_id == 0
            && crate::object::url_layout::is_url_object_shape(obj as *mut ObjectHeader))
    {
        return false;
    }
    let meta = (*obj).meta;
    meta.is_null()
        || ((*meta).elements == 0
            && (*meta).flags & super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER == 0)
}

fn addr_of(v: f64) -> usize {
    (v.to_bits() & crate::value::POINTER_MASK) as usize
}

/// Does the ordinary object `obj`'s shape list `name` as an own DATA key?
unsafe fn own_data_key(obj: *const ObjectHeader, name: &[u8]) -> bool {
    // An object with an overflow lists every own key, and its attributes,
    // there (object model step 4).
    let table = super::overflow::table(obj);
    if !table.is_null() {
        return super::overflow::find_bytes(table, name).is_some_and(|i| {
            super::overflow::entry_attrs(table.cast(), i) & super::key_attrs::ENTRY_ACCESSOR == 0
        });
    }
    let Some(shape) = super::shapes::object_shape_descriptor(obj) else {
        return false;
    };
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    !keys.is_null()
        && super::keys_find_slot_by_bytes_resolved(shape.keys_view(), shape.logical_key_count, name)
            .is_some_and(|slot| !super::key_attrs::key_is_accessor_at(keys, slot))
}

/// The receiver's OWN accessor for `name`, run now with the receiver as
/// `this`: the one read of a native cell the by-name dispatcher performs that
/// can run code (`native_call_method`'s accessor arm, gated alike). `None`
/// when the cell has no such getter.
unsafe fn own_getter_value(addr: usize, recv: f64, name: &[u8]) -> Option<f64> {
    let key = std::str::from_utf8(name).ok()?;
    let acc = super::descriptor_state::get_accessor_descriptor(addr, key)?;
    let getter = (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if acc.get == 0 || getter.is_null() {
        return None;
    }
    Some(crate::closure::js_closure_call0(
        getter,
        crate::closure::JsThis::from_f64(recv),
    ))
}

/// Does a primitive's chain hold `name` as an accessor? The chain is its
/// wrapper prototype, whose shape pins its own [[Prototype]], and then
/// `Object.prototype`; each shape's summary answers a prototype with no
/// accessor in one load. Symbols, bigints, an unbuilt realm and a wrapper
/// prototype that no longer inherits from the realm default take the full
/// walk.
unsafe fn primitive_chain_has_accessor(recv: f64, name: &[u8]) -> bool {
    let wrapper = crate::array::primitive_wrapper_prototype_addr(recv);
    let object = crate::array::object_prototype_addr_if_resolved();
    if object == 0 {
        return lookup_is_observable(recv, name);
    }
    match builtin_prototype_has_accessor(wrapper, object, name) {
        Some(held) => {
            held || super::key_attrs::object_key_is_accessor(object as *const ObjectHeader, name)
        }
        None => lookup_is_observable(recv, name),
    }
}

/// Call the value [`js_method_lookup`] returned with `recv` as `this`:
/// an object-literal method's baked `this` capture is rebound to the receiver
/// first (#6475), exactly as the by-name dispatcher does. Only that rebind
/// allocates, so only then are the arguments copied and rooted across it.
///
/// # Safety
/// `args_ptr` holds `argc` values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_call_value(
    func: f64,
    recv: f64,
    args_ptr: *const f64,
    argc: usize,
) -> f64 {
    // The value a getter or a trap returned is called as is: a nullish one
    // is not callable (the generic value call would answer `undefined`).
    let fb = func.to_bits();
    if fb == 0 || fb == crate::value::TAG_UNDEFINED || fb == crate::value::TAG_NULL {
        crate::closure::throw_not_callable();
    }
    let fptr = (fb & crate::value::POINTER_MASK) as usize;
    let rebinds = fb & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr(fptr)
        && crate::closure::closure_reads_this_from_capture(
            fptr as *const crate::closure::ClosureHeader,
        );
    if !rebinds {
        return crate::closure::native_call_value_this(
            func,
            crate::closure::JsThis::from_f64(recv),
            args_ptr,
            argc,
        );
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let original: Vec<f64> = if argc > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, argc).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&original);
    let bound = crate::closure::clone_closure_rebind_this(fb, recv_h.get_nanbox_f64());
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let this = recv_h.get_nanbox_f64();
    crate::closure::native_call_value_this(
        f64::from_bits(bound),
        crate::closure::JsThis::from_f64(this),
        args.as_ptr(),
        args.len(),
    )
}

/// [`js_method_call_value`] for a spread call: `args` is the array the
/// caller bundled every argument into.
///
/// # Safety
/// `args` is null or a live array.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_method_call_value_apply(func: f64, recv: f64, args: i64) -> f64 {
    let arr = args as *const crate::array::ArrayHeader;
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let buf: Vec<f64> = (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i as u32))
        .collect();
    js_method_call_value(func, recv, buf.as_ptr(), buf.len())
}

/// Can reading `name` off `recv` run code or throw? A nullish receiver throws,
/// a Proxy anywhere on the chain traps, and an accessor anywhere on the chain
/// runs its getter. Each holder shape answers whether the key is an accessor.
unsafe fn lookup_is_observable(recv: f64, name: &[u8]) -> bool {
    let bits = recv.to_bits();
    if bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL {
        return true;
    }
    let is_pointer = |b: u64| b & !crate::value::POINTER_MASK == crate::value::POINTER_TAG;
    let name_str = std::str::from_utf8(name).ok();
    let mut cur = recv;
    // A prototype chain is acyclic; the bound only guards a corrupt one.
    for _ in 0..256 {
        let b = cur.to_bits();
        if is_pointer(b) {
            let addr = (b & crate::value::POINTER_MASK) as usize;
            // A Proxy's [[Get]] is a trap: always observable.
            if crate::value::addr_class::is_proxy_cell(addr) {
                return true;
            }
            // Every cell kind answers: an ordinary object from its shape's
            // summary and keys, any other cell (an array exotic prototype, a
            // collection) from its own property holder's shape.
            if crate::value::addr_class::is_above_handle_band(addr) {
                match name_str {
                    Some(n) => {
                        if super::descriptor_state::get_accessor_descriptor(addr, n).is_some() {
                            return true;
                        }
                    }
                    None => return true,
                }
            }
        }
        // The internal read: `js_object_get_prototype_of` is the user-facing
        // one and publishes an exposed iterator prototype (#10086).
        let proto = crate::object::object_ops::get_prototype_of_resolved(cur);
        let pb = proto.to_bits();
        if pb == crate::value::TAG_NULL || pb == crate::value::TAG_UNDEFINED || pb == b {
            return false;
        }
        cur = proto;
    }
    false
}

/// [`spec_get`] for the builtin-prototype generic path
/// (`builtin_proto::redirect_call`).
pub(crate) unsafe fn spec_get_value(recv: f64, name: &[u8]) -> f64 {
    spec_get(recv, name)
}

/// The spec [[Get]] of `name` on `recv`: runs a getter (with `recv` as
/// `this`), a Proxy trap, or throws for a nullish receiver.
unsafe fn spec_get(recv: f64, name: &[u8]) -> f64 {
    // A nullish receiver throws node's TypeError before any argument runs.
    match recv.to_bits() {
        crate::value::TAG_NULL => {
            crate::error::js_throw_type_error_property_access(1, name.as_ptr(), name.len())
        }
        crate::value::TAG_UNDEFINED => {
            crate::error::js_throw_type_error_property_access(0, name.as_ptr(), name.len())
        }
        _ => {}
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    // The canonical interned key: no allocation per read.
    let key = crate::string::canonical_key(name);
    let recv_value = recv_h.get_nanbox_f64();
    let value = crate::object::js_object_get_field_by_name_f64(
        recv_value.to_bits() as usize as *const ObjectHeader,
        key,
    );
    if value.to_bits() == METHOD_SITE_BY_NAME || value.to_bits() == METHOD_SITE_BY_NAME_DIRECT {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        if let Some(inherited) = function_receiver_inherited_get(recv_value, key) {
            return inherited;
        }
    }
    value
}

/// The generic field get is not a faithful `[[Get]]` for a FUNCTION receiver:
/// `%Function.prototype%`'s method surface (`call`/`apply`/`bind`/`toString`/
/// `valueOf`/…) is answered by dedicated dispatch, so the closure arms of
/// `js_object_get_field_by_name_f64` report every one of those names absent.
/// [`spec_get`]'s caller (`builtin_proto::redirect_call`) reads that absence
/// as "not callable" and throws, where `#11915`'s predecessor fell through to
/// the by-name dispatcher — so a program that replaces
/// `Function.prototype.toString` made `fn.toString()` throw
/// `TypeError: value is not a function` (`globals/function-tostring-override`,
/// #9202).
///
/// Resolve the name off the receiver's REAL `[[Prototype]]`, the same `Get`
/// `closure::reify_function_method_value` already performs for
/// `call`/`apply`/`bind`: a replaced slot is the value the call runs, a
/// deleted one stays absent, and a detached function
/// (`Object.setPrototypeOf(fn, null)`) still finds nothing. `None` for any
/// receiver that is not a function, so no other family's path changes.
unsafe fn function_receiver_inherited_get(
    recv: f64,
    key: *const crate::string::StringHeader,
) -> Option<f64> {
    let jsval = crate::value::JSValue::from_bits(recv.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    if !crate::closure::is_closure_ptr(jsval.as_pointer::<u8>() as usize) {
        return None;
    }
    // `js_object_get_prototype_of` answers `null` for a detached function, and
    // the closure's recorded static prototype otherwise.
    let proto = crate::object::js_object_get_prototype_of(recv);
    if proto.to_bits() == crate::value::TAG_NULL {
        return None;
    }
    Some(crate::proxy::js_reflect_get(
        proto,
        crate::value::js_nanbox_string(key as i64),
        recv,
    ))
}

/// The compiler's unpatched `slice` site, after its String arm declined.
/// Arguments must be order-free and `argc <= 2`. The receiver's existing
/// Array shape proves no own method or custom prototype; every other case
/// retains the ordinary dispatcher in this one shared operation helper.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_slice_method(
    recv: f64,
    start: f64,
    end: f64,
    argc: usize,
) -> f64 {
    // #11915: the direct body only while `%Array.prototype%` is pristine.
    if is_site_receiver(recv)
        && crate::object::builtin_proto::pristine(crate::object::builtin_proto::BuiltinProto::Array)
    {
        let addr = (recv.to_bits() & crate::value::POINTER_MASK) as usize;
        if crate::value::addr_class::try_read_gc_header(addr)
            .is_some_and(|h| (*h).obj_type == crate::gc::GC_TYPE_ARRAY)
        {
            let array = crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader);
            if !array.is_null() && crate::array::array_has_plain_shape_resolved(array) {
                let result = crate::array::js_array_slice_values(array, start, end);
                return crate::value::js_nanbox_pointer(result as i64);
            }
        }
    }
    let args = [start, end];
    if !crate::object::builtin_proto::receiver_pristine(recv) {
        if let Some(value) =
            crate::object::builtin_proto::redirect_call(recv, b"slice", args.as_ptr(), argc)
        {
            return value;
        }
    }
    crate::object::js_native_call_method(recv, b"slice".as_ptr().cast(), 5, args.as_ptr(), argc)
}
