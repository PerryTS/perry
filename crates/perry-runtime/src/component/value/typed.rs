//! Snapshot only fields selected by WIT. Extra object properties must not run
//! getters or retain unrelated owners while a canonical call is suspended.
use super::*;

pub(in crate::component) fn arguments(
    value: f64,
    params: &[Value],
    world: &Value,
    owner: f64,
    retained: f64,
) -> Value {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let retained = scope.root_nanbox_f64(retained);
    let input = scope.root_nanbox_f64(value);
    let result = (|| {
        let pointer = super::super::array_pointer(input.get_nanbox_f64())
            .ok_or_else(|| encoding_error("Expected component argument array"))?;
        if crate::array::js_array_length(pointer) as usize != params.len() {
            return Err(encoding_error("Component argument count mismatch"));
        }
        let mut values = Vec::with_capacity(params.len());
        for (index, param) in params.iter().enumerate() {
            let value = catch_js_throw(|| {
                crate::array::js_array_get_f64(
                    super::super::array_pointer(input.get_nanbox_f64()).unwrap(),
                    index as u32,
                )
            })?;
            values.push(at(value, &param["ty"], world, &owner, &retained, 1)?);
        }
        Ok(Value::Array(values))
    })();
    result.unwrap_or_else(|error| crate::exception::js_throw(error))
}

pub(in crate::component) fn result(
    value: f64,
    ty: &Value,
    world: &Value,
    owner: f64,
    retained: f64,
) -> Value {
    if ty.is_null() {
        return Value::Null;
    }
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let retained = scope.root_nanbox_f64(retained);
    at(value, ty, world, &owner, &retained, 0)
        .unwrap_or_else(|error| crate::exception::js_throw(error))
}

fn at(
    value: f64,
    ty: &Value,
    world: &Value,
    owner: &crate::gc::RuntimeHandle<'_>,
    retained: &crate::gc::RuntimeHandle<'_>,
    depth: usize,
) -> Result<Value, f64> {
    if depth >= 128 {
        return Err(encoding_error(
            "Component value nesting exceeds 128 (or contains a cycle)",
        ));
    }
    let scope = RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(value);
    let ty = catch_js_throw(|| resolved(world, ty))?;
    if super::super::array_pointer(input.get_nanbox_f64()).is_some()
        && matches!(ty["kind"].as_str(), Some("list" | "tuple"))
    {
        let len = crate::array::js_array_length(
            super::super::array_pointer(input.get_nanbox_f64()).unwrap(),
        );
        let mut values = Vec::with_capacity(len as usize);
        for index in 0..len {
            let child = catch_js_throw(|| {
                crate::array::js_array_get_f64(
                    super::super::array_pointer(input.get_nanbox_f64()).unwrap(),
                    index,
                )
            })?;
            let member = if ty["kind"] == "tuple" {
                &ty["value"][index as usize]
            } else {
                &ty["value"]
            };
            values.push(at(child, member, world, owner, retained, depth + 1)?);
        }
        return Ok(Value::Array(values));
    }
    if object_pointer(input.get_nanbox_f64()).is_some() {
        if ty["kind"] == "record" {
            let fields = ty["value"]
                .as_array()
                .ok_or_else(|| encoding_error("Invalid WIT record manifest"))?;
            let mut output = serde_json::Map::new();
            for member in fields {
                let name = member["name"]
                    .as_str()
                    .ok_or_else(|| encoding_error("Invalid WIT field name"))?;
                if name.starts_with('$') {
                    return Err(encoding_error(
                        "Reserved component transport key in a WIT record",
                    ));
                }
                let child = catch_js_throw(|| field(input.get_nanbox_f64(), name.as_bytes()))?;
                output.insert(
                    name.to_owned(),
                    at(child, &member["ty"], world, owner, retained, depth + 1)?,
                );
            }
            return Ok(Value::Object(output));
        }
        if matches!(ty["kind"].as_str(), Some("option" | "result" | "variant")) {
            let tag_value = catch_js_throw(|| field(input.get_nanbox_f64(), b"tag"))?;
            let tag = catch_js_throw(|| string(tag_value))?;
            let mut output = serde_json::Map::new();
            output.insert("tag".to_owned(), Value::String(tag.clone()));
            let payload = match ty["kind"].as_str() {
                Some("option") if tag == "some" => Some(&ty["value"]),
                Some("result") => ty["value"].get(&tag).filter(|value| !value.is_null()),
                Some("variant") => ty["value"]
                    .as_array()
                    .and_then(|cases| cases.iter().find(|case| case["name"] == tag))
                    .map(|case| &case["ty"])
                    .filter(|value| !value.is_null()),
                _ => None,
            };
            if let Some(payload) = payload {
                let child = catch_js_throw(|| field(input.get_nanbox_f64(), b"value"))?;
                output.insert(
                    "value".to_owned(),
                    at(child, payload, world, owner, retained, depth + 1)?,
                );
            }
            return Ok(Value::Object(output));
        }
    }
    encode_at(input.get_nanbox_f64(), depth, Some(owner), Some(retained))
}

#[cfg(test)]
mod tests;
