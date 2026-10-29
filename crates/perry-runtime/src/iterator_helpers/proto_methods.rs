//! The helper methods of `%Iterator.prototype%` (ES2025 §27.1.4.2-12) as
//! real builtin functions, and the method-call fast path that runs them only
//! when the receiver would resolve the builtin itself.

use super::{
    call, call_back, close_normal, close_quietly, drain_to_array, get, get_iterator_direct,
    helper_step, is_object, new_helper, range_error, truthy, type_error, undefined, OP_DROP,
    OP_FILTER, OP_FLATMAP, OP_MAP, OP_TAKE,
};
use crate::closure::{is_closure_ptr, ClosureHeader, JsThis};
use crate::gc::RuntimeHandleScope;
use crate::object::ObjectHeader;
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_FALSE, TAG_TRUE};
use std::cell::Cell;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Method {
    Map,
    Filter,
    Take,
    Drop,
    FlatMap,
    Reduce,
    ToArray,
    ForEach,
    Some,
    Every,
    Find,
}

const METHODS: [Method; 11] = [
    Method::Map,
    Method::Filter,
    Method::Take,
    Method::Drop,
    Method::FlatMap,
    Method::Reduce,
    Method::ToArray,
    Method::ForEach,
    Method::Some,
    Method::Every,
    Method::Find,
];

macro_rules! method_thunks {
    ($($method:ident => $thunk:ident),* $(,)?) => {
        $(
            unsafe extern "C" fn $thunk(
                _closure: *const ClosureHeader,
                this: JsThis,
                args: *const f64,
                len: usize,
            ) -> f64 {
                let args = if args.is_null() || len == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(args, len)
                };
                run(Method::$method, this.as_f64(), args)
            }
        )*

        impl Method {
            fn code(self) -> *const u8 {
                match self {
                    $(Method::$method => $thunk as *const u8,)*
                }
            }

            fn info(self) -> *const crate::closure::JsFunctionInfo {
                match self {
                    $(Method::$method => crate::fn_info!(
                        native_args $thunk, 1; with_flags(crate::closure::FN_BUILTIN)
                    ),)*
                }
            }
        }
    };
}

method_thunks!(
    Map => map_thunk,
    Filter => filter_thunk,
    Take => take_thunk,
    Drop => drop_thunk,
    FlatMap => flat_map_thunk,
    Reduce => reduce_thunk,
    ToArray => to_array_thunk,
    ForEach => for_each_thunk,
    Some => some_thunk,
    Every => every_thunk,
    Find => find_thunk,
);

impl Method {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        METHODS.into_iter().find(|m| m.name() == name)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Method::Map => "map",
            Method::Filter => "filter",
            Method::Take => "take",
            Method::Drop => "drop",
            Method::FlatMap => "flatMap",
            Method::Reduce => "reduce",
            Method::ToArray => "toArray",
            Method::ForEach => "forEach",
            Method::Some => "some",
            Method::Every => "every",
            Method::Find => "find",
        }
    }

    fn length(self) -> u32 {
        (self != Method::ToArray) as u32
    }

    /// Is `value` this method's builtin function object?
    pub(crate) unsafe fn is_builtin(self, value: f64) -> bool {
        let p = js_nanbox_get_pointer(value) as *const ClosureHeader;
        JSValue::from_bits(value.to_bits()).is_pointer()
            && is_closure_ptr(p as usize)
            && (*p).code() == self.code()
    }
}

/// Install the helper methods on a newly built `%Iterator.prototype%`, and
/// `return` on `%IteratorHelperPrototype%`. Part of the tower's birth keys:
/// every tower gets the same keys, none of them a protocol method.
pub(crate) fn install_prototype_methods(
    iterator_proto: *mut ObjectHeader,
    helper_proto: *mut ObjectHeader,
) {
    for m in METHODS {
        crate::object::global_this::install_proto_method(
            iterator_proto,
            m.name(),
            m.info(),
            m.length(),
        );
    }
    crate::object::global_this::install_proto_method(
        helper_proto,
        "return",
        crate::fn_info!(helper_return_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
        0,
    );
}

extern "C" fn helper_return_thunk(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    let this = this.as_f64();
    let p = js_nanbox_get_pointer(this) as usize;
    if !is_object(this) || !super::is_iterator_helper_addr(p) {
        type_error("Iterator Helper.prototype.return called on incompatible receiver");
    }
    unsafe { super::helper_return(p as *mut ObjectHeader) }
}

/// `Iterator.prototype.<m>` called with `this` and `args`.
unsafe fn run(m: Method, this: f64, args: &[f64]) -> f64 {
    if !is_object(this) {
        type_error(&format!(
            "Iterator.prototype.{} called on non-object",
            m.name()
        ));
    }
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let arg0 = scope.root_nanbox_f64(args.first().copied().unwrap_or_else(undefined));
    let initial = args.get(1).map(|v| scope.root_nanbox_f64(*v));
    match m {
        Method::ToArray => drain_to_array(get_iterator_direct(this.get_nanbox_f64())),
        Method::Take | Method::Drop => {
            let limit = to_limit(this.get_nanbox_f64(), arg0.get_nanbox_f64());
            let op = if m == Method::Take { OP_TAKE } else { OP_DROP };
            new_helper(op, get_iterator_direct(this.get_nanbox_f64()), limit)
        }
        Method::Map | Method::Filter | Method::FlatMap => {
            require_callable(this.get_nanbox_f64(), arg0.get_nanbox_f64());
            let op = match m {
                Method::Map => OP_MAP,
                Method::Filter => OP_FILTER,
                _ => OP_FLATMAP,
            };
            let iterated = get_iterator_direct(this.get_nanbox_f64());
            new_helper(op, iterated, arg0.get_nanbox_f64())
        }
        _ => {
            require_callable(this.get_nanbox_f64(), arg0.get_nanbox_f64());
            let iterated = scope.root_nanbox_f64(get_iterator_direct(this.get_nanbox_f64()));
            let initial = initial.map(|v| v.get_nanbox_f64());
            terminal(m, &iterated, arg0.get_nanbox_f64(), initial)
        }
    }
}

/// A non-callable callback is a TypeError after closing `this` (the
/// argument-validation step closes the underlying iterator).
unsafe fn require_callable(this: f64, callback: f64) {
    if !crate::object::value_is_callable(callback) {
        close_this_quietly(this);
        type_error("Iterator helper callback is not a function");
    }
}

/// IteratorClose on the not-yet-direct `this` record.
unsafe fn close_this_quietly(this: f64) {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let _ = crate::exception::catch_js_throw(|| {
        let ret = get(this.get_nanbox_f64(), b"return");
        if crate::object::value_is_callable(ret) {
            call(ret, this.get_nanbox_f64(), &[]);
        }
    });
}

/// take/drop limit: ToNumber, NaN or negative is a RangeError, Infinity is
/// allowed; every failure closes `this` first.
unsafe fn to_limit(this: f64, limit: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let n = match crate::exception::catch_js_throw(|| crate::builtins::js_number_coerce(limit)) {
        Ok(n) => n,
        Err(error) => {
            let error = scope.root_nanbox_f64(error);
            close_this_quietly(this.get_nanbox_f64());
            crate::exception::js_throw(error.get_nanbox_f64());
        }
    };
    let n = n.trunc() + 0.0;
    if n.is_nan() || n < 0.0 {
        close_this_quietly(this.get_nanbox_f64());
        range_error("Iterator limit must be a non-negative number");
    }
    n
}

/// reduce/forEach/some/every/find over `iterated`. A callback's throw closes
/// the iterator; a short-circuit closes it normally.
unsafe fn terminal(
    m: Method,
    iterated: &crate::gc::RuntimeHandle<'_>,
    callback: f64,
    initial: Option<f64>,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(callback);
    let it = || iterated.get_nanbox_f64();
    let close = Cell::new(false);
    let result = crate::exception::catch_js_throw(|| {
        let mut counter = 0.0;
        let acc = scope.root_nanbox_f64(undefined());
        if m == Method::Reduce {
            match initial {
                Some(v) => acc.set_nanbox_f64(v),
                None => {
                    let Some(first) = helper_step(it()) else {
                        type_error("Reduce of empty iterator with no initial value");
                    };
                    acc.set_nanbox_f64(first);
                    counter = 1.0;
                }
            }
        }
        loop {
            close.set(false);
            let Some(value) = helper_step(it()) else {
                return match m {
                    Method::Reduce => acc.get_nanbox_f64(),
                    Method::Some => f64::from_bits(TAG_FALSE),
                    Method::Every => f64::from_bits(TAG_TRUE),
                    _ => undefined(),
                };
            };
            close.set(true);
            let step_scope = RuntimeHandleScope::new();
            let value = step_scope.root_nanbox_f64(value);
            let args = [value.get_nanbox_f64(), counter];
            counter += 1.0;
            if m == Method::Reduce {
                let next = [acc.get_nanbox_f64(), args[0], args[1]];
                acc.set_nanbox_f64(call_back(callback.get_nanbox_f64(), &next));
                continue;
            }
            let answer = call_back(callback.get_nanbox_f64(), &args);
            let stop = match m {
                Method::Some | Method::Find => truthy(answer),
                Method::Every => !truthy(answer),
                _ => false,
            };
            if stop {
                close.set(false);
                close_normal(it());
                return match m {
                    Method::Some => f64::from_bits(TAG_TRUE),
                    Method::Every => f64::from_bits(TAG_FALSE),
                    _ => value.get_nanbox_f64(),
                };
            }
        }
    });
    match result {
        Ok(value) => value,
        Err(error) => {
            let error = scope.root_nanbox_f64(error);
            if close.get() {
                close_quietly(it());
            }
            crate::exception::js_throw(error.get_nanbox_f64())
        }
    }
}

/// Is `name` one of the iterator-helper method names?
pub fn is_iterator_helper_method(name: &str) -> bool {
    Method::from_name(name).is_some()
}

/// A method call `receiver.<name>(args)` for a helper name. Runs the builtin
/// only when `receiver[name]` IS the builtin, so overrides on the instance,
/// a subclass or `Iterator.prototype` are honoured. A different callable is
/// called when `claim_overrides` (the receiver's class-id dispatcher would
/// not find it) and otherwise left to the caller (`None`).
unsafe fn call_resolved(
    receiver: f64,
    m: Method,
    args_ptr: *const f64,
    args_len: usize,
    claim_overrides: bool,
) -> Option<f64> {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let args = scope.root_nanbox_f64_slice(if args_ptr.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    });
    let name = m.name().as_bytes();
    let method = scope.root_nanbox_f64(get(receiver.get_nanbox_f64(), name));
    let args = RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    if m.is_builtin(method.get_nanbox_f64()) {
        return Some(run(m, receiver.get_nanbox_f64(), &args));
    }
    if claim_overrides && crate::object::value_is_callable(method.get_nanbox_f64()) {
        return Some(call(
            method.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
            &args,
        ));
    }
    None
}

/// #2874: a helper name called on an ordinary object that inherits it (a
/// generator, an `Iterator` subclass instance). `has_own_field` keeps the
/// caller's own-method precedence. `None` falls through to ordinary method lookup.
pub unsafe fn maybe_dispatch_helper_on_iterator(
    obj: *mut ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
    has_own_field: bool,
) -> Option<f64> {
    if has_own_field {
        return None;
    }
    let m = Method::from_name(method_name)?;
    call_resolved(js_nanbox_pointer(obj as i64), m, args_ptr, args_len, false)
}

/// #9068: a helper name called on a runtime builtin iterator (array, Map,
/// Set, String, RegExp-string, helper), whose class-id dispatchers only
/// implement the protocol methods.
pub unsafe fn maybe_dispatch_helper_on_builtin_iterator(
    obj: *mut ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let m = Method::from_name(method_name)?;
    call_resolved(js_nanbox_pointer(obj as i64), m, args_ptr, args_len, true)
}

/// Dispatch a method call on a helper iterator object. `args_ptr`/`args_len`
/// carry the NaN-boxed call arguments.
pub unsafe fn dispatch_iterator_helper_method(
    obj: *mut ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    match method_name {
        // #9019: an own `next` assigned onto the helper instance wins over
        // the builtin advance, exactly as on the Map/Set path.
        "next" => {
            if let Some(result) =
                crate::object::call_overridden_iterator_next(obj, super::ITERATOR_HELPER_CLASS_ID)
            {
                return result;
            }
            super::helper_next(obj)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(obj as i64),
        "return" => super::helper_return(obj),
        "throw" => crate::iter_result::make_iter_result(JSValue::undefined(), true),
        _ => match Method::from_name(method_name) {
            Some(m) => call_resolved(js_nanbox_pointer(obj as i64), m, args_ptr, args_len, true)
                .unwrap_or_else(|| type_error(&format!("{method_name} is not a function"))),
            None => undefined(),
        },
    }
}
