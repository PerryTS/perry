//! Constructor birth and key-add transition replay, validated by live ShapeIds.
//! Fixed layout and optional hook keys use the existing shape transition authority.
//! Instances retain the two-slot inline prefix and canonical overflow layout.
use super::*;
use crate::codegen_abi::LayoutWord;

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

    /// The instance layout a direct birth of this class is born on
    /// (`runtime_layouts::stream_instances`).
    pub(crate) fn layout(self) -> *const crate::object::layout_desc::PerryLayoutDesc {
        use crate::object::runtime_layouts as rl;
        match self {
            Self::Readable => &raw const rl::READABLE,
            Self::Writable => &raw const rl::WRITABLE,
            Self::Duplex => &raw const rl::DUPLEX,
            Self::Transform => &raw const rl::TRANSFORM,
            Self::PassThrough => &raw const rl::PASSTHROUGH,
        }
    }
}
/// A site remembers a transition, never a key list or heap pointer. Every
/// use validates the receiver and each prototype's current ShapeId, as well
/// as the successor's presence in this agent's shape directory.
#[derive(Clone, Copy)]
struct BirthWrite {
    from: LayoutWord,
    to: LayoutWord,
    slot: u32,
    chain: [LayoutWord; 8],
    depth: usize,
}
pub(crate) struct BirthWriteSite(std::cell::Cell<Option<BirthWrite>>, std::cell::Cell<bool>);
crate::perry_thread_local! {
    /// Sites of this thread that ever held a memo (thread-local statics live as
    /// long as the thread, so the pointers stay valid for every recompute).
    static WRITE_SITES: std::cell::RefCell<Vec<*const BirthWriteSite>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
/// Claim the successor word of every write memo (full-trace recompute): the
/// memo names a row by word and reads it, so the row must not retire.
pub(crate) fn note_write_site_carriers() {
    WRITE_SITES.with(|sites| {
        for &site in sites.borrow().iter() {
            // SAFETY: registered from a live thread-local of this thread.
            if let Some(memo) = unsafe { (*site).0.get() } {
                crate::object::shape_carriers::note_shape_id(memo.to);
            }
        }
    });
}
impl BirthWriteSite {
    pub(crate) const fn new() -> Self {
        Self(std::cell::Cell::new(None), std::cell::Cell::new(false))
    }

    /// Remember a transition: the successor row is claimed now and re-claimed
    /// after every full trace (`note_write_site_carriers`), because `write`
    /// reads it by word on the next birth.
    fn set_memo(&self, memo: BirthWrite) {
        crate::object::shape_carriers::note_shape_id(memo.to);
        if !self.1.replace(true) {
            WRITE_SITES.with(|s| s.borrow_mut().push(self as *const _));
        }
        self.0.set(Some(memo));
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
        // SAFETY: a live receiver.
        if unsafe { super::instance_birth::layout_birth_store(obj, key.bytes, value) } {
            return;
        }
        let from = unsafe { crate::object::shapes::object_shape_stamp(obj) };
        if let Some(memo) = self.0.get().filter(|m| m.from == from) {
            if chain_shapes(stream, None)
                .is_some_and(|(chain, depth)| chain == memo.chain && depth == memo.depth)
            {
                if let Some(to) = crate::object::shapes::shape_descriptor_for_word(memo.to) {
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
            (proof, crate::object::shapes::shape_record_for_word(to))
        {
            if let Some(slot) = unsafe {
                let keys = record.keys() as *mut crate::array::ArrayHeader;
                crate::object::keys_find_property_slot_by_bytes(
                    keys,
                    record.logical_key_count(),
                    key.bytes,
                )
            } {
                let descriptor = crate::object::shapes::shape_descriptor_for_word(to).unwrap();
                let entry = unsafe {
                    crate::object::key_attrs::keys_entry(descriptor.keys_view().arr(), slot)
                };
                if descriptor.integrity_flags != 0
                    || !crate::object::key_attrs::entry_is_plain_writable_data(entry)
                {
                    return;
                }
                self.set_memo(BirthWrite {
                    from,
                    to,
                    slot,
                    chain,
                    depth,
                });
            }
        }
    }
}

/// Validate the ordered chain without keeping heap addresses. A setter or
/// non-writable data property on the key refuses birth replay.
pub(super) fn chain_shapes(stream: f64, key: Option<&[u8]>) -> Option<([LayoutWord; 8], usize)> {
    let _no_move = crate::gc::GcSuppressScope::new();
    let obj = object_ptr_from_value(stream)?;
    let mut bits = unsafe { crate::object::shapes::object_prototype_word(obj) };
    if bits == 0 {
        bits = crate::object::js_object_get_prototype_of(stream).to_bits();
    }
    walk_shapes(bits, key)
}
pub(super) fn walk_shapes(mut bits: u64, key: Option<&[u8]>) -> Option<([LayoutWord; 8], usize)> {
    let _no_move = crate::gc::GcSuppressScope::new();
    let mut chain = [LayoutWord(0); 8];
    let mut depth = 0;
    while bits != TAG_NULL && bits != 0 && bits != TAG_UNDEFINED {
        let proto = object_ptr_from_value(f64::from_bits(bits))?;
        if depth == chain.len() {
            return None;
        }
        let shape = unsafe { crate::object::shapes::object_shape_stamp(proto) };
        let record = crate::object::shapes::shape_descriptor_for_word(shape)?;
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

    /// F2 #3: a write memo's successor word survives full traces with no
    /// receiver on it; fails if the recompute does not re-note it.
    #[test]
    fn write_memo_successor_is_carried_across_full_traces() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let site = BirthWriteSite::new();
        let to = {
            let obj = crate::object::js_object_alloc(0, 0);
            unsafe { crate::object::shapes::transition_object_shape_semantics(obj) }
        };
        site.set_memo(BirthWrite {
            from: LayoutWord(0),
            to,
            slot: 0,
            chain: [LayoutWord(0); 8],
            depth: 0,
        });
        let control = {
            let obj = crate::object::js_object_alloc(0, 0);
            unsafe { crate::object::shapes::transition_object_shape_semantics(obj) }
        };
        for round in 0..3 {
            crate::gc::test_collect_synchronous_full_trace();
            assert!(
                crate::object::shapes::shape_is_retired(control),
                "control must retire"
            );
            assert!(
                !crate::object::shapes::shape_is_retired(to),
                "round {round}: write memo successor retired"
            );
            assert!(crate::object::shapes::shape_descriptor_for_word(to).is_some());
        }
    }

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
            let kind = match name {
                "Readable" => Kind::Readable,
                "Writable" => Kind::Writable,
                _ => Kind::Duplex,
            };
            assert_eq!(
                unsafe { (*obj).parent_class_id },
                unsafe { (*kind.layout()).layout_id },
                "a direct birth is born on its instance layout"
            );
            let keys = unsafe { crate::object::object_keys(obj) };
            // The present keys: an optional member's hole is absent.
            let own: Vec<_> = (0..keys.count())
                .filter(|&i| unsafe { !crate::object::key_view::position_is_hole(obj, keys, i) })
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
            read_state_data(stream.get_nanbox_f64(), StateData::ReadableHwm),
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
