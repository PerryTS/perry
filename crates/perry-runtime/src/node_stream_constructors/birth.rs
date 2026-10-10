//! A direct stream is born with its two lifecycle data slots. Option
//! reads and hook installation still run in the ordinary constructor order.
//! The canonical keys trie owns this layout, as it owns function bags: no
//! construction cache or remembered final shape. Per-side public fields keep
//! the existing overflow layout: widening them inline slows stream unpacking.
use super::*;

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum PublicField {
    Destroyed,
    Closed,
}
impl PublicField {
    fn name(self) -> &'static str {
        match self {
            Self::Destroyed => "destroyed",
            Self::Closed => "closed",
        }
    }
}

const COMMON: &[PublicField] = &[PublicField::Destroyed, PublicField::Closed];
/// A direct constructor owns fresh data slots and can initialize them by
/// index. Subclasses retain ordinary property writes: user hooks and option
/// getters can redefine their fields while the constructor runs.
#[derive(Clone, Copy)]
pub(super) struct PublicInit {
    direct: bool,
}
impl PublicInit {
    pub(super) fn new(how: super::builders::StreamInit) -> Self {
        Self {
            direct: how == super::builders::StreamInit::Direct,
        }
    }
    pub(super) fn set(self, stream: f64, field: PublicField, value: f64) {
        if self.direct {
            let index = field as usize;
            let obj =
                object_ptr_from_value(stream).expect("a direct stream has its born data layout");
            // No allocation or user call occurs between the born receiver
            // proof and this write. The normal slot writer preserves tracing.
            unsafe {
                crate::object::store_object_field_slot(obj, index, value.to_bits());
            }
        } else {
            let key = match field {
                PublicField::Destroyed => crate::runtime_state_key!(b"destroyed"),
                PublicField::Closed => crate::runtime_state_key!(b"closed"),
            };
            set_hidden_value(stream, key, value);
        }
    }
}

pub(crate) fn alloc_initialized_stream_shell(proto: f64) -> f64 {
    let _no_move = crate::gc::GcSuppressScope::new();
    let hidden = crate::object::key_attrs::attr_bits_to_entry(
        crate::object::PropertyAttrs::new(true, false, true).bits,
    );
    const COUNT: usize = COMMON.len();
    let mut entries = [("", f64::from_bits(TAG_UNDEFINED)); COUNT];
    let attrs = [hidden; COUNT];
    let mut count = 0;
    for &key in COMMON {
        entries[count].0 = key.name();
        count += 1;
    }
    let obj = unsafe {
        crate::object::alloc::object_alloc_null_proto_with_key_attrs(
            &entries[..count],
            &attrs[..count],
        )
    };
    crate::object::prototype_chain::object_link_created_prototype(obj as usize, proto.to_bits());
    box_pointer(obj as *const u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_has_attributed_state_slots() {
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
            let keys = unsafe { crate::object::object_keys(obj) };
            let destroyed = (0..keys.count())
                .find(|&i| unsafe {
                    string_value_eq(f64::from_bits(keys.get(i).bits()), b"destroyed")
                })
                .unwrap();
            let entry = unsafe { crate::object::key_attrs::keys_entry(keys.arr(), destroyed) };
            assert_eq!(
                entry,
                crate::object::key_attrs::attr_bits_to_entry(
                    crate::object::PropertyAttrs::new(true, false, true).bits
                )
            );
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
            get_hidden_value(
                stream.get_nanbox_f64(),
                hidden_key(b"readableHighWaterMark")
            ),
            Some(default_hwm(false))
        );
        let obj = object_ptr_from_value(stream.get_nanbox_f64()).unwrap();
        assert!(unsafe { crate::object::object_live_slot_count(obj) } >= COMMON.len() as u32);
        unsafe {
            crate::closure::props::bag_set(
                raw_ptr_from_value(ctor.get_nanbox_f64()),
                "prototype",
                prototype.get_nanbox_f64(),
            );
        }
    }
}
