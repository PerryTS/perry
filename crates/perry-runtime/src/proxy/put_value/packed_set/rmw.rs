//! Read/modify/write consumes the existing writable-own-slot store memo.
//! No second read cache: its ShapeId proves both GetValue and PutValue.
use super::*;

struct OwnSlot {
    slot: *mut f64,
    raw_number: bool,
}

/// The single entry proof, shared by the leaf and generic entries. The memo
/// was published by prime_packed_set; it contains no heap addresses.
unsafe fn prove(target: f64, packed: *const AtomicU64) -> Option<OwnSlot> {
    let bits = target.to_bits();
    if packed.is_null()
        || bits & !POINTER_MASK != POINTER_TAG
        || bits & POINTER_MASK <= crate::value::addr_class::HANDLE_BAND_MAX as u64
    {
        return None;
    }
    let obj = (bits & POINTER_MASK) as *mut crate::ObjectHeader;
    let word = (*packed).load(Ordering::Relaxed);
    if crate::object::shapes::object_shape_stamp(obj) != word as u32
        || word & PACKED_SET_CONSTFN_SLOT != 0
    {
        return None;
    }
    let index = ((word & !PACKED_SET_FLAGS) >> 32) as usize;
    let slot = (obj as *mut u8)
        .add(std::mem::size_of::<crate::ObjectHeader>())
        .cast::<f64>()
        .add(index);
    Some(OwnSlot {
        slot,
        raw_number: word & PACKED_SET_F64_SLOT != 0,
    })
}

fn number_op(old: f64, rhs: f64, op: i32) -> Option<f64> {
    Some(match op {
        0 | 2 => old - 1.0,
        1 | 3 => old + 1.0,
        16 => old + rhs,
        17 => old - rhs,
        18 => old * rhs,
        19 => old / rhs,
        20 => old % rhs,
        21 => (crate::value::number_to_int32(old) | crate::value::number_to_int32(rhs)) as f64,
        22 => (crate::value::number_to_int32(old) & crate::value::number_to_int32(rhs)) as f64,
        23 => (crate::value::number_to_int32(old) ^ crate::value::number_to_int32(rhs)) as f64,
        24 => crate::value::number_to_int32(old)
            .wrapping_shl(crate::value::number_to_int32(rhs) as u32 & 31) as f64,
        25 => {
            (crate::value::number_to_int32(old) >> (crate::value::number_to_int32(rhs) as u32 & 31))
                as f64
        }
        26 => {
            ((crate::value::number_to_int32(old) as u32)
                >> (crate::value::number_to_int32(rhs) as u32 & 31)) as f64
        }
        27 => crate::math::js_math_pow(old, rhs),
        32 | 33 => rhs,
        _ => return None,
    })
}

unsafe fn body(proof: OwnSlot, rhs: f64, op: i32) -> Option<f64> {
    let value = crate::value::JSValue::from_bits((*proof.slot).to_bits());
    let old = if proof.raw_number {
        // The memo's F64 bit is a fact of the matched ShapeId. No second
        // value proof is needed for that lane (deprecated carriers too).
        *proof.slot
    } else if value.is_number() {
        value.as_number()
    } else {
        // Legacy class references share INT32_TAG. Until their representation
        // is disjoint, the tag alone does not prove a Number; decline to the
        // existing ToNumeric entry instead of adding a registry probe here.
        return None;
    };
    if op == 34
        || (op == 32 && (old == 0.0 || old.is_nan()))
        || (op == 33 && old != 0.0 && !old.is_nan())
    {
        return Some(old);
    }
    let rhs = crate::value::JSValue::from_bits(rhs.to_bits());
    let rhs = if op < 4 {
        0.0
    } else if rhs.is_number() {
        rhs.as_number()
    } else {
        return None;
    };
    let new = number_op(old, rhs, op)?;
    let new = if new.is_nan() { f64::NAN } else { new };
    // GC_STORE_AUDIT(POINTER_FREE): Number operation; canonical raw double
    // preserves both Any and F64 lanes, with no pointer edge or rep change.
    *proof.slot = new;
    Some(if op < 2 { old } else { new })
}

#[no_mangle]
pub unsafe extern "C" fn js_property_rmw_fast(
    target: f64,
    rhs: f64,
    op: i32,
    packed: *const AtomicU64,
) -> f64 {
    prove(target, packed)
        .and_then(|proof| body(proof, rhs, op))
        .unwrap_or(f64::from_bits(crate::value::TAG_HOLE))
}

/// One generic continuation for every declined shape or numeric proof. Get,
/// conversion, operation and Set happen once, in order; returned BigInts stay
/// rooted across the setter. RHS here is an inert literal evaluated by codegen.
#[no_mangle]
pub unsafe extern "C" fn js_property_rmw(
    target: f64,
    key: *const crate::StringHeader,
    rhs: f64,
    flags: i32,
    cache: *mut PackedSetWaysSlot,
    packed: *const AtomicU64,
) -> f64 {
    let op = flags & 255;
    if let Some(result) = prove(target, packed).and_then(|proof| body(proof, rhs, op)) {
        return result;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(target);
    let key = scope.root_string_ptr(key);
    let rhs = scope.root_nanbox_f64(rhs);
    let key_box =
        crate::value::js_nanbox_string(key.get_raw_const_ptr::<crate::StringHeader>() as i64);
    let old = crate::value::js_dyn_index_get(receiver.get_nanbox_f64(), key_box);
    let old = scope.root_nanbox_f64(old);
    if (op == 32 && crate::value::js_is_truthy(old.get_nanbox_f64()) == 0)
        || (op == 33 && crate::value::js_is_truthy(old.get_nanbox_f64()) != 0)
        || (op == 34
            && !matches!(
                old.get_nanbox_f64().to_bits(),
                crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
            ))
    {
        return old.get_nanbox_f64();
    }
    let result = if op < 4 {
        crate::value::js_to_numeric(old.get_nanbox_f64())
    } else {
        old.get_nanbox_f64()
    };
    let old = scope.root_nanbox_f64(result);
    let (a, b) = (old.get_nanbox_f64(), rhs.get_nanbox_f64());
    let new = match op {
        0..=3 => crate::value::js_numeric_step(a, op & 1),
        16 => crate::value::js_dynamic_string_or_number_add(a, b),
        17 => crate::value::js_dynamic_sub(a, b),
        18 => crate::value::js_dynamic_mul(a, b),
        19 => crate::value::js_dynamic_div(a, b),
        20 => crate::value::js_dynamic_mod(a, b),
        21 => crate::value::js_dynamic_bitor(a, b),
        22 => crate::value::js_dynamic_bitand(a, b),
        23 => crate::value::js_dynamic_bitxor(a, b),
        24 => crate::value::js_dynamic_shl(a, b),
        25 => crate::value::js_dynamic_shr(a, b),
        26 => crate::value::js_dynamic_ushr(a, b),
        27 => crate::value::js_dynamic_pow(a, b),
        32..=34 => b,
        _ => unreachable!("invalid property RMW operation"),
    };
    let new = scope.root_nanbox_f64(new);
    js_put_value_set_packed_miss(
        receiver.get_nanbox_f64(),
        key.get_raw_const_ptr::<crate::StringHeader>(),
        new.get_nanbox_f64(),
        (flags >> 8) & 1,
        cache,
        packed,
    );
    if op < 2 {
        old.get_nanbox_f64()
    } else {
        new.get_nanbox_f64()
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static FAST: unsafe extern "C" fn(f64, f64, i32, *const AtomicU64) -> f64 = js_property_rmw_fast;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static GENERIC: unsafe extern "C" fn(
    f64,
    *const crate::StringHeader,
    f64,
    i32,
    *mut PackedSetWaysSlot,
    *const AtomicU64,
) -> f64 = js_property_rmw;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::put_value::record_fixture::{json, record};
    fn receiver() -> f64 {
        record(br#"{"x":5}"#)
    }
    /// An object with an overflow (a delete establishes one, §3.2) is not
    /// served by the shared store memo. Every RMW is answered by the generic
    /// path, with the right old and new values.
    #[test]
    fn an_overflow_receiver_declines_the_memo_and_computes_generically() {
        unsafe {
            let target = json(br#"{"x":5,"gone":0}"#);
            let obj = (target.to_bits() & crate::value::POINTER_MASK) as *mut crate::ObjectHeader;
            let gone = crate::string::js_string_from_bytes(b"gone".as_ptr(), 4);
            assert_eq!(crate::object::js_object_delete_field(obj, gone), 1);
            assert!(crate::object::overflow::has_overflow(obj));
            let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
            let site = super::super::super::packed_add::PackedSetSite::empty();
            let mut cache = std::ptr::null_mut();
            assert_eq!(
                js_property_rmw(target, key, 0.0, 1, &mut cache, &site.set),
                5.0,
                "postfix increment yields the old value"
            );
            assert_eq!(
                js_property_rmw_fast(target, 0.0, 3, &site.set).to_bits(),
                crate::value::TAG_HOLE,
                "the memo stays empty for an overflow receiver"
            );
            assert_eq!(
                js_property_rmw(target, key, 3.0, 21, &mut cache, &site.set),
                7.0,
                "6 | 3"
            );
            let x = crate::object::js_object_get_field_by_name(obj, key);
            assert_eq!(f64::from_bits(x.bits()), 7.0);
        }
    }
    #[test]
    fn shared_store_memo_updates_and_shape_mutations_decline() {
        unsafe {
            let target = receiver();
            let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
            let site = super::super::super::packed_add::PackedSetSite::empty();
            let mut cache = std::ptr::null_mut();
            assert_eq!(
                js_property_rmw(target, key, 0.0, 1, &mut cache, &site.set),
                5.0
            );
            assert_eq!(js_property_rmw_fast(target, 0.0, 3, &site.set), 7.0);
            assert_eq!(js_property_rmw_fast(target, 3.0, 21, &site.set), 7.0);
            // Negative control: the same memo must decline after freeze.
            crate::object::js_object_freeze(target);
            assert_eq!(
                js_property_rmw_fast(target, 0.0, 1, &site.set).to_bits(),
                crate::value::TAG_HOLE
            );
        }
    }
    #[test]
    fn string_numeric_conversion_and_postfix_result() {
        unsafe {
            let target = receiver();
            let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
            let site = super::super::super::packed_add::PackedSetSite::empty();
            let mut cache = std::ptr::null_mut();
            js_property_rmw(target, key, 0.0, 1, &mut cache, &site.set);
            let string = crate::string::js_string_from_bytes(b"12".as_ptr(), 2);
            js_put_value_set_packed_miss(
                target,
                key,
                crate::value::js_nanbox_string(string as i64),
                0,
                &mut cache,
                &site.set,
            );
            assert_eq!(
                js_property_rmw_fast(target, 0.0, 1, &site.set).to_bits(),
                crate::value::TAG_HOLE
            );
            assert_eq!(
                js_property_rmw(target, key, 0.0, 1, &mut cache, &site.set),
                12.0
            );
            assert_eq!(js_property_rmw_fast(target, 0.0, 3, &site.set), 14.0);
        }
    }
}
