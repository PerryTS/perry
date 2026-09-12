//! RegExpExec: observable method lookup, JS overrides and the one builtin
//! Perex matcher. Never retain a subject/program view across user code.
use super::perex_api as api;
use super::perex_memory::MemoryBudget;
use super::perex_runtime::{self as host, EngineError};
use super::RegExpHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, js_nanbox_string, TAG_NULL};
use perex::Budget;

pub(crate) enum ExecResult {
    Builtin(api::ExecMatch),
    Override(f64),
}

impl ExecResult {
    /// Factory return: the caller must root this before any collecting action.
    /// The builtin result must have been requested with materialize=true.
    pub(crate) fn object(self) -> f64 {
        match self {
            Self::Builtin(result) => {
                assert!(!result.array.is_null());
                js_nanbox_pointer(result.array as i64)
            }
            Self::Override(value) => value,
        }
    }
}

pub(crate) fn require_object(value: f64) -> Result<(), EngineError> {
    if crate::proxy::reflect_value_is_object(value) {
        Ok(())
    } else {
        Err(EngineError::Type("RegExp operation requires an object"))
    }
}

pub(super) fn is_regexp(value: &RuntimeHandle<'_>) -> Result<bool, EngineError> {
    if !crate::proxy::reflect_value_is_object(value.get_nanbox_f64()) {
        return Ok(false);
    }
    let marker = get_symbol(value, "match")?;
    if marker.to_bits() != crate::value::TAG_UNDEFINED {
        return Ok(crate::value::js_is_truthy(marker) != 0);
    }
    Ok(super::is_registered_regex(
        crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize,
    ))
}

pub(crate) fn get(owner: &RuntimeHandle<'_>, name: &[u8]) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let result = api::caught(|| {
        let key = crate::string::canonical_key(name);
        let value = owner.get_nanbox_f64();
        crate::proxy::js_reflect_get(value, js_nanbox_string(key as i64), value)
    });
    // Getter dispatch can throw through its own normal this-restoration path.
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    result
}

pub(crate) fn call_one(
    method: &RuntimeHandle<'_>,
    receiver: &RuntimeHandle<'_>,
    argument: &RuntimeHandle<'_>,
) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let result = api::caught(|| {
        crate::object::js_implicit_this_set(receiver.get_nanbox_f64());
        if crate::proxy::js_proxy_is_proxy(method.get_nanbox_f64()) == 1 {
            // The generic value-call bridge drops this for proxies. Supply
            // the actual receiver and an exact one-element GC argument array.
            let args = scope.root_raw_mut_ptr(crate::array::js_array_alloc(1));
            let grown =
                crate::array::js_array_push_f64(args.get_raw_mut_ptr(), argument.get_nanbox_f64());
            args.set_raw_mut_ptr(grown);
            crate::proxy::js_proxy_apply(
                method.get_nanbox_f64(),
                receiver.get_nanbox_f64(),
                js_nanbox_pointer(args.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64),
            )
        } else {
            let args = [argument.get_nanbox_f64()];
            unsafe {
                crate::closure::js_native_call_value(method.get_nanbox_f64(), args.as_ptr(), 1)
            }
        }
    });
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    result
}

/// RegExpExec with operation-owned limits. Lookup happens on every iteration;
/// a callback may replace exec or recompile the receiver before the next one.
/// Only the known builtin may omit materialization for a boolean test.
pub(crate) fn execute(
    receiver: &RuntimeHandle<'_>,
    input: &RuntimeHandle<'_>,
    materialize: bool,
    budget: &mut Budget,
    memory: &MemoryBudget,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Option<ExecResult>, EngineError> {
    host::charge(budget, 1)?;
    require_object(receiver.get_nanbox_f64())?;
    crate::string::js_string_addref(input.get_raw_mut_ptr::<StringHeader>());
    let scope = RuntimeHandleScope::new();
    let method = scope.root_nanbox_f64(get(receiver, b"exec")?);
    let callable = crate::proxy::proxy_wraps_callable(method.get_nanbox_f64());
    let builtin =
        crate::object::regex_proto_thunks::is_builtin_regexp_exec(method.get_nanbox_f64());
    if callable && !builtin {
        let argument = scope.root_nanbox_f64(js_nanbox_string(
            input.get_raw_const_ptr::<StringHeader>() as i64,
        ));
        let value = call_one(&method, receiver, &argument)?;
        if value.to_bits() == TAG_NULL {
            return Ok(None);
        }
        if !crate::proxy::reflect_value_is_object(value) {
            return Err(EngineError::Type(
                "RegExp exec method must return an object or null",
            ));
        }
        return Ok(Some(ExecResult::Override(value)));
    }
    let re = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as *mut RegExpHeader;
    if !super::is_valid_regex_ptr(re) {
        return Err(EngineError::Type(
            "RegExp builtin exec requires a RegExp receiver",
        ));
    }
    api::execute_with_resources(
        re,
        input.get_raw_const_ptr(),
        materialize,
        budget,
        memory,
        poll,
    )
    .map(|result| result.map(ExecResult::Builtin))
}

pub(crate) fn to_string(value: &RuntimeHandle<'_>) -> Result<*mut StringHeader, EngineError> {
    let scope = RuntimeHandleScope::new();
    let primitive = if crate::proxy::reflect_value_is_object(value.get_nanbox_f64()) {
        scope.root_nanbox_f64(to_primitive(value, true)?)
    } else {
        scope.root_nanbox_f64(value.get_nanbox_f64())
    };
    // Abstract ToString rejects Symbols, including the result of an object's
    // conversion. The String constructor's explicit Symbol display is separate.
    if unsafe { crate::symbol::js_is_symbol(primitive.get_nanbox_f64()) } != 0 {
        return Err(EngineError::Type(
            "Cannot convert a Symbol value to a string",
        ));
    }
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let result =
        api::caught(|| crate::value::js_jsvalue_to_string_coerce(primitive.get_nanbox_f64()));
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    let string = result?;
    crate::string::js_string_addref(string);
    Ok(string)
}

pub(crate) fn get_symbol(owner: &RuntimeHandle<'_>, name: &str) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let result = api::caught(|| {
        let key = crate::symbol::well_known_symbol(name);
        let value = owner.get_nanbox_f64();
        crate::proxy::js_reflect_get(value, js_nanbox_pointer(key as i64), value)
    });
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    result
}

pub(crate) fn set_last_index(owner: &RuntimeHandle<'_>, value: f64) -> Result<(), EngineError> {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let re = crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as *mut RegExpHeader;
    if super::is_valid_regex_ptr(re) {
        if crate::object::get_property_attrs(re as usize, "lastIndex")
            .is_some_and(|a| !a.writable())
        {
            return Err(EngineError::Type(
                "Cannot assign to read only property 'lastIndex' of object",
            ));
        }
        super::js_regexp_set_last_index(re, value.get_nanbox_f64());
        return Ok(());
    }
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let result = api::caught(|| {
        let key = crate::string::canonical_key(b"lastIndex");
        crate::proxy::js_reflect_set(
            owner.get_nanbox_f64(),
            js_nanbox_string(key as i64),
            value.get_nanbox_f64(),
            owner.get_nanbox_f64(),
        )
    });
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    if crate::value::js_is_truthy(result?) != 0 {
        Ok(())
    } else {
        Err(EngineError::Type("Cannot set RegExp lastIndex"))
    }
}

/// Abstract ToNumber, including object conversion with the number hint.
/// Number(bigint) is intentionally allowed by Perry's Number constructor;
/// limits and lastIndex use this stricter abstract operation instead.
pub(crate) fn to_number(value: &RuntimeHandle<'_>) -> Result<f64, EngineError> {
    let scope = RuntimeHandleScope::new();
    let primitive = if crate::proxy::reflect_value_is_object(value.get_nanbox_f64()) {
        scope.root_nanbox_f64(to_primitive(value, false)?)
    } else {
        scope.root_nanbox_f64(value.get_nanbox_f64())
    };
    if crate::value::JSValue::from_bits(primitive.get_nanbox_f64().to_bits()).is_bigint() {
        return Err(EngineError::Type(
            "Cannot convert a BigInt value to a number",
        ));
    }
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let result = api::caught(|| crate::builtins::js_number_coerce(primitive.get_nanbox_f64()));
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    result
}

fn to_primitive(value: &RuntimeHandle<'_>, string_hint: bool) -> Result<f64, EngineError> {
    use super::perex_replace::callable;
    use super::perex_replace_storage::{call, List};
    let scope = RuntimeHandleScope::new();
    let method = scope.root_nanbox_f64(get_symbol(value, "toPrimitive")?);
    let mut budget = Budget::new(api::WORK);
    let memory = MemoryBudget::new(api::SCRATCH_BYTES);
    if !matches!(
        method.get_nanbox_f64().to_bits(),
        TAG_NULL | crate::value::TAG_UNDEFINED
    ) {
        if !callable(&method)? {
            return Err(EngineError::Type("Symbol.toPrimitive is not callable"));
        }
        let mut args = List::new(&scope)?;
        let name = if string_hint { b"string" } else { b"number" };
        let hint = api::caught(|| crate::string::js_string_from_bytes(name.as_ptr(), 6))?;
        args.push(js_nanbox_string(hint as i64), &mut budget)?;
        let result = call(&method, value, &args, &memory)?;
        if crate::proxy::reflect_value_is_object(result) {
            return Err(EngineError::Type(
                "Cannot convert object to primitive value",
            ));
        }
        return Ok(result);
    }
    let args = List::new(&scope)?;
    let order = if string_hint {
        [b"toString".as_slice(), b"valueOf"]
    } else {
        [b"valueOf".as_slice(), b"toString"]
    };
    for name in order {
        let local = RuntimeHandleScope::new();
        let method = local.root_nanbox_f64(get(value, name)?);
        if callable(&method)? {
            let result = call(&method, value, &args, &memory)?;
            if !crate::proxy::reflect_value_is_object(result) {
                return Ok(result);
            }
        }
    }
    Err(EngineError::Type(
        "Cannot convert object to primitive value",
    ))
}

pub(crate) fn to_length(value: &RuntimeHandle<'_>) -> Result<f64, EngineError> {
    let n = to_number(value)?;
    Ok(if n.is_nan() || n <= 0.0 {
        0.0
    } else {
        n.floor().min(9_007_199_254_740_991.0)
    })
}

pub(crate) fn same_value(
    a: &RuntimeHandle<'_>,
    b: &RuntimeHandle<'_>,
) -> Result<bool, EngineError> {
    let av = crate::value::JSValue::from_bits(a.get_nanbox_f64().to_bits());
    let bv = crate::value::JSValue::from_bits(b.get_nanbox_f64().to_bits());
    if av.is_any_string() && bv.is_any_string() {
        let scope = RuntimeHandleScope::new();
        let a = scope.root_string_ptr(to_string(a)?);
        let b = scope.root_string_ptr(to_string(b)?);
        return Ok(
            crate::string::js_string_equals(a.get_raw_const_ptr(), b.get_raw_const_ptr()) != 0,
        );
    }
    Ok(
        crate::object::js_object_is(a.get_nanbox_f64(), b.get_nanbox_f64()).to_bits()
            == crate::value::TAG_TRUE,
    )
}

pub(crate) fn test_string(receiver: f64, input: *const StringHeader) -> Result<bool, EngineError> {
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let input = scope.root_string_ptr(input);
    execute(
        &receiver,
        &input,
        false,
        &mut Budget::new(api::WORK),
        &MemoryBudget::new(api::SCRATCH_BYTES),
        &mut host::poll,
    )
    .map(|result| result.is_some())
}

pub(crate) fn test_value(receiver: f64, argument: f64) -> Result<bool, EngineError> {
    require_object(receiver)?;
    let scope = RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let argument = scope.root_nanbox_f64(argument);
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let input =
        api::caught(|| crate::value::js_jsvalue_to_string_coerce(argument.get_nanbox_f64()));
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    test_string(receiver.get_nanbox_f64(), input?)
}
