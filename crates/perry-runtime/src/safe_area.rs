//! Safe-area inset object for `perry/system` `getSafeAreaInsets()` (issue #1475).
//!
//! Builds the `{ top, right, bottom, left }` object returned to TS, with all
//! four values in points. Each platform UI crate computes the four insets from
//! the OS (e.g. `UIWindow.safeAreaInsets` on iOS, `WindowInsets.Type.systemBars()`
//! on Android, zero on macOS/host) and calls `perry_safe_area_insets_make` to
//! materialize the JS object — keeping the object layout owned in one place in
//! the runtime rather than duplicated across every platform backend.
//!
//! Allocated in the caller's thread-local nursery — cheap, dies on the next
//! minor GC once the caller drops it. `getSafeAreaInsets()` is invoked on the
//! main thread (same as the UI runtime), so the allocation lands in the right
//! arena.

use crate::object::{js_object_alloc_with_shape, js_object_set_field};
use crate::value::JSValue;

/// Allocate a `{ top, right, bottom, left }` object and return it NaN-boxed
/// (POINTER_TAG). The system-table row for `getSafeAreaInsets` uses
/// `ReturnKind::F64`, so this NaN-boxed pointer is passed straight through to
/// TS unchanged — TS sees a plain object it can destructure.
#[no_mangle]
pub extern "C" fn perry_safe_area_insets_make(top: f64, right: f64, bottom: f64, left: f64) -> f64 {
    let packed = b"top\0right\0bottom\0left\0";
    let field_count: u32 = 4;
    // Unique shape_id — must not collide with other shape-allocated objects
    // (grep `0x7FFF_FF` in perry-runtime to verify).
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF34,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    );

    js_object_set_field(obj, 0, JSValue::number(top));
    js_object_set_field(obj, 1, JSValue::number(right));
    js_object_set_field(obj, 2, JSValue::number(bottom));
    js_object_set_field(obj, 3, JSValue::number(left));

    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::js_object_get_field;
    use crate::value::POINTER_TAG;

    /// Confirm the object carries the four expected fields in the documented
    /// `top, right, bottom, left` order. Guards against silently reordering the
    /// packed-keys list — every platform crate passes the four insets in this
    /// exact positional order.
    #[test]
    fn safe_area_insets_layout_matches_documented_order() {
        let nb = perry_safe_area_insets_make(59.0, 0.0, 34.0, 0.0);
        let bits = nb.to_bits();
        assert_eq!(bits & 0xFFFF_0000_0000_0000, POINTER_TAG);
        let obj_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut crate::object::ObjectHeader;

        let top = unsafe { js_object_get_field(obj_ptr, 0) };
        let right = unsafe { js_object_get_field(obj_ptr, 1) };
        let bottom = unsafe { js_object_get_field(obj_ptr, 2) };
        let left = unsafe { js_object_get_field(obj_ptr, 3) };

        assert_eq!(f64::from_bits(top.bits()), 59.0);
        assert_eq!(f64::from_bits(right.bits()), 0.0);
        assert_eq!(f64::from_bits(bottom.bits()), 34.0);
        assert_eq!(f64::from_bits(left.bits()), 0.0);
    }
}
