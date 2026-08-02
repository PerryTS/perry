//! `node:test` snapshot support: `t.assert.snapshot()` /
//! `t.assert.fileSnapshot()`, the serializer/path-resolver setters, and the
//! `snapshot` export object they hang off.
//!
//! Split verbatim out of the parent [`super`] module to keep `test.rs` under
//! the 2000-line gate. Pure code move — no behavior change.

use super::*;

fn json_stringify_pretty(value: f64) -> String {
    let spacer = string_value("  ");
    let bits =
        unsafe { crate::json::js_json_stringify_full(value, undefined_value(), spacer) } as u64;
    if bits == TAG_UNDEFINED {
        return "undefined".to_string();
    }
    let boxed = f64::from_bits(bits);
    value_to_string(boxed).unwrap_or_else(|| "undefined".to_string())
}

fn snapshot_payload(value: f64) -> String {
    let json = json_stringify_pretty(value);
    if json == "undefined" {
        crate::builtins::format_jsvalue(value, 0)
    } else {
        json
    }
}

pub(super) extern "C" fn snapshot_set_default_serializers(
    _closure: *const ClosureHeader,
    serializers: f64,
) -> f64 {
    if !is_array_value(serializers) {
        throw_invalid_arg_type("serializers", "Array", serializers);
    }
    undefined_value()
}

pub(super) extern "C" fn snapshot_set_resolve_snapshot_path(
    _closure: *const ClosureHeader,
    resolver: f64,
) -> f64 {
    if !is_callable_value(resolver) {
        throw_invalid_arg_type("fn", "function", resolver);
    }
    SNAPSHOT_RESOLVER.with(|slot| slot.set(resolver));
    undefined_value()
}

pub(super) extern "C" fn assert_snapshot(_closure: *const ClosureHeader, value: f64) -> f64 {
    CURRENT_ASSERT_COUNT.with(|count| count.set(count.get() + 1));
    let resolver = SNAPSHOT_RESOLVER.with(|slot| slot.get());
    if !is_callable_value(resolver) {
        throw_error_with_code(
            "Invalid state: snapshot.setResolveSnapshotPath() must be called before t.assert.snapshot()",
            "ERR_INVALID_STATE",
        );
    }
    let resolver_ptr = raw_ptr_from_value(resolver) as *const ClosureHeader;
    let path_value = js_closure_call1(resolver_ptr, string_value(""));
    let Some(path) = value_to_string(path_value) else {
        throw_invalid_arg_type("snapshot path", "string", path_value);
    };
    let file = fs::read_to_string(&path).unwrap_or_else(|_| {
        throw_error_with_code(
            &format!("Invalid state: snapshot file does not exist: {path}"),
            "ERR_INVALID_STATE",
        )
    });
    let name = CURRENT_TEST_NAME
        .with(|n| n.borrow().clone())
        .unwrap_or_else(|| "snapshot".to_string());
    let index = CURRENT_SNAPSHOT_INDEX.with(|idx| {
        let next = idx.get() + 1;
        idx.set(next);
        next
    });
    let marker = format!("exports[`{} {}`] = `", name, index);
    let Some(start) = file.find(&marker).map(|pos| pos + marker.len()) else {
        throw_error_with_code(
            &format!("Snapshot `{name} {index}` was not found"),
            "ERR_INVALID_STATE",
        );
    };
    let Some(end_rel) = file[start..].find("`;") else {
        throw_error_with_code("Snapshot file is malformed", "ERR_INVALID_STATE");
    };
    let expected = &file[start..start + end_rel];
    let actual = format!("\n{}\n", snapshot_payload(value));
    if expected.trim_end() != actual.trim_end() {
        throw_error_with_code(
            &format!(
                "Snapshot mismatch for `{name} {index}`\nExpected:\n{expected}\nActual:\n{actual}"
            ),
            "ERR_ASSERTION",
        );
    }
    undefined_value()
}

pub(super) extern "C" fn assert_file_snapshot(
    _closure: *const ClosureHeader,
    value: f64,
    path_value: f64,
) -> f64 {
    CURRENT_ASSERT_COUNT.with(|count| count.set(count.get() + 1));
    let Some(path) = value_to_string(path_value) else {
        throw_invalid_arg_type("path", "string", path_value);
    };
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        throw_error_with_code(
            &format!("Invalid state: snapshot file does not exist: {path}"),
            "ERR_INVALID_STATE",
        )
    });
    let actual = snapshot_payload(value);
    if expected.trim_end() != actual.trim_end() {
        throw_error_with_code(
            &format!("File snapshot mismatch for `{path}`"),
            "ERR_ASSERTION",
        );
    }
    undefined_value()
}

pub(super) fn snapshot_object_value() -> f64 {
    SNAPSHOT_OBJECT.with(|slot| {
        if let Some(ptr) = *slot.borrow() {
            return boxed_ptr(ptr);
        }
        let obj = js_object_alloc(0, 2);
        set_field(
            obj,
            "setDefaultSnapshotSerializers",
            closure_value(snapshot_set_default_serializers as *const u8, 1),
        );
        set_field(
            obj,
            "setResolveSnapshotPath",
            closure_value(snapshot_set_resolve_snapshot_path as *const u8, 1),
        );
        *slot.borrow_mut() = Some(obj);
        boxed_ptr(obj)
    })
}
