//! Proxy serialization uses internal operations, never the target's storage.

use super::*;

/// Run SerializeJSONProperty's toJSON lookup on a proxy. Even a missing
/// method is observable through `get`, so pass that verdict to the value walk.
pub(super) unsafe fn to_json(value: f64) -> Option<f64> {
    let ptr = extract_pointer(value.to_bits())?;
    let result = object_get_to_json(ptr);
    if result.is_none() || result.map(f64::to_bits) == Some(value.to_bits()) {
        TO_JSON_RESOLVED_FOR.with(|c| c.set(ptr as usize));
    }
    result
}

#[derive(Clone, Copy)]
pub(super) enum Replacer<'a> {
    None,
    Function(*const crate::ClosureHeader),
    Keys(&'a [String]),
}

/// Dispatch a proxy before the native-handle rejection in the ordinary walks.
/// `prepared` means toJSON and the function replacer have already run.
pub(super) unsafe fn try_stringify(
    value: f64,
    buf: &mut String,
    indent: &str,
    depth: usize,
    replacer: Replacer<'_>,
    prepared: bool,
) -> bool {
    if crate::proxy::js_proxy_is_proxy(value) == 0 {
        return false;
    }
    check_stringify_nesting_depth(depth);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let replacer_root = match replacer {
        Replacer::Function(f) => Some(scope.root_raw_const_ptr(f)),
        _ => None,
    };
    let id = (value.to_bits() & POINTER_MASK) as usize;
    let resolved = TO_JSON_RESOLVED_FOR.with(|c| c.replace(0)) == id;
    let suppressed = SUPPRESS_NEXT_TO_JSON.with(|c| c.replace(false));
    if !prepared && !resolved && !suppressed {
        if let Some(result) = object_get_to_json(id as *const u8) {
            emit_prepared(result, buf, indent, depth, replacer);
            return true;
        }
    }
    if crate::proxy::proxy_wraps_callable(value) {
        buf.push_str("null");
        return true;
    }
    let is_array = crate::array::js_array_is_array(value).to_bits() == TAG_TRUE;
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&id)) {
        let msg = "Converting circular structure to JSON";
        let key = crate::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let error = crate::error::js_typeerror_new(key);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(id));

    let mut keys = crate::gc::RootedValues::new(&scope);
    let len = if is_array {
        let key = crate::string::canonical_key(b"length");
        let length = crate::object::native_get::get_by_canonical_key(value, key);
        let number = crate::builtins::js_number_coerce(length);
        // LengthOfArrayLike / ToLength, including a trapped length.
        if number.is_nan() || number <= 0.0 {
            0
        } else {
            number.floor().min(9_007_199_254_740_991.0) as u64
        }
    } else {
        match replacer {
            Replacer::Keys(names) => {
                for name in names {
                    let key = crate::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    keys.push(nanbox_string_f64(key));
                }
            }
            _ => {
                // EnumerableOwnProperties snapshots keys and descriptors before
                // any value Get; this also validates ownKeys trap invariants.
                let names = crate::proxy::proxy_enum_own_keys(value);
                let names = scope.root_nanbox_f64(names);
                let arr =
                    (names.get_nanbox_f64().to_bits() & POINTER_MASK) as *const crate::ArrayHeader;
                for i in 0..(*arr).length {
                    keys.push(f64::from_bits(crate::array::js_array_get(arr, i).bits()));
                }
            }
        }
        keys.len() as u64
    };
    buf.push(if is_array { '[' } else { '{' });
    let mut first = true;
    for i in 0..len {
        let member_scope = crate::gc::RuntimeHandleScope::new();
        let key = if is_array {
            let index = i.to_string();
            nanbox_string_f64(crate::js_string_from_bytes(
                index.as_ptr(),
                index.len() as u32,
            ))
        } else {
            keys.get(i as usize)
        };
        let key = member_scope.root_nanbox_f64(key);
        let member = crate::proxy::js_proxy_get(receiver.get_nanbox_f64(), key.get_nanbox_f64());
        let member = super::replacer::apply_to_json_keyed(member, key.get_nanbox_f64());
        let member = member_scope.root_nanbox_f64(member);
        let member = match replacer_root.as_ref() {
            Some(root) => root.with_const_ptr(|f: *const crate::ClosureHeader| {
                super::replacer::call_replacer(
                    f,
                    key.get_nanbox_f64(),
                    member.get_nanbox_f64(),
                    receiver.get_nanbox_f64(),
                )
            }),
            None => member.get_nanbox_f64(),
        };
        // The replacer result is the value emitted below; keep it rooted across
        // the key write and any allocation in the emit.
        let member = member_scope.root_nanbox_f64(member);
        let member_bits = member.get_nanbox_f64();
        if !is_array && omitted(member_bits) {
            continue;
        }
        if !first {
            buf.push(',');
        }
        first = false;
        newline(buf, indent, depth + 1);
        if !is_array {
            stringify_value(key.get_nanbox_f64(), TYPE_UNKNOWN, buf);
            buf.push(':');
            if !indent.is_empty() {
                buf.push(' ');
            }
        }
        let member = member.get_nanbox_f64();
        match replacer_root.as_ref() {
            Some(root) => root.with_const_ptr(|f: *const crate::ClosureHeader| {
                emit_prepared(member, buf, indent, depth + 1, Replacer::Function(f))
            }),
            None => emit_prepared(member, buf, indent, depth + 1, replacer),
        }
    }
    if !first {
        newline(buf, indent, depth);
    }
    buf.push(if is_array { ']' } else { '}' });
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
    true
}

unsafe fn omitted(value: f64) -> bool {
    value.to_bits() == TAG_UNDEFINED
        || is_symbol_value(value.to_bits())
        || crate::proxy::is_callable_function(value)
}

unsafe fn emit_prepared(
    value: f64,
    buf: &mut String,
    indent: &str,
    depth: usize,
    replacer: Replacer<'_>,
) {
    if omitted(value) {
        buf.push_str("null");
        return;
    }
    if try_stringify(value, buf, indent, depth, replacer, true) {
        return;
    }
    match replacer {
        Replacer::Function(f) => {
            if !super::replacer::write_replaced_scalar(buf, value) {
                super::replacer::dispatch_pointer_with_replacer(
                    extract_pointer(value.to_bits()).unwrap(),
                    value,
                    f,
                    buf,
                    indent,
                    depth,
                );
            }
        }
        Replacer::Keys(keys) => super::replacer::stringify_value_with_array_replacer(
            value,
            keys,
            buf,
            indent,
            depth,
            !indent.is_empty(),
        ),
        Replacer::None => {
            arm_to_json_result_guard(value);
            if indent.is_empty() {
                super::stringify::stringify_value_depth(value, TYPE_UNKNOWN, buf, depth as u32);
            } else {
                super::replacer::stringify_value_pretty(value, TYPE_UNKNOWN, buf, indent, depth);
            }
            SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
        }
    }
}

fn newline(buf: &mut String, indent: &str, depth: usize) {
    if !indent.is_empty() {
        buf.push('\n');
        for _ in 0..depth {
            buf.push_str(indent);
        }
    }
}
