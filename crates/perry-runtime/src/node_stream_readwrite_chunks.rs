//! Rooted pipeline chunk collection, split for the file-size gate.
use super::*;

/// The chunks a pipeline source holds, copied into one fresh GC array rooted
/// in `scope`, or `None` when it holds none.
///
/// `_read` and the hidden-property reads (generic lookups, so a getter may
/// run) can collect, so the stream is reread from its handle after each and
/// the chunks go straight into a GC array the collector rewrites.
pub(crate) fn js_node_stream_readable_chunks_result<'s>(
    scope: &'s crate::gc::RuntimeHandleScope,
    stream: &crate::gc::RuntimeHandle<'_>,
) -> Result<Option<crate::gc::RuntimeHandle<'s>>, f64> {
    invoke_read_once(stream.get_nanbox_f64());
    if let Some(err) = readable_hidden_error(stream.get_nanbox_f64()) {
        return Err(err);
    }
    let Some(chunks) = readable_hidden_chunks(stream.get_nanbox_f64()) else {
        return Ok(None);
    };
    let chunks = scope.root_nanbox_f64(chunks);
    let out = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    append_chunk_values(&chunks, &out, 0);
    if let Some(err) = readable_hidden_error(stream.get_nanbox_f64()) {
        return Err(err);
    }
    Ok(Some(out))
}

/// [`push_chunk_values`] into a rooted GC array: `value` is reread from its
/// handle after every step that can collect.
fn append_chunk_values(
    value: &crate::gc::RuntimeHandle<'_>,
    out: &crate::gc::RuntimeHandle<'_>,
    depth: u8,
) {
    if depth > 8 {
        return;
    }
    // `js_array_push_f64` roots its receiver and the value before it can grow.
    let push = |chunk: f64| {
        out.set_raw_mut_ptr(out.with_mut_ptr(|arr: *mut crate::array::ArrayHeader| {
            crate::array::js_array_push_f64(arr, chunk)
        }));
    };
    if let Some(chunks) = readable_hidden_chunks(value.get_nanbox_f64()) {
        let scope = crate::gc::RuntimeHandleScope::new();
        append_chunk_values(&scope.root_nanbox_f64(chunks), out, depth + 1);
        return;
    }
    // `is_array_like_value` has already rejected the handle band.
    if is_array_like_value(value.get_nanbox_f64()) {
        let arr = || raw_ptr_from_value(value.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        let mut i = 0;
        while i < crate::array::js_array_length(arr()) {
            push(crate::array::js_array_get_f64(arr(), i));
            i += 1;
        }
        return;
    }
    if is_single_chunk_value(value.get_nanbox_f64()) {
        push(value.get_nanbox_f64());
    }
}
