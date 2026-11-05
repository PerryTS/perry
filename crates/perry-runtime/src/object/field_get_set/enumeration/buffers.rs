use super::*;

/// #8149: the own property keys of a REGISTERED BUFFER receiver, in
/// `OrdinaryOwnPropertyKeys` order — canonical array indices ascending, then
/// the string keys.
///
/// `None` when `addr` is not a registered buffer at all, so the caller keeps
/// its ordinary walk. `Some` for all four buffer-backed types, and the four do
/// NOT answer the same thing:
///
/// * a node `Buffer` / `Uint8Array` IS an integer-indexed exotic object, so its
///   byte indices are own properties — `Object.keys(Buffer.from([1,2,3]))` is
///   `["0","1","2"]`;
/// * an `ArrayBuffer` / `SharedArrayBuffer` / `DataView` has NONE — only
///   whatever the user assigned (`dv.foo = 1`, or `dv[0] = 7`, which creates an
///   ordinary property rather than writing a byte).
///
/// Before this arm existed the enumeration paths had no registered-buffer case
/// and fell through to the generic `ObjectHeader` walk, reading buffer payload
/// bytes as the `keys_array` pointer. That answered `[]` when those bytes
/// happened to be zero — which is why `Object.keys(Buffer)` looked merely wrong
/// — and SIGBUS'd in `js_array_length` when they did not, e.g.
/// `Object.keys(new DataView(new ArrayBuffer(8)))` in any program that had also
/// allocated a `Buffer`.
///
/// Expando ordering among the non-index keys follows property creation order,
/// recorded by `buffer::own_props`. Canonical indices are still separated and
/// sorted below, as required by `OrdinaryOwnPropertyKeys`.
pub(crate) fn registered_buffer_own_keys(addr: usize) -> Option<Vec<String>> {
    if addr == 0 || !crate::buffer::is_registered_buffer(addr) {
        return None;
    }
    let mut indices: Vec<u32> = Vec::new();
    if crate::buffer::is_byte_indexed_buffer(addr) {
        let len = crate::buffer::js_buffer_length(addr as *const crate::buffer::BufferHeader);
        indices.extend(0..len.max(0) as u32);
    }
    let mut names: Vec<String> = Vec::new();
    for name in crate::buffer::buffer_own_prop_names(addr) {
        match canonical_array_index(&name) {
            Some(idx) if !indices.contains(&idx) => indices.push(idx),
            Some(_) => {}
            None => names.push(name),
        }
    }
    indices.sort_unstable();
    let mut keys: Vec<String> = indices.into_iter().map(|i| i.to_string()).collect();
    keys.append(&mut names);
    Some(keys)
}

/// The value each key of [`registered_buffer_own_keys`] names: the byte for an
/// in-bounds index of a byte-indexed buffer, else the stored own property.
pub(crate) fn registered_buffer_own_value(addr: usize, key: &str) -> f64 {
    if let Some(v) = crate::buffer::buffer_read_own_prop(addr, key) {
        return v;
    }
    if crate::buffer::is_byte_indexed_buffer(addr) {
        if let Some(idx) = canonical_array_index(key) {
            let buf = addr as *const crate::buffer::BufferHeader;
            if (idx as i32) < crate::buffer::js_buffer_length(buf) {
                return f64::from(crate::buffer::js_buffer_get(buf, idx as i32));
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Build the `Object.keys` / `.values` / `.entries` answer for a registered
/// buffer from [`registered_buffer_own_keys`].
pub(in super::super) fn registered_buffer_enum(
    addr: usize,
    what: MapSetEnum,
) -> Option<*mut ArrayHeader> {
    if addr == 0 || !crate::buffer::is_registered_buffer(addr) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(addr as *mut crate::buffer::BufferHeader);
    let keys = receiver.with_mut_ptr::<crate::buffer::BufferHeader, _>(|receiver| {
        registered_buffer_own_keys(receiver as usize)
    })?;
    let out = scope.root_raw_mut_ptr(crate::array::js_array_alloc(keys.len().max(1) as u32));
    for key in keys {
        let key_str = || crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        match what {
            MapSetEnum::Keys => {
                let key = scope.root_string_ptr(key_str());
                let next = out.with_mut_ptr::<ArrayHeader, _>(|out| {
                    key.with_mut_ptr::<crate::StringHeader, _>(|key| {
                        crate::array::js_array_push(out, JSValue::string_ptr(key))
                    })
                });
                out.set_raw_mut_ptr(next);
            }
            MapSetEnum::Values => {
                let value =
                    scope.root_nanbox_f64(receiver.with_mut_ptr::<crate::buffer::BufferHeader, _>(
                        |receiver| registered_buffer_own_value(receiver as usize, &key),
                    ));
                let next = out.with_mut_ptr::<ArrayHeader, _>(|out| {
                    crate::array::js_array_push_f64(out, value.get_nanbox_f64())
                });
                out.set_raw_mut_ptr(next);
            }
            MapSetEnum::Entries => {
                let pair = scope.root_raw_mut_ptr(crate::array::js_array_alloc(2));
                let key_value = scope.root_string_ptr(key_str());
                let next = pair.with_mut_ptr::<ArrayHeader, _>(|pair| {
                    key_value.with_mut_ptr::<crate::StringHeader, _>(|key| {
                        crate::array::js_array_push(pair, JSValue::string_ptr(key))
                    })
                });
                pair.set_raw_mut_ptr(next);
                let value =
                    scope.root_nanbox_f64(receiver.with_mut_ptr::<crate::buffer::BufferHeader, _>(
                        |receiver| registered_buffer_own_value(receiver as usize, &key),
                    ));
                let next = pair.with_mut_ptr::<ArrayHeader, _>(|pair| {
                    crate::array::js_array_push_f64(pair, value.get_nanbox_f64())
                });
                pair.set_raw_mut_ptr(next);
                let pair_value =
                    scope.root_nanbox_f64(pair.with_mut_ptr::<ArrayHeader, _>(|pair| {
                        crate::value::js_nanbox_pointer(pair as i64)
                    }));
                let next = out.with_mut_ptr::<ArrayHeader, _>(|out| {
                    crate::array::js_array_push(
                        out,
                        JSValue::from_bits(pair_value.get_nanbox_u64()),
                    )
                });
                out.set_raw_mut_ptr(next);
            }
        }
    }
    Some(out.with_mut_ptr::<ArrayHeader, _>(|out| out))
}
