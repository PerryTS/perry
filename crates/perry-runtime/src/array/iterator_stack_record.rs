//! Cold protocol and completion operations over a caller-owned GC stack range.
use crate::value::TAG_UNDEFINED;

/// All six words are mutable roots in the caller's native home map (or its
/// shadow frame). Numeric words decode to no heap pointer. Never keep a Rust
/// reference or pre-collection copy of a field across reentry.
#[repr(C)]
pub struct ArrayStackRecord {
    pub payload: f64,
    pub next: f64,
    pub index: f64,
    pub state: f64,
    pub protocol: f64,
    pub value: f64,
}

/// Release before a reentrant call, after its arguments have been placed in
/// RuntimeHandleScope. Perry's JS unwinder skips Rust cleanups, so neither Drop
/// nor a post-call clear can own the escaping path.
unsafe fn clear_record(record: *mut ArrayStackRecord) {
    let absent = f64::from_bits(TAG_UNDEFINED);
    // GC_STORE_AUDIT(STACK): transfer from the caller range to rooted call arguments.
    std::ptr::write_volatile(std::ptr::addr_of_mut!((*record).payload), absent);
    // GC_STORE_AUDIT(STACK): transfer from the caller range to rooted call arguments.
    std::ptr::write_volatile(std::ptr::addr_of_mut!((*record).next), absent);
    // GC_STORE_AUDIT(STACK): transfer from the caller range to rooted call arguments.
    std::ptr::write_volatile(std::ptr::addr_of_mut!((*record).value), absent);
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_array_record_stack_dispatch(
    record: *mut ArrayStackRecord,
    op: u32,
) -> f64 {
    macro_rules! read {
        ($field:ident) => {
            std::ptr::read_volatile(std::ptr::addr_of!((*record).$field))
        };
    }
    macro_rules! write {
        ($field:ident, $value:expr) => {
            // GC_STORE_AUDIT(STACK): the caller publishes this whole home range.
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*record).$field), $value)
        };
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(read!(payload));
    // Completion may need an error; Capture/Step need captured next instead.
    let auxiliary = scope.root_nanbox_f64(if op == 4 { read!(value) } else { read!(next) });
    let index = read!(index);
    let state = read!(state);
    let protocol = read!(protocol);
    clear_record(record);
    match op {
        0 => {
            let iter =
                scope.root_nanbox_f64(crate::symbol::js_get_iterator(payload.get_nanbox_f64()));
            let next = super::iterator_step::js_iterator_next_method(iter.get_nanbox_f64());
            write!(payload, iter.get_nanbox_f64());
            write!(next, next);
        }
        1 | 6 if op == 1 || protocol != 0.0 => {
            let done = super::iterator_step::js_iterator_step(
                payload.get_nanbox_f64(),
                auxiliary.get_nanbox_f64(),
                std::ptr::addr_of_mut!((*record).value),
            );
            if done == 0 {
                write!(payload, payload.get_nanbox_f64());
                write!(next, auxiliary.get_nanbox_f64());
            }
            return f64::from_bits(if done != 0 {
                crate::value::TAG_TRUE
            } else {
                crate::value::TAG_FALSE
            });
        }
        2 | 4 => {
            let flags = u32::from(protocol != 0.0)
                | (u32::from(state == 2.0) << 1)
                | (u32::from(op == 4) << 2);
            super::iterator_record_cleanup::js_array_record_finish(
                payload.get_nanbox_f64(),
                index,
                if op == 4 {
                    auxiliary.get_nanbox_f64()
                } else {
                    f64::from_bits(TAG_UNDEFINED)
                },
                flags,
            );
        }
        6 => {
            let raw = crate::value::js_nanbox_get_pointer(payload.get_nanbox_f64())
                as *const super::ArrayHeader;
            let len = super::js_array_length(raw);
            if index >= len as f64 {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
            let raw = crate::value::js_nanbox_get_pointer(payload.get_nanbox_f64())
                as *const super::ArrayHeader;
            let value = super::js_array_get_f64(raw, index as u32);
            let current = super::clean_arr_ptr(crate::value::js_nanbox_get_pointer(
                payload.get_nanbox_f64(),
            ) as *const super::ArrayHeader);
            write!(payload, crate::value::js_nanbox_pointer(current as i64));
            write!(value, value);
            return f64::from_bits(crate::value::TAG_FALSE);
        }
        5 => {
            let value = super::js_array_get_f64(
                crate::value::js_nanbox_get_pointer(payload.get_nanbox_f64())
                    as *const super::ArrayHeader,
                index as u32,
            );
            // The counted loop continues from its sole mutable payload home.
            write!(payload, payload.get_nanbox_f64());
            return value;
        }
        3 => {
            super::iterator_record_cleanup::js_array_record_abrupt(
                payload.get_nanbox_f64(),
                index,
                state,
                u32::from(protocol != 0.0),
            );
        }
        _ => unreachable!("compiler-owned stack-record operation"),
    }

    f64::from_bits(TAG_UNDEFINED)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ARRAY_STACK_DISPATCH: unsafe extern "C-unwind" fn(*mut ArrayStackRecord, u32) -> f64 =
    js_array_record_stack_dispatch;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn array_stack_record_fields_are_rewritten_by_moving_collection() {
        let _nursery = crate::gc::CopyingNurseryTestGuard::new(6);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        let _verify = crate::gc::knob_overrides::VerifyEvacuationTestGuard::on();
        unsafe {
            let mut record = ArrayStackRecord {
                payload: crate::value::js_nanbox_pointer(super::super::js_array_alloc(0) as i64),
                next: crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                    b"captured".as_ptr(),
                    8,
                ) as i64),
                index: 3.0,
                state: 0.0,
                protocol: 1.0,
                value: crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64),
            };
            let before = [
                record.payload.to_bits(),
                record.next.to_bits(),
                record.value.to_bits(),
            ];
            crate::gc::js_shadow_slot_bind(0, std::ptr::addr_of_mut!(record.payload).cast());
            crate::gc::js_shadow_slot_bind(1, std::ptr::addr_of_mut!(record.next).cast());
            crate::gc::js_shadow_slot_bind(5, std::ptr::addr_of_mut!(record.value).cast());
            let cycles = crate::gc::copying_minor_cycles();
            crate::gc::gc_collect_minor();
            assert!(
                crate::gc::copying_minor_cycles() > cycles,
                "the test must actually copy"
            );
            for (old, new) in before
                .into_iter()
                .zip([record.payload, record.next, record.value])
            {
                assert_ne!(
                    old,
                    new.to_bits(),
                    "every managed stack field must be rewritten"
                );
                let raw = (new.to_bits() & crate::value::POINTER_MASK) as usize;
                assert!(crate::arena::pointer_in_nursery(raw));
            }
            assert_eq!(
                (record.index, record.state, record.protocol),
                (3.0, 0.0, 1.0)
            );
        }
    }
    #[test]
    fn stack_record_release_is_complete_and_preserves_scalars() {
        let mut record = ArrayStackRecord {
            payload: 11.0,
            next: 22.0,
            value: 33.0,
            index: 4.0,
            state: 0.0,
            protocol: 1.0,
        };
        unsafe {
            clear_record(&mut record);
        }
        for word in [record.payload, record.next, record.value] {
            assert_eq!(
                word.to_bits(),
                TAG_UNDEFINED,
                "every transferred word must be released before reentry"
            );
        }
        assert_eq!(
            (record.index, record.state, record.protocol),
            (4.0, 0.0, 1.0)
        );
    }
    #[test]
    fn stack_record_read_uses_the_existing_getter_and_retains_payload() {
        let _stable = crate::gc::GcSuppressScope::new();
        unsafe {
            let array = super::super::js_array_alloc(2);
            let array = super::super::js_array_push_f64(array, 11.0);
            let array = super::super::js_array_push_f64(array, 17.0);
            let mut record = ArrayStackRecord {
                payload: crate::value::js_nanbox_pointer(array as i64),
                next: f64::from_bits(TAG_UNDEFINED),
                value: f64::from_bits(TAG_UNDEFINED),
                index: 1.0,
                state: 2.0,
                protocol: 0.0,
            };
            assert_eq!(js_array_record_stack_dispatch(&mut record, 5), 17.0);
            assert_eq!(
                record.payload.to_bits(),
                crate::value::js_nanbox_pointer(array as i64).to_bits()
            );
            assert_eq!(record.next.to_bits(), TAG_UNDEFINED);
            assert_eq!(record.value.to_bits(), TAG_UNDEFINED);
        }
    }
    #[test]
    fn stack_record_next_uses_live_array_bounds_and_one_value() {
        let _stable = crate::gc::GcSuppressScope::new();
        unsafe {
            let array = super::super::js_array_alloc(2);
            let array = super::super::js_array_push_f64(array, 11.0);
            let array = super::super::js_array_push_f64(array, 17.0);
            let mut record = ArrayStackRecord {
                payload: crate::value::js_nanbox_pointer(array as i64),
                next: f64::from_bits(TAG_UNDEFINED),
                value: f64::from_bits(TAG_UNDEFINED),
                index: 1.0,
                state: 2.0,
                protocol: 0.0,
            };
            assert_eq!(
                js_array_record_stack_dispatch(&mut record, 6).to_bits(),
                crate::value::TAG_FALSE
            );
            assert_eq!(record.value, 17.0);
            assert_eq!(
                record.payload.to_bits(),
                crate::value::js_nanbox_pointer(array as i64).to_bits()
            );
            record.index = 2.0;
            assert_eq!(
                js_array_record_stack_dispatch(&mut record, 6).to_bits(),
                crate::value::TAG_TRUE
            );
            assert_eq!(record.payload.to_bits(), TAG_UNDEFINED);
            assert_eq!(record.value.to_bits(), TAG_UNDEFINED);
        }
    }
    #[test]
    fn stack_record_abi_is_six_gc_words() {
        assert_eq!(std::mem::size_of::<ArrayStackRecord>(), 48);
        assert_eq!(std::mem::offset_of!(ArrayStackRecord, next), 8);
        assert_eq!(std::mem::offset_of!(ArrayStackRecord, index), 16);
        assert_eq!(std::mem::offset_of!(ArrayStackRecord, state), 24);
        assert_eq!(std::mem::offset_of!(ArrayStackRecord, protocol), 32);
        assert_eq!(std::mem::offset_of!(ArrayStackRecord, value), 40);
    }
}
