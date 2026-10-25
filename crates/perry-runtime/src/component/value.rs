//! Component transport values, with moving-GC roots at every allocating edge.
use super::{fail, field, object_pointer, set_field, string};
use crate::exception::catch_js_throw;
use crate::{gc::RuntimeHandleScope, value::JSValue};
use serde_json::{json, Value};

pub(super) fn encode(value: f64) -> Value {
    encode_at(value, 0, None, None).unwrap_or_else(|error| crate::exception::js_throw(error))
}

pub(super) fn encode_for(value: f64, owner: f64) -> Value {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    encode_at(value, 0, Some(&owner), None)
        .unwrap_or_else(|error| crate::exception::js_throw(error))
}

/// Async submission snapshots bytes immediately but must also retain resource
/// wrappers until native lowering commits ownership. Retain them during this
/// traversal, including resources produced by getters or later removed from
/// the caller's mutable argument objects.
pub(super) fn encode_for_retained(value: f64, owner: f64, retained: f64) -> Value {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let retained = scope.root_nanbox_f64(retained);
    encode_at(value, 0, Some(&owner), Some(&retained))
        .unwrap_or_else(|error| crate::exception::js_throw(error))
}

fn encoding_error(message: &str) -> f64 {
    catch_js_throw(|| fail(message)).unwrap_err()
}

// Native containers live outside each throwing JS operation. A getter or
// rejected child returns Err through Rust so partially encoded values drop.
fn encode_at(
    value: f64,
    depth: usize,
    owner: Option<&crate::gc::RuntimeHandle<'_>>,
    retained: Option<&crate::gc::RuntimeHandle<'_>>,
) -> Result<Value, f64> {
    if depth >= 128 {
        return Err(encoding_error(
            "Component value nesting exceeds 128 (or contains a cycle)",
        ));
    }
    let scope = RuntimeHandleScope::new();
    let root = scope.root_nanbox_f64(value);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return Ok(Value::Null);
    }
    if js.is_bool() {
        return Ok(json!(js.as_bool()));
    }
    if js.is_number() {
        let number = js.as_number();
        return Ok(if number.is_nan() {
            json!({"$float":"nan"})
        } else if number == f64::INFINITY {
            json!({"$float":"infinity"})
        } else if number == f64::NEG_INFINITY {
            json!({"$float":"-infinity"})
        } else if number == 0.0 && number.is_sign_negative() {
            json!({"$float":"-0"})
        } else if number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0 {
            if number >= 0.0 {
                json!(number as u64)
            } else {
                json!(number as i64)
            }
        } else {
            json!(number)
        });
    }
    if js.is_any_string() {
        return Ok(json!(string(root.get_nanbox_f64())));
    }
    if js.is_bigint() {
        let text = crate::bigint::js_bigint_to_string(js.as_bigint_ptr());
        return Ok(json!({"$bigint":string(crate::value::js_nanbox_string(text as i64))}));
    }
    if super::array_pointer(root.get_nanbox_f64()).is_some() {
        let pointer = || {
            JSValue::from_bits(root.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>()
        };
        let len = crate::array::js_array_length(pointer());
        let mut values = Vec::with_capacity(len as usize);
        for i in 0..len {
            let child = catch_js_throw(|| crate::array::js_array_get_f64(pointer(), i))?;
            values.push(encode_at(child, depth + 1, owner, retained)?);
        }
        return Ok(Value::Array(values));
    }
    let js = JSValue::from_bits(root.get_nanbox_f64().to_bits());
    if js.is_pointer()
        && crate::typedarray::lookup_typed_array_kind(js.as_pointer::<u8>() as usize)
            == Some(crate::typedarray::KIND_UINT8)
    {
        return crate::buffer::bytes::no_gc(|scope| {
            crate::buffer::bytes::bytes(root.get_nanbox_f64(), scope)
                .map(|bytes| Value::Array(bytes.iter().map(|byte| json!(*byte)).collect()))
        })
        .map_err(|_| encoding_error("Detached component byte list"));
    }
    if object_pointer(root.get_nanbox_f64()).is_some() {
        if let Some(resource) = catch_js_throw(|| {
            super::host_resource::encode(
                root.get_nanbox_f64(),
                owner.map_or_else(super::undefined, |owner| owner.get_nanbox_f64()),
            )
        })? {
            retain_resource(retained, &root)?;
            return Ok(resource);
        }
        if let Some(resource) = catch_js_throw(|| super::resource::encode(root.get_nanbox_f64()))? {
            retain_resource(retained, &root)?;
            return Ok(resource);
        }
        let keys = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(catch_js_throw(|| {
            crate::object::js_object_keys_value(root.get_nanbox_f64())
        })? as i64));
        let pointer = || {
            JSValue::from_bits(keys.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>()
        };
        let len = crate::array::js_array_length(pointer());
        let mut object = serde_json::Map::new();
        for i in 0..len {
            let key = string(crate::array::js_array_get_f64(pointer(), i));
            if key.starts_with('$') {
                return Err(encoding_error(
                    "Reserved component transport key in a plain object",
                ));
            }
            let value = catch_js_throw(|| field(root.get_nanbox_f64(), key.as_bytes()))?;
            object.insert(key, encode_at(value, depth + 1, owner, retained)?);
        }
        return Ok(Value::Object(object));
    }
    Err(encoding_error(
        "Value cannot cross a WIT component boundary",
    ))
}

fn retain_resource(
    retained: Option<&crate::gc::RuntimeHandle<'_>>,
    resource: &crate::gc::RuntimeHandle<'_>,
) -> Result<(), f64> {
    if let Some(retained) = retained {
        catch_js_throw(|| {
            let array =
                super::array_pointer(retained.get_nanbox_f64()).expect("retained argument list");
            let count = unsafe { (*array).length };
            crate::array::js_array_set_f64_extend(array, count, resource.get_nanbox_f64());
        })?;
    }
    Ok(())
}

fn resolved<'a>(world: &'a Value, mut ty: &'a Value) -> &'a Value {
    for _ in 0..128 {
        if ty["kind"] != "ref" {
            return ty;
        }
        let Some(name) = ty["value"].as_str() else {
            fail("Invalid WIT type reference");
        };
        ty = &world["types"][name]["ty"];
    }
    fail("Cyclic WIT type reference")
}

pub(super) fn decode(value: &Value, ty: &Value, world: &Value, owner: f64) -> f64 {
    decode_with_lifetime(value, ty, world, owner, super::undefined())
}

pub(super) fn decode_with_lifetime(
    value: &Value,
    ty: &Value,
    world: &Value,
    owner: f64,
    lifetime: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let lifetime = scope.root_nanbox_f64(lifetime);
    decode_at(value, ty, world, &owner, &lifetime, 0)
}

fn decode_at(
    value: &Value,
    ty: &Value,
    world: &Value,
    owner: &crate::gc::RuntimeHandle<'_>,
    lifetime: &crate::gc::RuntimeHandle<'_>,
    depth: usize,
) -> f64 {
    if depth >= 128 {
        fail("Component result nesting exceeds 128");
    }
    let ty = resolved(world, ty);
    let scope = RuntimeHandleScope::new();
    match value {
        Value::Null => f64::from_bits(crate::value::TAG_UNDEFINED),
        Value::Bool(value) => f64::from_bits(JSValue::bool(*value).bits()),
        Value::Number(value) => value
            .as_f64()
            .unwrap_or_else(|| fail("Invalid component number")),
        Value::String(value) => crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64,
        ),
        Value::Array(values)
            if ty["kind"] == "list" && resolved(world, &ty["value"])["kind"] == "u8" =>
        {
            let pointer = crate::typedarray::typed_array_alloc(
                crate::typedarray::KIND_UINT8,
                values
                    .len()
                    .try_into()
                    .unwrap_or_else(|_| fail("Component byte list is too long")),
            );
            let array = scope.root_raw_mut_ptr(pointer);
            for (i, value) in values.iter().enumerate() {
                crate::typedarray::js_typed_array_set(
                    array.get_raw_mut_ptr(),
                    i as i32,
                    value
                        .as_f64()
                        .unwrap_or_else(|| fail("Invalid component byte")),
                );
            }
            crate::value::js_nanbox_pointer(
                array.get_raw_mut_ptr::<crate::typedarray::TypedArrayHeader>() as i64,
            )
        }
        Value::Array(values) => {
            let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(
                values
                    .len()
                    .try_into()
                    .unwrap_or_else(|_| fail("Component list is too long")),
            ));
            for (i, value) in values.iter().enumerate() {
                let item_type = if ty["kind"] == "tuple" {
                    &ty["value"][i]
                } else {
                    &ty["value"]
                };
                let value = decode_at(value, item_type, world, owner, lifetime, depth + 1);
                crate::array::js_array_set_f64(array.get_raw_mut_ptr(), i as u32, value);
            }
            crate::value::js_nanbox_pointer(
                array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64
            )
        }
        Value::Object(object) => {
            if object.contains_key("$hostResource") {
                return super::host_resource::decode(
                    owner.get_nanbox_f64(),
                    value,
                    lifetime.get_nanbox_f64(),
                );
            }
            if let Some(text) = object.get("$bigint").and_then(Value::as_str) {
                return crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_string(
                    text.as_ptr(),
                    text.len() as u32,
                ) as i64);
            }
            if let Some(number) = object.get("$float").and_then(Value::as_str) {
                return match number {
                    "nan" => f64::NAN,
                    "infinity" => f64::INFINITY,
                    "-infinity" => f64::NEG_INFINITY,
                    "-0" => -0.0,
                    _ => fail("Invalid component float"),
                };
            }
            if object.contains_key("$resource") {
                return super::resource::decode(
                    owner.get_nanbox_f64(),
                    value,
                    lifetime.get_nanbox_f64(),
                );
            }
            let output = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            for (key, value) in object {
                let field_type = member_type(ty, key, object.get("tag"));
                let child = decode_at(value, field_type, world, owner, lifetime, depth + 1);
                set_field(output.get_nanbox_f64(), key.as_bytes(), child);
            }
            output.get_nanbox_f64()
        }
    }
}

fn member_type<'a>(ty: &'a Value, key: &str, tag: Option<&Value>) -> &'a Value {
    static NULL: Value = Value::Null;
    match ty["kind"].as_str() {
        Some("record") => ty["value"]
            .as_array()
            .and_then(|fields| fields.iter().find(|f| f["name"] == key))
            .map(|f| &f["ty"])
            .unwrap_or(&NULL),
        Some("option") if key == "value" => &ty["value"],
        Some("result") if key == "value" => &ty["value"][tag.and_then(Value::as_str).unwrap_or("")],
        Some("variant") if key == "value" => ty["value"]
            .as_array()
            .and_then(|cases| cases.iter().find(|c| Some(&c["name"]) == tag))
            .map(|c| &c["ty"])
            .unwrap_or(&NULL),
        _ => &NULL,
    }
}
