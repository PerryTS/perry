//! `Iterator.from` (§27.1.3.2.1) with `%WrapForValidIteratorPrototype%`,
//! `Iterator.concat`, and `Iterator.prototype[Symbol.dispose]`.
//!
//! A wrapper is an `OP_WRAP` helper object: its fields are the Iterator
//! Record (`F_ITERATED`, `F_NEXT`), so stepping it as a record skips the
//! wrapper's own `next` (which only forwards) while closing it still goes
//! through its `return`.

use super::{
    alloc_helper, call, field, get, is_iterator_helper_addr, is_nullish, is_object, new_helper,
    type_error, undefined, F_ITERATED, F_NEXT, F_OP, OP_CONCAT, OP_WRAP,
};
use crate::closure::{ClosureHeader, JsThis};
use crate::gc::RuntimeHandleScope;
use crate::iter_result::make_iter_result;
use crate::object::ObjectHeader;
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue};

/// An ECMAScript Object: symbols are pointer-tagged cells too.
fn is_js_object(value: f64) -> bool {
    is_object(value) && unsafe { crate::symbol::js_is_symbol(value) } == 0
}

/// `Get(value, @@iterator)`.
unsafe fn get_iterator_method(value: f64) -> f64 {
    let sym = crate::symbol::well_known_symbol("iterator");
    let key = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    crate::proxy::js_reflect_get(value, key, value)
}

/// GetIteratorFlattenable(value, ...) up to the iterator object; the caller
/// reads `next` (GetIteratorDirect).
pub(super) unsafe fn flattenable_iterator(value: f64, iterate_strings: bool) -> f64 {
    if !is_js_object(value) {
        if iterate_strings && JSValue::from_bits(value.to_bits()).is_any_string() {
            return crate::symbol::js_get_iterator(value);
        }
        type_error("Iterator.from called on non-object");
    }
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let method = get_iterator_method(value.get_nanbox_f64());
    let iter = if is_nullish(method) {
        value.get_nanbox_f64()
    } else if !crate::object::value_is_callable(method) {
        type_error("Symbol.iterator is not a function");
    } else {
        call(method, value.get_nanbox_f64(), &[])
    };
    if !is_js_object(iter) {
        type_error("Iterator is not an object");
    }
    iter
}

/// OrdinaryHasInstance(%Iterator%, value) for an object `value`.
unsafe fn inherits_iterator_prototype(value: f64) -> bool {
    let target = crate::object::iterator_ctor::iterator_prototype_object() as usize;
    let mut current = value;
    loop {
        current = crate::object::object_ops::get_prototype_of_resolved(current);
        if !is_js_object(current) {
            return false;
        }
        if js_nanbox_get_pointer(current) as usize == target {
            return true;
        }
    }
}

/// `Iterator.from(value)`: the iterator itself when it already inherits from
/// `Iterator.prototype`, else a `%WrapForValidIteratorPrototype%` wrapper.
#[no_mangle]
pub extern "C" fn js_iterator_from(value: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let iter = scope.root_nanbox_f64(flattenable_iterator(value, true));
        let next = scope.root_nanbox_f64(get(iter.get_nanbox_f64(), b"next"));
        if inherits_iterator_prototype(iter.get_nanbox_f64()) {
            return iter.get_nanbox_f64();
        }
        alloc_helper(
            OP_WRAP,
            iter.get_nanbox_f64(),
            undefined(),
            next.get_nanbox_f64(),
        )
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ITERATOR_FROM: extern "C" fn(f64) -> f64 = js_iterator_from;

/// Is `value` an `Iterator.from` wrapper (has `[[Iterated]]`)?
pub(super) unsafe fn is_wrapper(value: f64) -> bool {
    let p = js_nanbox_get_pointer(value) as usize;
    is_object(value) && is_iterator_helper_addr(p) && field(value, F_OP) as i32 == OP_WRAP
}

unsafe fn wrapper_receiver(this: JsThis, method: &str) -> f64 {
    let this = this.as_f64();
    if !is_wrapper(this) {
        type_error(&format!(
            "Method %WrapForValidIteratorPrototype%.{method} called on incompatible receiver"
        ));
    }
    this
}

/// `%WrapForValidIteratorPrototype%.next()`: the wrapped `next`'s result,
/// unchanged.
extern "C" fn wrap_next_thunk(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let this = wrapper_receiver(this, "next");
        call(field(this, F_NEXT), field(this, F_ITERATED), &[])
    }
}

/// `%WrapForValidIteratorPrototype%.return()`.
extern "C" fn wrap_return_thunk(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let iter = scope.root_nanbox_f64(field(wrapper_receiver(this, "return"), F_ITERATED));
        let ret = get(iter.get_nanbox_f64(), b"return");
        if is_nullish(ret) {
            return make_iter_result(JSValue::undefined(), true);
        }
        if !crate::object::value_is_callable(ret) {
            type_error("Iterator return method is not a function");
        }
        call(ret, iter.get_nanbox_f64(), &[])
    }
}

pub(super) fn wrap_next_code() -> *const u8 {
    wrap_next_thunk as *const u8
}

/// `next` and `return` of a newly built `%WrapForValidIteratorPrototype%`.
pub(crate) fn install_wrap_prototype_methods(proto: *mut ObjectHeader) {
    crate::object::global_this::install_proto_method(
        proto,
        "next",
        crate::fn_info!(wrap_next_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
    crate::object::global_this::install_proto_method(
        proto,
        "return",
        crate::fn_info!(wrap_return_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
}

/// `Iterator.prototype[Symbol.dispose]()`: calls `this.return` if present.
extern "C" fn iterator_dispose_thunk(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        let ret = get(this.get_nanbox_f64(), b"return");
        if !is_nullish(ret) {
            if !crate::object::value_is_callable(ret) {
                type_error("Iterator return method is not a function");
            }
            call(ret, this.get_nanbox_f64(), &[]);
        }
        undefined()
    }
}

/// Install `[Symbol.dispose]` on a newly built `%Iterator.prototype%`.
pub(crate) fn install_dispose(proto: *mut ObjectHeader) {
    let sym = crate::symbol::well_known_symbol("dispose");
    let closure = crate::closure::js_closure_alloc(
        crate::fn_info!(iterator_dispose_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
    if sym.is_null() || closure.is_null() {
        return;
    }
    crate::object::native_module::set_bound_native_closure_metadata(closure, "[Symbol.dispose]", 0);
    crate::object::native_module::set_builtin_closure_non_constructable(closure as usize);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            js_nanbox_pointer(proto as i64),
            f64::from_bits(JSValue::pointer(sym as *const u8).bits()),
            js_nanbox_pointer(closure as i64),
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto as usize,
        sym as usize,
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

/// `Iterator.concat(...items)`: every item is validated before any is
/// iterated. The helper's `F_ARG` holds `[item0, method0, item1, ...]`.
unsafe extern "C" fn concat_thunk(
    _closure: *const ClosureHeader,
    _this: JsThis,
    args: *const f64,
    len: usize,
) -> f64 {
    let args = if args.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(args, len)
    };
    let scope = RuntimeHandleScope::new();
    let items = scope.root_nanbox_f64_slice(args);
    let list = scope.root_raw_mut_ptr(crate::array::js_array_alloc((len * 2) as u32));
    for item in &items {
        if !is_js_object(item.get_nanbox_f64()) {
            type_error("Iterator.concat called on non-object");
        }
        let method = get_iterator_method(item.get_nanbox_f64());
        if !crate::object::value_is_callable(method) {
            type_error("Iterator.concat item is not iterable");
        }
        let method = scope.root_nanbox_f64(method);
        for value in [item.get_nanbox_f64(), method.get_nanbox_f64()] {
            list.set_raw_mut_ptr(crate::array::js_array_push_f64(
                list.get_raw_mut_ptr(),
                value,
            ));
        }
    }
    let list = js_nanbox_pointer(list.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64);
    new_helper(OP_CONCAT, undefined(), list)
}

extern "C" fn iterator_from_thunk(
    _closure: *const ClosureHeader,
    _this: JsThis,
    value: f64,
) -> f64 {
    js_iterator_from(value)
}

/// `Iterator.from` and `Iterator.concat` on the `Iterator` constructor.
pub(crate) fn install_iterator_statics(s: &mut crate::object::global_this::ConstructorStatics) {
    s.method(
        "from",
        crate::fn_info!(iterator_from_thunk, 1; with_declared(1)),
        1,
    );
    s.method(
        "concat",
        crate::fn_info!(native_args concat_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
}
