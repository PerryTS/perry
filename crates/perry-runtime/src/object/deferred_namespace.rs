//! A deferred namespace keeps its identity while forwarding export operations
//! to the module's ordinary namespace. The code address is a private slot,
//! not a heap pointer cache; the object and its slot follow normal GC tracing.
use super::*;

pub(crate) const DEFERRED_NAMESPACE_CLASS_ID: u32 = 0xFFFF_4446;
const EVALUATE_SLOT: &[u8] = b"[[DeferredModuleEvaluate]]";

#[inline]
pub(crate) fn is_deferred(value: f64) -> bool {
    let bits = value.to_bits();
    let Some(header) = crate::value::addr_class::pointer_value_header(bits) else {
        return false;
    };
    if !crate::gc_type_is_object!(header.obj_type) {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    unsafe { (*obj).class_id == DEFERRED_NAMESPACE_CLASS_ID }
}

/// Invoke the module-owned evaluator. Generated callbacks reuse its existing
/// init guard, failure cache and namespace root; this object owns no loader.
unsafe fn evaluate(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let obj = JSValue::from_bits(receiver.get_nanbox_u64()).as_pointer::<ObjectHeader>();
    let keys = object_keys(obj);
    let pos = keys_find_private_slot_by_bytes(keys.arr(), keys.count(), EVALUATE_SLOT)
        .expect("deferred namespace evaluator slot");
    let address = key_attrs::private_element_word(obj, keys.storage_slot(pos))
        .expect("deferred namespace evaluator value");
    // NOT-A-JS-BODY: this is the compiler's own `double ()` module evaluator,
    // which takes no callee and no receiver, so it is not a JS function body.
    let callback: extern "C" fn() -> f64 = std::mem::transmute(address as usize);
    callback()
}

/// For an exotic receiver normalize the key exactly once, then run the ordinary
/// operation on either its evaluated namespace or its symbol-like own surface.
/// Both operands remain rooted across module evaluation and the operation.
pub(crate) fn with_key_target<R>(
    receiver: f64,
    key: f64,
    operation: impl FnOnce(f64, f64) -> R,
) -> Option<R> {
    if !is_deferred(receiver) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_nanbox_f64(key);
    let property_key = scope.root_nanbox_f64(unsafe { js_to_property_key(key.get_nanbox_f64()) });
    let symbol_like = unsafe { crate::symbol::js_is_symbol(property_key.get_nanbox_f64()) } != 0
        || {
            let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
            (unsafe {
                crate::string::js_string_key_bytes(
                    JSValue::from_bits(property_key.get_nanbox_u64()),
                    &mut sso,
                )
            }) == Some(b"then".as_slice())
        };
    let target = scope.root_nanbox_f64(if symbol_like {
        receiver.get_nanbox_f64()
    } else {
        unsafe { evaluate(receiver.get_nanbox_f64()) }
    });
    Some(operation(
        target.get_nanbox_f64(),
        property_key.get_nanbox_f64(),
    ))
}

/// OwnPropertyKeys always evaluates, even when the caller only wants symbols.
pub(crate) fn with_keys_target<R>(receiver: f64, operation: impl FnOnce(f64) -> R) -> Option<R> {
    if !is_deferred(receiver) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(unsafe { evaluate(receiver) });
    Some(operation(target.get_nanbox_f64()))
}

/// EnumerableOwnProperties skips `then`: it occurs in the export name list,
/// but a deferred namespace never exposes a string-keyed descriptor for it.
pub(crate) fn enumerable_keys(receiver: f64) -> Option<*mut crate::ArrayHeader> {
    with_keys_target(receiver, |target| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let keys = scope.root_raw_mut_ptr(js_object_keys_value(target));
        let count = crate::array::js_array_length(keys.get_raw_mut_ptr::<crate::ArrayHeader>());
        let output = scope.root_raw_mut_ptr(crate::array::js_array_alloc(count));
        for index in 0..count {
            let value =
                crate::array::js_array_get(keys.get_raw_mut_ptr::<crate::ArrayHeader>(), index);
            let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
            if unsafe { crate::string::js_string_key_bytes(value, &mut sso) }
                == Some(b"then".as_slice())
            {
                continue;
            }
            let next = output.with_mut_ptr(|array: *mut crate::ArrayHeader| {
                crate::array::js_array_push_f64(array, f64::from_bits(value.bits()))
            });
            output.set_raw_mut_ptr(next);
        }
        output.get_raw_mut_ptr::<crate::ArrayHeader>()
    })
}

/// Create an unevaluated namespace. Its callback is immutable native code and
/// returned namespace is held by the module, so neither requires a side table.
#[no_mangle]
pub extern "C" fn js_create_deferred_namespace(evaluate: i64) -> f64 {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(js_object_alloc_null_proto(0, 1));
        obj.with_mut_ptr(|obj: *mut ObjectHeader| {
            key_attrs::apply_edits(obj, &[key_attrs::AttrsEdit::Private(EVALUATE_SLOT)]);
        });
        let current = obj.get_raw_mut_ptr::<ObjectHeader>();
        let keys = object_keys(current);
        let pos = keys_find_private_slot_by_bytes(keys.arr(), keys.count(), EVALUATE_SLOT)
            .expect("new evaluator private slot");
        let slot = keys.storage_slot(pos);
        assert!(slot < object_live_slot_count(current));
        let data = (current as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        // GC_STORE_AUDIT(INIT): raw code address has no JS pointer tag.
        *data.add(slot as usize) = evaluate as u64;
        let tag = scope.root_string_ptr(crate::string::js_string_from_bytes(
            b"Deferred Module".as_ptr(),
            15,
        ));
        let symbol = crate::symbol::well_known_symbol("toStringTag");
        let symbol_value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(symbol as i64));
        let receiver =
            || crate::value::js_nanbox_pointer(obj.get_raw_mut_ptr::<ObjectHeader>() as i64);
        crate::symbol::define_symbol_data_property(
            receiver(),
            symbol_value.get_nanbox_f64(),
            tag.with_const_ptr(|p: *const crate::StringHeader| {
                crate::value::js_nanbox_string(p as i64)
            }),
        );
        crate::symbol::set_symbol_property_attrs(
            obj.get_raw_mut_ptr::<ObjectHeader>() as usize,
            (symbol_value.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            PropertyAttrs::new(false, false, false),
        );
        // Property installation can restamp a birth's shape. Apply the
        // canonical namespace's null-prototype/non-extensibility invariants
        // only after all private and symbol slots have been installed.
        js_finalize_namespace(receiver());
        (*obj.get_raw_mut_ptr::<ObjectHeader>()).class_id = DEFERRED_NAMESPACE_CLASS_ID;
        // Existing exotic-read marking invalidates shape-only method and
        // prototype memos, not just the generic property probes.
        super::proto_validity::mark_exotic_read_receiver(
            obj.get_raw_mut_ptr::<ObjectHeader>() as usize
        );
        // Exotic-read marking moves prototype authority into metadata. Null
        // has no shape identity word to copy, so publish its explicit link
        // through the existing prototype funnel after that transition.
        super::prototype_chain::object_set_static_prototype(
            obj.get_raw_mut_ptr::<ObjectHeader>() as usize,
            crate::value::TAG_NULL,
        );
        receiver()
    }
}

/// Deferred access cannot observe a module still executing its synchronous body.
#[no_mangle]
pub extern "C" fn js_deferred_namespace_not_ready() -> f64 {
    super::property_ops::throw_type_error("Deferred module is not ready for synchronous evaluation")
}

crate::perry_thread_local! {
    // Only compiler-owned guard addresses for the current pure readiness walk.
    // No JS heap pointers or persistent module state are cached here.
    static READY_STACK: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Cycle-safe ReadyForSyncExecution over generated, non-evaluating callbacks.
///
/// # Safety
/// guard is a live generated per-heap byte and dependencies is an extern C
/// function returning i32, containing only calls to this readiness protocol.
#[no_mangle]
pub unsafe extern "C" fn js_deferred_module_ready(
    guard: i64,
    dependencies: i64,
    has_tla: i32,
) -> i32 {
    // A zero guard means the program has no per-module init byte to inspect.
    match if guard == 0 { 0 } else { *(guard as *const u8) } {
        2 => return 1,
        1 => return 0,
        _ if has_tla != 0 => return 0,
        _ => {}
    }
    let entered = READY_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if stack.contains(&(guard as usize)) {
            return false;
        }
        stack.push(guard as usize);
        true
    });
    if !entered {
        return 1;
    }
    let callback: extern "C" fn() -> i32 = std::mem::transmute(dependencies as usize);
    let ready = callback();
    READY_STACK.with(|stack| {
        let popped = stack.borrow_mut().pop();
        debug_assert_eq!(popped, Some(guard as usize));
    });
    ready
}
