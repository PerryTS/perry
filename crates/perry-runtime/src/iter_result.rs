//! Runtime iterator results from declared layouts, with source key order.
use crate::value::JSValue;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IterResultOrder {
    ValueDone = 0,
    DoneValue = 1,
}

/// Pointer-bearing members remain rooted across the sole object allocation.
pub unsafe fn build_iter_result_ordered(
    first: JSValue,
    second: JSValue,
    order: IterResultOrder,
) -> f64 {
    let desc = match order {
        IterResultOrder::ValueDone => {
            std::ptr::addr_of!(crate::object::runtime_layouts::ITERATOR_RESULT)
        }
        IterResultOrder::DoneValue => {
            std::ptr::addr_of!(crate::object::runtime_layouts::ITERATOR_RESULT_DONE_FIRST)
        }
    };
    // Scalar members (numbers, booleans, undefined) are not managed, so only
    // a pointer-bearing member needs a root across the sole allocation.
    if crate::gc::barrier_scalar_child_skips(first.bits())
        && crate::gc::barrier_scalar_child_skips(second.bits())
    {
        let object = crate::object::layout_desc::perry_alloc_layout(desc);
        return init_iter_result(object, first.bits(), second.bits());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let first = scope.root_nanbox_u64(first.bits());
    let second = scope.root_nanbox_u64(second.bits());
    let object = crate::object::layout_desc::perry_alloc_layout(desc);
    init_iter_result(object, first.get_nanbox_u64(), second.get_nanbox_u64())
}

/// Initialize a just-born, unpublished ITERATOR_RESULT layout object.
#[inline(always)]
unsafe fn init_iter_result(
    object: *mut crate::object::ObjectHeader,
    first: u64,
    second: u64,
) -> f64 {
    // The result is born here and unpublished: its declared layout is REP_ANY
    // with default attributes and no overflow, so both slots take the plain
    // fresh-object store (string alias demotion + barrier) without the
    // general Set's overflow, representation and attribute re-derivation.
    let fields =
        crate::meta_slots_of!(crate::meta_obj_runtime_born!(object as *mut _)).cast::<u64>();
    for (index, bits) in [first, second].into_iter().enumerate() {
        crate::gc::runtime_store_object_jsvalue_slot(
            object as usize,
            fields.add(index) as usize,
            index,
            bits,
        );
    }
    crate::js_nanbox_pointer(object as i64)
}

#[inline]
pub unsafe fn make_iter_result(value: JSValue, done: bool) -> f64 {
    build_iter_result_ordered(value, JSValue::bool(done), IterResultOrder::ValueDone)
}

#[inline]
pub unsafe fn make_sqlite_iter_result(value: JSValue, done: bool) -> f64 {
    build_iter_result_ordered(JSValue::bool(done), value, IterResultOrder::DoneValue)
}

/// Where the single built-in advance algorithm delivers its result.
/// Manual next calls materialize the public object; compiled consumers own
/// a native pair that never escapes into JavaScript.
#[derive(Clone, Copy)]
pub(crate) enum IterResultTarget {
    Object,
    Step(*mut IteratorStep),
}

pub(crate) struct IteratorStep {
    pub value: f64,
    pub done: bool,
}

pub(crate) unsafe fn emit_iter_result(
    target: IterResultTarget,
    order: IterResultOrder,
    value: JSValue,
    done: bool,
) -> f64 {
    match target {
        IterResultTarget::Object => {
            let (first, second) = match order {
                IterResultOrder::ValueDone => (value, JSValue::bool(done)),
                IterResultOrder::DoneValue => (JSValue::bool(done), value),
            };
            build_iter_result_ordered(first, second, order)
        }
        IterResultTarget::Step(out) => {
            // GC_STORE_AUDIT(STACK): native result sink is a Rust caller-frame local.
            (*out).value = f64::from_bits(value.bits());
            // GC_STORE_AUDIT(POINTER_FREE): the done member is a native boolean.
            (*out).done = done;
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    }
}
