// Birth and population of the node prototypes declared in
// `node_prototypes.rs`. A prototype is born on its layout (old, never moving,
// every slot the hole), each builder fills every declared slot in place, and
// `seal_node_prototype` checks the layout is complete before it seals the
// prototype with its reserved serial. Population names a slot by its key's
// position in the static key list: no key is added, no lookup is consulted.

/// A node prototype being populated: the object and its static layout.
pub(crate) struct NodePrototype {
    desc: *const PerryLayoutDesc,
}

impl NodePrototype {
    /// The layout `desc` names.
    ///
    /// # Safety
    /// `desc` is one of the node prototype descriptors of this file.
    pub(crate) unsafe fn of(desc: *const PerryLayoutDesc) -> Self {
        NodePrototype { desc }
    }

    /// A prototype born on the layout: old, never moving, every slot the hole.
    ///
    /// # Safety
    /// As [`super::layout_desc::alloc_layout_prototype`].
    pub(crate) unsafe fn birth(&self) -> *mut crate::object::ObjectHeader {
        super::layout_desc::alloc_layout_prototype(self.desc, 0)
    }

    unsafe fn keys(&self) -> &'static [PerryLayoutKey] {
        std::slice::from_raw_parts((*self.desc).keys, (*self.desc).key_count as usize)
    }

    /// The storage slot of the string key `name`.
    unsafe fn slot_of_name(&self, name: &[u8]) -> u32 {
        for key in self.keys() {
            let atom = key.atom;
            if (*atom).kind == crate::codegen_abi::ATOM_KIND_STRING {
                let s = (*atom).string;
                let text = std::slice::from_raw_parts(
                    crate::string::string_data(s),
                    (*s).byte_len as usize,
                );
                if text == name {
                    return key.slot;
                }
            }
        }
        layout_error(self.desc, name)
    }

    /// The storage slot of the symbol key `atom`.
    unsafe fn slot_of_atom(&self, atom: *const StaticAtom) -> u32 {
        for key in self.keys() {
            if std::ptr::eq(key.atom, atom) {
                return key.slot;
            }
        }
        layout_error(self.desc, b"<symbol>")
    }

    /// Store data `value` in `name`'s slot of `proto`.
    ///
    /// # Safety
    /// `proto` is a live prototype born on this layout.
    pub(crate) unsafe fn data(
        &self,
        proto: *mut crate::object::ObjectHeader,
        name: &[u8],
        value: f64,
    ) {
        let slot = self.slot_of_name(name);
        crate::object::js_object_set_field(proto, slot, crate::JSValue::from_bits(value.to_bits()));
    }

    /// Store data `value` in the slot of the symbol key `atom`.
    ///
    /// # Safety
    /// As [`Self::data`].
    pub(crate) unsafe fn symbol_data(
        &self,
        proto: *mut crate::object::ObjectHeader,
        atom: *const StaticAtom,
        value: f64,
    ) {
        let slot = self.slot_of_atom(atom);
        crate::object::js_object_set_field(proto, slot, crate::JSValue::from_bits(value.to_bits()));
    }

    /// Store the accessor pair (`get`, `set`: NaN-boxed closures, 0 when
    /// absent) in `name`'s slot.
    ///
    /// # Safety
    /// As [`Self::data`]; the key is an accessor of the layout.
    pub(crate) unsafe fn accessor(
        &self,
        proto: *mut crate::object::ObjectHeader,
        name: &[u8],
        get: u64,
        set: u64,
    ) {
        let slot = self.slot_of_name(name);
        let _no_move = crate::gc::GcSuppressScope::new();
        let pair = crate::object::accessor_pair::pair_new(crate::object::accessor_pair::Accessor {
            get,
            set,
            ..Default::default()
        });
        crate::object::js_object_set_field(
            proto,
            slot,
            crate::JSValue::from_bits(crate::value::js_nanbox_pointer(pair as i64).to_bits()),
        );
    }

    /// Check every declared slot is filled, then seal `proto` with its reserved
    /// `serial`: from now on every write sets its shadow bit, and the serial's
    /// identity word names it, which is how the layouts of its children and
    /// its instances reach it.
    ///
    /// # Safety
    /// `proto` is a live prototype born on this layout, fully populated, not
    /// yet reachable by program code.
    pub(crate) unsafe fn seal(&self, proto: *mut crate::object::ObjectHeader, serial: u64) {
        for key in self.keys() {
            if *crate::object::header::object_slot_at(proto, u32::from(key.off))
                == crate::value::TAG_HOLE
            {
                let atom = key.atom;
                let name = if (*atom).string.is_null() {
                    b"<symbol>".as_slice()
                } else {
                    std::slice::from_raw_parts(
                        crate::string::string_data((*atom).string),
                        (*(*atom).string).byte_len as usize,
                    )
                };
                eprintln!(
                    "Perry internal error: node prototype (family {:#x}) left `{}` unfilled",
                    (*self.desc).family,
                    String::from_utf8_lossy(name)
                );
                std::process::abort();
            }
        }
        // Population is the runtime's definition, not a program write: its
        // slot stores set the shadow bit when the program has a site naming
        // one of the keys (`shadow::note_prototype_slot_store`), but no
        // program code can have seen the prototype yet. The sealed word
        // starts unshadowed unless an overflow holds keys (the bit names it).
        if !crate::object::overflow::has_overflow(proto) {
            (*proto).parent_class_id &= !crate::object::layout_desc::LAYOUT_SHADOW_BIT;
        }
        crate::object::builtin_proto::seal_runtime_prototype(proto, serial);
    }
}

#[cold]
unsafe fn layout_error(desc: *const PerryLayoutDesc, name: &[u8]) -> ! {
    eprintln!(
        "Perry internal error: node prototype (family {:#x}) has no key `{}`",
        (*desc).family,
        String::from_utf8_lossy(name)
    );
    std::process::abort();
}

/// The prototype of node prototype family `serial` this agent has built, if
/// any: its serial's identity word.
pub(crate) fn node_prototype_of_serial(serial: u64) -> Option<*mut crate::object::ObjectHeader> {
    let slot = crate::object::shapes::identity_word_slot(serial)?;
    // SAFETY: the agent's own identity word.
    let bits = unsafe { *slot };
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    Some((bits & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader)
}

#[cfg(test)]
mod tests {
    //! A sealed node prototype is answered from its layout only while no
    //! program write reached it: the compiled store funnel
    //! (`js_object_set_field_by_name`) sets its shadow bit, and the probe
    //! then reads the written slot or the overflow, or declines; it never
    //! answers past a key the write added.
    use crate::object::ObjectHeader;
    use crate::value::{POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_HOLE};

    fn on_a_fresh_thread(body: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(body)
            .expect("spawn node-prototype test thread")
            .join()
            .expect("node-prototype test thread panicked");
    }

    unsafe fn atom(key: &str) -> u64 {
        let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
        let a = crate::string::js_string_pool_atom(key.as_ptr(), key.len() as u32, hash, 0);
        STRING_TAG | a as u64
    }

    unsafe fn probe(obj: usize, key: &str) -> u64 {
        let bits = atom(key);
        let hash = crate::string::property_atom_hash((bits & POINTER_MASK) as *const _);
        crate::object::layout_probe::perry_probe_get(
            crate::object::layout_probe::test_dir(),
            obj.wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
            bits,
            hash,
        )
        .to_bits()
    }

    unsafe fn get(obj: usize, key: &str) -> u64 {
        let boxed = f64::from_bits(POINTER_TAG | obj as u64);
        crate::proxy::js_reflect_get(boxed, f64::from_bits(atom(key)), boxed).to_bits()
    }

    unsafe fn set(obj: usize, key: &str, value: f64) {
        crate::object::js_object_set_field_by_name(
            obj as *mut ObjectHeader,
            (atom(key) & POINTER_MASK) as *const crate::StringHeader,
            value,
        );
    }

    #[test]
    fn a_compiled_store_shadows_the_sealed_event_emitter_prototype() {
        on_a_fresh_thread(|| unsafe {
            let _no_move = crate::gc::GcSuppressScope::new();
            let proto_bits =
                crate::node_stream::event_emitter_prototype_value("EventEmitter").to_bits();
            assert_eq!(proto_bits & !POINTER_MASK, POINTER_TAG);
            let ee = (proto_bits & POINTER_MASK) as usize;
            let _ = crate::object::bound_native_callable_export_value("stream", "Readable");
            let stream = super::node_prototype_of_serial(
                crate::closure::shape::INTRINSIC_SERIAL_STREAM_PROTOTYPE,
            )
            .expect("Stream.prototype is built with the stream module")
                as usize;
            let word = |p: usize| (*(p as *const ObjectHeader)).parent_class_id;
            for p in [ee, stream] {
                assert!(!crate::object::layout_desc::word_is_shadowed(word(p)));
            }
            let child = |p: usize| {
                let v = crate::object::js_object_create(f64::from_bits(POINTER_TAG | p as u64));
                (v.to_bits() & POINTER_MASK) as usize
            };
            let (on_ee, on_stream) = (child(ee), child(stream));

            // Pristine: the probe answers `emit` through the sealed layouts,
            // from EventEmitter.prototype (one and two hops up).
            let emit = get(on_ee, "emit");
            for o in [on_ee, on_stream] {
                assert_eq!(probe(o, "emit"), emit, "the probe walks the sealed layouts");
            }

            // A compiled plain store to EventEmitter.prototype.emit (its own
            // slot, in place) sets its shadow bit; the probe reads the slot
            // it wrote, or declines.
            set(ee, "emit", 7.0);
            assert!(
                crate::object::layout_desc::word_is_shadowed(word(ee)),
                "a program store to a sealed runtime prototype sets its shadow bit"
            );
            assert_eq!(get(on_ee, "emit"), 7.0f64.to_bits());
            let p = probe(on_ee, "emit");
            assert!(
                p == TAG_HOLE || p == 7.0f64.to_bits(),
                "probe answered {p:#x}"
            );
            set(ee, "emit", f64::from_bits(emit));

            // A store of a key Stream.prototype's layout lacks lands in its
            // overflow and shadows EventEmitter.prototype.emit: a probe that
            // walked Stream's layout past it would answer the stale `emit`.
            set(stream, "emit", 8.0);
            assert!(crate::object::layout_desc::word_is_shadowed(word(stream)));
            assert_eq!(get(on_stream, "emit"), 8.0f64.to_bits());
            let p = probe(on_stream, "emit");
            assert!(
                p != emit,
                "the probe answered the shadowed EventEmitter.prototype.emit"
            );
            assert!(
                p == TAG_HOLE || p == 8.0f64.to_bits(),
                "probe answered {p:#x}"
            );
        });
    }
}
