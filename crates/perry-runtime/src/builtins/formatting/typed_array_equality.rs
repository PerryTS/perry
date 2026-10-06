struct TypedArrayView<'s> {
    kind: u8,
    data: &'s [u8],
}

fn value_pointer_addr(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let jv = crate::value::JSValue::from_bits(bits);
    if jv.is_pointer() {
        return Some(jv.as_pointer::<u8>() as usize);
    }
    // #10694: a raw word must be allocator-owned before a brand probe reads
    // its header.
    if bits > 0x1000 && (bits >> 48) == 0 && crate::buffer::header_is_owned(bits as usize) {
        return Some(bits as usize);
    }
    None
}

fn typed_array_view<'s>(
    value: f64,
    scope: &'s crate::buffer::bytes::NoGc<'s>,
) -> Option<TypedArrayView<'s>> {
    let addr = value_pointer_addr(value)?;
    let kind = if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
        kind
    } else if crate::buffer::is_registered_buffer(addr) && crate::buffer::is_uint8array_buffer(addr)
    {
        crate::typedarray::KIND_UINT8
    } else {
        return None;
    };
    let data =
        crate::buffer::bytes::bytes(crate::value::js_nanbox_pointer(addr as i64), scope).ok()?;
    Some(TypedArrayView { kind, data })
}

pub(super) fn deep_strict_typed_array_equal(left: f64, right: f64) -> Option<bool> {
    crate::buffer::bytes::no_gc(|scope| {
        match (
            typed_array_view(left, scope),
            typed_array_view(right, scope),
        ) {
            (Some(left), Some(right)) => Some(left.kind == right.kind && left.data == right.data),
            (Some(_), None) | (None, Some(_)) => Some(false),
            (None, None) => None,
        }
    })
}
