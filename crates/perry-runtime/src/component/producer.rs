//! Component-bound canonical factory capabilities. Public metadata is a copy;
//! creating a value uses the schema/index captured privately by its closure.
use super::*;

/// Expand structural aliases for discovery; resource ownership/type keys stay
/// nominal. Native factory indices and the original schema remain private.
pub(super) fn expanded_schema(
    world: &Value,
    schema: &Value,
    depth: usize,
) -> Result<Value, &'static str> {
    if depth >= 128 {
        return Err("Async factory schema exceeds the nesting limit");
    }
    if schema["kind"] == "ref" {
        let key = schema["value"]
            .as_str()
            .ok_or("Invalid async factory type reference")?;
        let ty = &world["types"][key]["ty"];
        if ty.is_null() {
            return Err("Unknown async factory type reference");
        }
        return expanded_schema(world, ty, depth + 1);
    }
    match schema {
        Value::Array(values) => values
            .iter()
            .map(|value| expanded_schema(world, value, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), expanded_schema(world, value, depth + 1)?)))
            .collect::<Result<serde_json::Map<_, _>, _>>()
            .map(Value::Object),
        _ => Ok(schema.clone()),
    }
}

pub(super) fn managed_text(bytes: &[u8]) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ) as i64)
}
pub(super) fn install(owner: f64) {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(types, 0),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        closure.get_raw_mut_ptr(),
        0,
        owner.get_nanbox_f64(),
    );
    crate::object::set_builtin_closure_non_constructable(
        closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
    );
    set_field(
        owner.get_nanbox_f64(),
        b"asyncTypes",
        crate::value::js_nanbox_pointer(
            closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
        ),
    );
}

extern "C" fn types(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let mut bytes = Vec::new();
    let mut types = Value::Null;
    let mut schema = Vec::new();
    let mut display = Value::Null;
    let mut display_bytes = Vec::new();
    let outcome = crate::exception::catch_js_throw(|| {
        let ctx = context::get(owner.get_nanbox_f64());
        if unsafe { !(*ctx).alive || (*ctx).host.is_null() }
            || !persistent::enabled(owner.get_nanbox_f64())
        {
            fail("Component is not initialized or has been disposed");
        }
        let mut buffer = Buffer::default();
        let status =
            unsafe { (host::ops().perry_component_driver_types)((*ctx).host, &mut buffer) };
        bytes = unsafe { take(buffer) };
        if status != perry_abi::component_host::OK {
            fail(&String::from_utf8_lossy(&bytes));
        }
        types = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| fail("Invalid async factory metadata"));
        let list = types
            .as_array()
            .unwrap_or_else(|| fail("Invalid async factory list"));
        let output = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(
            list.len().try_into().unwrap(),
        ));
        for (position, ty) in list.iter().enumerate() {
            let index = ty["index"]
                .as_u64()
                .filter(|n| *n <= 9_007_199_254_740_991)
                .unwrap_or_else(|| fail("Invalid async factory index"));
            let kind = ty["schema"]["kind"]
                .as_str()
                .filter(|kind| matches!(*kind, "future" | "stream"))
                .unwrap_or_else(|| fail("Invalid async factory schema"));
            schema = serde_json::to_vec(&ty["schema"]).unwrap();
            let private_schema = scope.root_nanbox_f64(managed_text(&schema));
            display = expanded_schema(
                unsafe { &(*context::get(owner.get_nanbox_f64())).world },
                &ty["schema"],
                0,
            )
            .unwrap_or_else(|message| fail(message));
            display_bytes = serde_json::to_vec(&display).unwrap();
            let public_schema = scope.root_nanbox_f64(managed_text(&display_bytes));
            let factory = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            let kind = scope.root_nanbox_f64(managed_text(kind.as_bytes()));
            set_field(factory.get_nanbox_f64(), b"kind", kind.get_nanbox_f64());
            set_field(
                factory.get_nanbox_f64(),
                b"schema",
                public_schema.get_nanbox_f64(),
            );
            let create = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
                crate::fn_info!(create, 0),
                3,
            ));
            for (slot, value) in [
                owner.get_nanbox_f64(),
                index as f64,
                private_schema.get_nanbox_f64(),
            ]
            .into_iter()
            .enumerate()
            {
                crate::closure::js_closure_set_capture_f64(
                    create.get_raw_mut_ptr(),
                    slot as u32,
                    value,
                );
            }
            crate::object::set_builtin_closure_non_constructable(
                create.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            );
            set_field(
                factory.get_nanbox_f64(),
                b"create",
                crate::value::js_nanbox_pointer(
                    create.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
                ),
            );
            crate::array::js_array_set_f64(
                output.get_raw_mut_ptr(),
                position as u32,
                factory.get_nanbox_f64(),
            );
        }
        crate::value::js_nanbox_pointer(output.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64)
    });
    drop((bytes, types, schema, display, display_bytes));
    finish(outcome)
}

extern "C" fn create(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let index = crate::closure::js_closure_get_capture_f64(closure, 1) as u64;
    let schema = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
    let mut parsed = Value::Null;
    let outcome = crate::exception::catch_js_throw(|| {
        parsed = serde_json::from_str(&string(schema.get_nanbox_f64()))
            .unwrap_or_else(|_| fail("Invalid private factory schema"));
        persistent::operations::create(owner.get_nanbox_f64(), index, &parsed)
    });
    drop(parsed);
    finish(outcome)
}

pub(super) fn decode(owner: f64, result: &Value, schema: &Value) -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let stream = schema["kind"] == "stream";
    let readable = scope.root_nanbox_f64(async_value::decode(
        owner.get_nanbox_f64(),
        &result["value"],
        &schema["value"],
        stream,
    ));
    let writer = scope.root_nanbox_f64(writer::decode(
        owner.get_nanbox_f64(),
        &result["writer"],
        &schema["value"],
        stream,
    ));
    let output = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let kind = scope.root_nanbox_f64(managed_text(if stream { b"stream" } else { b"future" }));
    set_field(output.get_nanbox_f64(), b"kind", kind.get_nanbox_f64());
    set_field(output.get_nanbox_f64(), b"value", readable.get_nanbox_f64());
    set_field(output.get_nanbox_f64(), b"writer", writer.get_nanbox_f64());
    output.get_nanbox_f64()
}
