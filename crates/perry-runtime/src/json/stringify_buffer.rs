//! `Buffer` / `Uint8Array` / `TypedArray` JSON.stringify emitters (compact and
//! pretty-printed). Split out of `stringify.rs` (#5847) to keep that file
//! under the repo's file-size gate; these are self-contained leaf emitters
//! called from the traversal in `stringify.rs`/`replacer.rs`.

use super::*;

/// Issue #639: emit a Buffer / Uint8Array as JSON in the Node-compatible shape.
///
/// `Buffer.from(...)` returns `{"type":"Buffer","data":[b0,b1,...]}` (Node's
/// `Buffer.prototype.toJSON()` output). `new Uint8Array(...)` returns
/// `{"0":b0,"1":b1,...}` (the typed-array shape Node falls through to with
/// no custom `toJSON`). Distinguished via `is_uint8array_buffer`, which the
/// Uint8Array constructor path explicitly marks (see `buffer.rs::js_uint8array_*`).
///
/// Must be called BEFORE `gc_obj_type(ptr)` — `BufferHeader` has no `GcHeader`,
/// so reading 8 bytes before the header reads unrelated memory and would
/// dispatch to the wrong arm (or panic when `is_object_pointer` deref's a
/// bogus `keys_array` pointer).
pub(crate) unsafe fn stringify_buffer(ptr: *const u8, buf: &mut String) {
    let buf_ptr = ptr as *const crate::buffer::BufferHeader;
    if buf_ptr.is_null() {
        buf.push_str("null");
        return;
    }
    let len = (*buf_ptr).length as usize;
    let data = (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);

    if crate::buffer::is_uint8array_buffer(ptr as usize) {
        buf.push('{');
        for (i, b) in bytes.iter().enumerate() {
            if i > 0 {
                buf.push(',');
            }
            buf.push('"');
            let mut idx_buf = itoa::Buffer::new();
            buf.push_str(idx_buf.format(i));
            buf.push_str("\":");
            let mut byte_buf = itoa::Buffer::new();
            buf.push_str(byte_buf.format(*b));
        }
        buf.push('}');
    } else {
        buf.push_str(r#"{"type":"Buffer","data":["#);
        for (i, b) in bytes.iter().enumerate() {
            if i > 0 {
                buf.push(',');
            }
            let mut byte_buf = itoa::Buffer::new();
            buf.push_str(byte_buf.format(*b));
        }
        buf.push_str("]}");
    }
}

/// Issue #5111: serialize a `TypedArrayHeader`-backed typed array (`Int8Array`
/// … `Float64Array`, `BigInt64Array`/`BigUint64Array`, `Float16Array`, plus the
/// `map`/`subarray`/`slice`/`filter` results) in Node's shape `{"0":v,…}`.
///
/// Like `stringify_buffer`, this MUST run BEFORE `gc_obj_type`: a small typed
/// array is plain-`alloc`'d with NO `GcHeader`, so the gc-tag read 8 bytes
/// before the header reads unrelated allocator memory and dispatches to a
/// random arm — the SIGSEGV reported for `JSON.stringify(ta.map(...))`. Each
/// element is funneled through `write_number`, which renders `NaN`/`±Infinity`
/// as `null` and routes a `BigInt64`/`BigUint64` element to the throwing
/// serializer (Node's "Do not know how to serialize a BigInt" `TypeError`).
pub(crate) unsafe fn stringify_typed_array(ptr: *const u8, buf: &mut String) {
    let ta = ptr as *const crate::typedarray::TypedArrayHeader;
    let len = crate::typedarray::js_typed_array_length(ta);
    buf.push('{');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        let mut idx_buf = itoa::Buffer::new();
        buf.push('"');
        buf.push_str(idx_buf.format(i));
        buf.push_str("\":");
        write_number(buf, crate::typedarray::js_typed_array_get(ta, i));
    }
    buf.push('}');
}

/// Pretty-printed (`space`-indented) form of `stringify_typed_array`, matching
/// the layout of `stringify_buffer_pretty`'s plain-Uint8Array branch.
pub(crate) unsafe fn stringify_typed_array_pretty(
    ptr: *const u8,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    let ta = ptr as *const crate::typedarray::TypedArrayHeader;
    let len = crate::typedarray::js_typed_array_length(ta);
    if len <= 0 {
        buf.push_str("{}");
        return;
    }
    let push_indent = |buf: &mut String, levels: usize| {
        for _ in 0..levels {
            buf.push_str(indent);
        }
    };
    buf.push_str("{\n");
    for i in 0..len {
        push_indent(buf, depth + 1);
        let mut idx_buf = itoa::Buffer::new();
        buf.push('"');
        buf.push_str(idx_buf.format(i));
        buf.push_str("\": ");
        write_number(buf, crate::typedarray::js_typed_array_get(ta, i));
        if i + 1 < len {
            buf.push(',');
        }
        buf.push('\n');
    }
    push_indent(buf, depth);
    buf.push('}');
}

/// Pretty-printed (`space`-indented) form of `stringify_buffer`. Emits the
/// same `{type,data}` (Buffer) / `{index:byte}` (plain Uint8Array) shape as
/// the compact version but with newlines + indentation, matching Node's
/// `JSON.stringify(buf, null, n)`. `depth` is the indent level of the value
/// itself (content sits at `depth + 1`, the closing brace at `depth`),
/// mirroring `stringify_object_pretty`.
pub(crate) unsafe fn stringify_buffer_pretty(
    ptr: *const u8,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    let buf_ptr = ptr as *const crate::buffer::BufferHeader;
    if buf_ptr.is_null() {
        buf.push_str("null");
        return;
    }
    let len = (*buf_ptr).length as usize;
    let data = (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);

    let push_indent = |buf: &mut String, levels: usize| {
        for _ in 0..levels {
            buf.push_str(indent);
        }
    };

    if len == 0 {
        // Empty Uint8Array -> "{}"; empty Buffer -> {"type":"Buffer","data":[]}.
        if crate::buffer::is_uint8array_buffer(ptr as usize) {
            buf.push_str("{}");
        } else {
            buf.push_str("{\n");
            push_indent(buf, depth + 1);
            buf.push_str("\"type\": \"Buffer\",\n");
            push_indent(buf, depth + 1);
            buf.push_str("\"data\": []\n");
            push_indent(buf, depth);
            buf.push('}');
        }
        return;
    }

    if crate::buffer::is_uint8array_buffer(ptr as usize) {
        // Plain Uint8Array: { "0": b0, "1": b1, ... }
        buf.push_str("{\n");
        for (i, b) in bytes.iter().enumerate() {
            push_indent(buf, depth + 1);
            let mut idx_buf = itoa::Buffer::new();
            buf.push('"');
            buf.push_str(idx_buf.format(i));
            buf.push_str("\": ");
            let mut byte_buf = itoa::Buffer::new();
            buf.push_str(byte_buf.format(*b));
            if i + 1 < len {
                buf.push(',');
            }
            buf.push('\n');
        }
        push_indent(buf, depth);
        buf.push('}');
    } else {
        // Buffer: { "type": "Buffer", "data": [ b0, b1, ... ] }
        buf.push_str("{\n");
        push_indent(buf, depth + 1);
        buf.push_str("\"type\": \"Buffer\",\n");
        push_indent(buf, depth + 1);
        buf.push_str("\"data\": [\n");
        for (i, b) in bytes.iter().enumerate() {
            push_indent(buf, depth + 2);
            let mut byte_buf = itoa::Buffer::new();
            buf.push_str(byte_buf.format(*b));
            if i + 1 < len {
                buf.push(',');
            }
            buf.push('\n');
        }
        push_indent(buf, depth + 1);
        buf.push_str("]\n");
        push_indent(buf, depth);
        buf.push('}');
    }
}
