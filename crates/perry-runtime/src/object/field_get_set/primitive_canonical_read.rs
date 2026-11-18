//! A primitive's already-computed key enters its intrinsic chain once.
use crate::value::{JSValue, POINTER_MASK, TAG_HOLE};

pub(crate) fn primitive_canonical_get(receiver: f64, key: f64, intrinsic: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_nanbox_f64(key);
    let prototype = scope.root_nanbox_f64(crate::object::builtin_prototype_value(intrinsic));
    let holder = JSValue::from_bits(prototype.get_nanbox_u64());
    if holder.is_pointer()
        && (holder.bits() & POINTER_MASK) as usize >= perry_abi::RECEIVER_HANDLE_FLOOR
    {
        // This probe is a leaf. Its absence verdict covers the complete
        // ordinary chain; a getter/exotic decline retains the original receiver.
        if let Some((word, hash)) =
            unsafe { crate::object::layout_probe::canonical_key_for_read(key.get_nanbox_u64()) }
        {
            let address = (holder.bits() & POINTER_MASK) as usize;
            let value = unsafe {
                crate::object::layout_probe::perry_probe_get(
                    crate::object::shapes::ordinary_dir_addr(),
                    address.wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
                    word,
                    hash,
                )
            };
            if value.to_bits() != TAG_HOLE {
                return value;
            }
        }
    }
    crate::object::property_ops::reflect::canonical_get(
        prototype.get_nanbox_f64(),
        key.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
    )
}
