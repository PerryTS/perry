//! Class read sites refill after real weak receiver-shape retirements.
use super::*;
use crate::object::{ObjectHeader, PicCache, PicCacheSlot};
use std::sync::atomic::AtomicU64;

struct ReadSite {
    packed: AtomicU64,
    slot: PicCacheSlot,
}
impl ReadSite {
    fn new() -> Self {
        Self {
            packed: AtomicU64::new(0xFFFF_FFFF),
            slot: std::ptr::null_mut(),
        }
    }
    unsafe fn read(&mut self, receiver: *mut ObjectHeader, key: &'static [u8]) -> u64 {
        let dir = std::ptr::addr_of!(crate::object::shapes::PERRY_EMPTY_SHAPE_DIR).cast::<u8>();
        let biased = (receiver as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
        let value = crate::object::field_get_set::js_object_get_field_ic_front(
            dir,
            biased,
            0,
            &mut self.slot,
            &self.packed,
        );
        if value.to_bits() != crate::value::TAG_HOLE {
            return value.to_bits();
        }
        let atom = crate::string::canonical_key(key);
        crate::object::field_get_set::js_object_get_field_ic_slow(
            receiver as i64,
            atom,
            &mut self.slot,
            &self.packed,
        )
        .to_bits()
    }
    fn primary(&mut self) -> u32 {
        unsafe {
            let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(&mut self.slot);
            crate::object::method_site::read_holder::class_read::test_shared_primary(&*cache)
        }
    }
    fn snapshot(&mut self) -> (usize, bool, u32) {
        unsafe {
            let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(&mut self.slot);
            crate::object::method_site::read_holder::class_read::test_retirement_snapshot(&*cache)
        }
    }
}

extern "C" fn method(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    7.0
}

fn receiver<'a>(
    scope: &'a RuntimeHandleScope,
    proto: &RuntimeHandle<'_>,
    own: &str,
) -> RuntimeHandle<'a> {
    const CID: u32 = 0x0C3C_9020;
    let r = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 4));
    r.with_mut_ptr::<ObjectHeader, _>(|r| {
        proto.with_const_ptr::<ObjectHeader, _>(|p| {
            crate::object::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(r as i64),
                crate::value::js_nanbox_pointer(p as i64),
            )
        })
    });
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        own.as_ptr(),
        own.len() as u32,
    ));
    r.with_mut_ptr::<ObjectHeader, _>(|r| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(r, k, 1.0))
    });
    r
}

#[test]
fn class_sites_refill_after_moving_gc_receiver_retirement() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "class_sites_refill_after_moving_gc_receiver_retirement",
    ) {
        return;
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    gc_init();
    let scope = RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(method, 0),
        0,
    ));
    let terminal = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 4));
    terminal.with_mut_ptr::<ObjectHeader, _>(|p| {
        closure.with_const_ptr(|c: *const crate::closure::ClosureHeader| {
            crate::object::js_object_set_field_by_name(
                p,
                crate::string::canonical_key(b"method"),
                crate::value::js_nanbox_pointer(c as i64),
            );
        })
    });
    let mut chain = vec![terminal];
    for _ in 0..8 {
        let hop = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        hop.with_mut_ptr::<ObjectHeader, _>(|p| {
            chain
                .last()
                .unwrap()
                .with_const_ptr::<ObjectHeader, _>(|next| {
                    crate::object::js_object_set_prototype_of(
                        crate::value::js_nanbox_pointer(p as i64),
                        crate::value::js_nanbox_pointer(next as i64),
                    )
                })
        });
        chain.push(hop);
    }
    let proto = chain.last().unwrap();
    let mut absent = ReadSite::new();
    let mut deep = ReadSite::new();
    let mut previous = 0;
    for generation in 0..24 {
        let retired = {
            let temporary = RuntimeHandleScope::new();
            // As in the ordinary-holder witness, use owned receiver keys.
            // The shape retirement below is asserted, never simulated.
            let r = receiver(&temporary, proto, &format!("own_{generation}"));
            let shape = r.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
                assert_eq!(absent.read(r, b"missing"), crate::value::TAG_UNDEFINED);
                let bits = closure.with_const_ptr(|c: *const crate::closure::ClosureHeader| {
                    crate::value::js_nanbox_pointer(c as i64).to_bits()
                });
                assert_eq!(deep.read(r, b"method"), bits);
                crate::object::shapes::object_shape_stamp(r)
            });
            assert_ne!(shape, previous);
            previous = shape;
            assert_eq!(
                absent.snapshot(),
                (1, false, 0),
                "expiry cannot arm sharing"
            );
            assert_eq!(
                deep.snapshot(),
                (1, false, 0),
                "expired deep ways are reclaimed"
            );
            let before = r.with_const_ptr(|r: *const ObjectHeader| r as usize);
            gc_collect_minor();
            let after = r.with_const_ptr(|r: *const ObjectHeader| r as usize);
            assert_ne!(before, after, "the witness must run a moving collection");
            r.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
                assert_eq!(absent.read(r, b"missing"), crate::value::TAG_UNDEFINED);
                let bits = closure.with_const_ptr(|c: *const crate::closure::ClosureHeader| {
                    crate::value::js_nanbox_pointer(c as i64).to_bits()
                });
                assert_eq!(deep.read(r, b"method"), bits);
            });
            shape
        };
        // The live moving minor left a carried-shape note. Rotate that
        // epoch, then run an exact full trace with no receiver owner.
        for _ in 0..2 {
            gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                GcTriggerKind::Direct,
            ));
        }
        assert!(
            crate::object::shapes::shape_record_by_id(retired).is_none(),
            "receiver ShapeIds must really retire: {:?}",
            crate::object::shapes::shape_descriptor_by_id(retired)
        );
    }

    // Genuine simultaneous polymorphism arms sharing. Keep the primary
    // receiver alive, retire the other 127, then admit a new shape at the
    // same full shared site.
    let mut shared = ReadSite::new();
    let anchor = scope.root_raw_mut_ptr(std::ptr::null_mut::<ObjectHeader>());
    let retired = {
        let temporary = RuntimeHandleScope::new();
        let mut receivers = Vec::new();
        for i in 0..128 {
            let r = receiver(&temporary, proto, &format!("own_{i}"));
            r.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
                assert_eq!(shared.read(r, b"missing"), crate::value::TAG_UNDEFINED);
            });
            receivers.push(r);
        }
        for r in &receivers {
            r.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
                assert_eq!(shared.read(r, b"missing"), crate::value::TAG_UNDEFINED);
            });
        }
        assert_eq!(
            shared.snapshot(),
            (1, true, 128),
            "exercise the full shared set"
        );
        let primary = shared.primary();
        let keep = receivers
            .iter()
            .position(|r| {
                r.with_const_ptr::<ObjectHeader, _>(|r| unsafe {
                    crate::object::shapes::object_shape_stamp(r) == primary
                })
            })
            .expect("the shared primary belongs to a live receiver");
        receivers[keep].with_mut_ptr::<ObjectHeader, _>(|r| anchor.set_raw_mut_ptr(r));
        gc_collect_minor();
        receivers
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != keep)
            .map(|(_, r)| {
                r.with_const_ptr::<ObjectHeader, _>(|r| unsafe {
                    crate::object::shapes::object_shape_stamp(r)
                })
            })
            .collect::<Vec<_>>()
    };
    for _ in 0..2 {
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    }
    for shape in retired {
        assert!(crate::object::shapes::shape_record_by_id(shape).is_none());
    }
    let fresh = receiver(&scope, proto, "own_new");
    fresh.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
        assert_eq!(shared.read(r, b"missing"), crate::value::TAG_UNDEFINED);
    });
    assert_eq!(
        shared.snapshot(),
        (1, true, 2),
        "full sets reclaim retired capacity"
    );
    anchor.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
        assert_eq!(shared.read(r, b"missing"), crate::value::TAG_UNDEFINED);
    });
    // Refill always reproves the chain: an own add on the terminal defeats
    // the old absence, and a method replacement is read from the holder.
    chain[0].with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::object::js_object_set_field_by_name(
            p,
            crate::string::canonical_key(b"missing"),
            42.0,
        );
        crate::object::js_object_set_field_by_name(p, crate::string::canonical_key(b"method"), 9.0);
    });
    fresh.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
        assert_eq!(shared.read(r, b"missing"), 42.0f64.to_bits());
        assert_eq!(deep.read(r, b"method"), 9.0f64.to_bits());
    });
}
