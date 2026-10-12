//! Constructor birth and key-add transition replay, validated by live ShapeIds.
//! Fixed layout and optional hook keys use the existing shape transition authority.
//! Instances retain the two-slot inline prefix and canonical overflow layout.
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Readable,
    Writable,
    Duplex,
    Transform,
    PassThrough,
}
impl Kind {
    pub(crate) fn constructor(self) -> (&'static str, &'static str) {
        match self {
            Self::Readable => ("Readable", "stream\0Readable"),
            Self::Writable => ("Writable", "stream\0Writable"),
            Self::Duplex => ("Duplex", "stream\0Duplex"),
            Self::Transform => ("Transform", "stream\0Transform"),
            Self::PassThrough => ("PassThrough", "stream\0PassThrough"),
        }
    }
}
thread_local! {
    static R_SHELL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static W_SHELL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static D_SHELL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static T_SHELL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static P_SHELL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
pub(crate) fn alloc_initialized_stream_shell(proto: f64, kind: Kind) -> f64 {
    let site = match kind {
        Kind::Readable => &R_SHELL,
        Kind::Writable => &W_SHELL,
        Kind::Duplex => &D_SHELL,
        Kind::Transform => &T_SHELL,
        Kind::PassThrough => &P_SHELL,
    };
    if let Some(obj) = crate::object::object_alloc_recorded_birth(
        site.with(std::cell::Cell::get),
        proto.to_bits(),
        0,
        2,
        false,
    ) {
        return box_pointer(obj.cast());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc_plain(2));
    object.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
        crate::object::prototype_chain::object_link_created_prototype(
            obj as usize,
            proto.get_nanbox_u64(),
        );
        site.with(|memo| memo.set(unsafe { crate::object::shapes::object_shape_stamp(obj) }));
        box_pointer(obj.cast())
    })
}
/// A site remembers a transition, never a key list or heap pointer. Every
/// use validates the receiver and each prototype's current ShapeId, as well
/// as the successor's presence in this agent's shape directory.
#[derive(Clone, Copy)]
struct BirthWrite {
    from: u32,
    to: u32,
    slot: u32,
    chain: [u32; 8],
    depth: usize,
}
pub(crate) struct BirthWriteSite(std::cell::Cell<Option<BirthWrite>>);
impl BirthWriteSite {
    pub(crate) const fn new() -> Self {
        Self(std::cell::Cell::new(None))
    }

    #[inline(never)]
    pub(crate) fn write(
        &self,
        stream: f64,
        key: crate::runtime_state_key::NamedStateKey,
        value: f64,
    ) {
        let obj = crate::object::field_get_set::runtime_read_site::object_receiver(stream)
            .expect("stream receiver");
        if unsafe { key.write_existing(obj, value) } {
            return;
        }
        let from = unsafe { crate::object::shapes::object_shape_stamp(obj) };
        if let Some(memo) = self.0.get().filter(|m| m.from == from) {
            if chain_shapes(stream, None)
                .is_some_and(|(chain, depth)| chain == memo.chain && depth == memo.depth)
            {
                if let Some(to) = crate::object::shapes::shape_descriptor_by_id(memo.to) {
                    // SAFETY: this site's exact historical successor is still
                    // present, and the complete predecessor guard matched.
                    if unsafe {
                        crate::object::shapes::install_cached_object_shape_transition(
                            obj,
                            from,
                            memo.to,
                            to.keys_view(),
                        )
                    } {
                        if memo.slot < to.live_inline_slot_count {
                            unsafe {
                                crate::object::store_object_field_slot(
                                    obj,
                                    memo.slot as usize,
                                    value.to_bits(),
                                )
                            };
                        } else {
                            crate::object::overflow_set(
                                obj as usize,
                                memo.slot as usize,
                                value.to_bits(),
                            );
                        }
                        return;
                    }
                }
            }
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let stream = scope.root_nanbox_f64(stream);
        let value = scope.root_nanbox_f64(value);
        let proof = chain_shapes(stream.get_nanbox_f64(), Some(key.bytes));
        ordinary_birth_set(stream.get_nanbox_f64(), key, value.get_nanbox_f64());
        let obj = object_ptr_from_value(stream.get_nanbox_f64()).unwrap();
        let to = unsafe { crate::object::shapes::object_shape_stamp(obj) };
        if let (Some((chain, depth)), Some(record)) =
            (proof, crate::object::shapes::shape_record_by_id(to))
        {
            if let Some(slot) = unsafe {
                let keys = record.keys() as *mut crate::array::ArrayHeader;
                crate::object::keys_find_property_slot_by_bytes(
                    keys,
                    record.logical_key_count(),
                    key.bytes,
                )
            } {
                let descriptor = crate::object::shapes::shape_descriptor_by_id(to).unwrap();
                let entry = unsafe {
                    crate::object::key_attrs::keys_entry(descriptor.keys_view().arr(), slot)
                };
                if descriptor.integrity_flags != 0
                    || !crate::object::key_attrs::entry_is_plain_writable_data(entry)
                {
                    return;
                }
                self.0.set(Some(BirthWrite {
                    from,
                    to,
                    slot,
                    chain,
                    depth,
                }));
            }
        }
    }
}

/// Validate the ordered chain without keeping heap addresses. A setter or
/// non-writable data property on the key refuses birth replay.
pub(super) fn chain_shapes(stream: f64, key: Option<&[u8]>) -> Option<([u32; 8], usize)> {
    let _no_move = crate::gc::GcSuppressScope::new();
    let obj = object_ptr_from_value(stream)?;
    let mut bits = unsafe { crate::object::shapes::object_prototype_word(obj) };
    if bits == 0 {
        bits = crate::object::js_object_get_prototype_of(stream).to_bits();
    }
    walk_shapes(bits, key)
}
pub(super) fn walk_shapes(mut bits: u64, key: Option<&[u8]>) -> Option<([u32; 8], usize)> {
    let _no_move = crate::gc::GcSuppressScope::new();
    let mut chain = [0; 8];
    let mut depth = 0;
    while bits != TAG_NULL && bits != 0 && bits != TAG_UNDEFINED {
        let proto = object_ptr_from_value(f64::from_bits(bits))?;
        if depth == chain.len() {
            return None;
        }
        let shape = unsafe { crate::object::shapes::object_shape_stamp(proto) };
        let record = crate::object::shapes::shape_descriptor_by_id(shape)?;
        chain[depth] = shape;
        depth += 1;
        let keys = record.keys_view();
        if let Some(key) = key {
            if let Some(slot) = unsafe {
                crate::object::keys_find_slot_by_bytes_resolved(keys.arr(), keys.count(), key)
            } {
                let entry = unsafe { crate::object::key_attrs::keys_entry(keys.arr(), slot) };
                if !crate::object::key_attrs::entry_is_plain_writable_data(entry) {
                    return None;
                }
            }
        }
        bits = unsafe { crate::object::shapes::object_prototype_word(proto) };
        if bits == 0 {
            bits = crate::object::js_object_get_prototype_of(f64::from_bits(
                crate::value::JSValue::pointer(proto.cast()).bits(),
            ))
            .to_bits();
        }
    }
    Some((chain, depth))
}

macro_rules! stream_birth_field {
    ($stream:expr, $key:expr, $value:expr $(,)?) => {{
        thread_local! {
            static SITE: $crate::node_stream::constructors::birth::BirthWriteSite =
                const { $crate::node_stream::constructors::birth::BirthWriteSite::new() };
        }
        SITE.with(|site| site.write($stream, $key, $value));
    }};
}
pub(crate) use stream_birth_field;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_has_node_state_layout_and_bounded_inline_prefix() {
        let scope = crate::gc::RuntimeHandleScope::new();
        for (name, readable, writable) in [
            ("Readable", true, false),
            ("Writable", false, true),
            ("Duplex", true, true),
        ] {
            let stream = scope.root_nanbox_f64(match name {
                "Readable" => js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED)),
                "Writable" => js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED)),
                _ => js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED)),
            });
            assert_eq!(
                crate::object::js_object_get_prototype_of(stream.get_nanbox_f64()).to_bits(),
                proto_methods::stream_prototype_value(name).to_bits(),
            );
            for (key, present) in [
                (b"_readableState".as_slice(), readable),
                (b"_writableState".as_slice(), writable),
            ] {
                assert_eq!(
                    get_hidden_value(stream.get_nanbox_f64(), hidden_key(key)).is_some(),
                    present
                );
            }
            let obj = object_ptr_from_value(stream.get_nanbox_f64()).unwrap();
            assert_eq!(
                unsafe { crate::object::object_live_slot_count(obj) },
                2,
                "fixed births preserve the stream inline prefix"
            );
            let keys = unsafe { crate::object::object_keys(obj) };
            let own: Vec<_> = (0..keys.count())
                .map(|i| {
                    crate::fs::validate::read_js_string_pub(f64::from_bits(unsafe {
                        keys.get(i).bits()
                    }))
                })
                .collect();
            let expected = if name == "Writable" {
                vec!["_events", "_writableState", "_maxListeners"]
            } else if name == "Readable" {
                vec!["_events", "_readableState", "_maxListeners"]
            } else {
                vec![
                    "_events",
                    "_readableState",
                    "_writableState",
                    "allowHalfOpen",
                    "_maxListeners",
                ]
            };
            assert_eq!(own, expected);
        }
    }

    #[test]
    fn primitive_prototype_fallback_keeps_state_storage() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let ctor = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
            "stream", "Readable",
        ));
        let prototype = scope.root_nanbox_f64(proto_methods::stream_prototype_value("Readable"));
        unsafe {
            crate::closure::props::bag_set(
                raw_ptr_from_value(ctor.get_nanbox_f64()),
                "prototype",
                f64::from_bits(TAG_NULL),
            );
        }
        assert_eq!(
            proto_methods::stream_prototype_value("Readable").to_bits(),
            TAG_NULL
        );
        let stream =
            scope.root_nanbox_f64(js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED)));
        assert_eq!(
            read_slot(stream.get_nanbox_f64(), Slot::ReadableHwm),
            Some(default_hwm(false))
        );
        let obj = object_ptr_from_value(stream.get_nanbox_f64()).unwrap();
        assert_eq!(unsafe { crate::object::object_live_slot_count(obj) }, 2);
        unsafe {
            crate::closure::props::bag_set(
                raw_ptr_from_value(ctor.get_nanbox_f64()),
                "prototype",
                prototype.get_nanbox_f64(),
            );
        }
    }
}

/// The shared ordinary Set operation for every constructor field. The engine's
/// OrdinarySetWithReceiver authority honors inherited accessors and readonly
/// data; an own-slot proof merely replaces that same operation.
#[inline(never)]
pub(crate) fn ordinary_birth_set(
    stream: f64,
    key: crate::runtime_state_key::NamedStateKey,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let value = scope.root_nanbox_f64(value);
    let name = key.bytes;
    let key = crate::string::intern_ascii_literal(name);
    let key = f64::from_bits(JSValue::string_ptr(key as *mut _).bits());
    let result = crate::proxy::reflect_ordinary_set_with_receiver(
        stream.get_nanbox_f64(),
        key,
        value.get_nanbox_f64(),
        stream.get_nanbox_f64(),
    );
    if result.to_bits() == TAG_FALSE {
        crate::collection_iter::throw_type_error(&format!(
            "Cannot assign to property '{}'",
            String::from_utf8_lossy(name)
        ));
    }
}
