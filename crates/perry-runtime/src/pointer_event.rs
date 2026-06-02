//! PointerEvent allocation for perry/ui issue #1868.
//!
//! Builds the `{ x, y, button, pointerType }` object that gets passed to
//! `onMouseDown`/`onMouseUp`/`onMouseMove` callbacks. Allocated in the
//! caller's thread-local nursery — cheap, dies on the next minor GC if
//! the callback doesn't retain it.
//!
//! Every UI platform crate calls `js_pointer_event_new` from its native
//! event handler, then passes the returned NaN-boxed f64 to
//! `js_closure_call1`.

use crate::object::{js_object_alloc_with_shape, js_object_set_field};
use crate::string::js_string_from_bytes_longlived;
use crate::value::{JSValue, POINTER_MASK, STRING_TAG};

/// `PointerType` discriminator passed in via FFI. Keep in sync with the
/// TS `enum PointerType` and with every platform backend's call site.
pub const POINTER_TYPE_MOUSE: u32 = 0;
pub const POINTER_TYPE_TOUCH: u32 = 1;
pub const POINTER_TYPE_PEN: u32 = 2;

/// Stable interned strings for `pointerType`. Built once on first use
/// and reused for every event — avoids per-event string allocation
/// in the move/down/up hot path.
struct InternedPointerTypes {
    mouse: f64,
    touch: f64,
    pen: f64,
}

fn intern_pointer_type_strings() -> &'static InternedPointerTypes {
    use std::sync::OnceLock;
    static CELL: OnceLock<InternedPointerTypes> = OnceLock::new();
    CELL.get_or_init(|| {
        let mk = |bytes: &[u8]| -> f64 {
            let ptr = js_string_from_bytes_longlived(bytes.as_ptr(), bytes.len() as u32);
            f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK))
        };
        InternedPointerTypes {
            mouse: mk(b"mouse"),
            touch: mk(b"touch"),
            pen: mk(b"pen"),
        }
    })
}

/// Allocate a `PointerEvent { x, y, button, pointerType }` object and
/// return it NaN-boxed (POINTER_TAG).
///
/// - `x`, `y`: widget-local coordinates in points, origin top-left.
/// - `button`: 0=Left, 1=Middle, 2=Right, 3=Back, 4=Forward. Always 0 on touch.
/// - `pointer_type`: see `POINTER_TYPE_*` constants above.
///
/// Pass the returned f64 directly to `js_closure_call1`.
#[no_mangle]
pub extern "C" fn js_pointer_event_new(x: f64, y: f64, button: u32, pointer_type: u32) -> f64 {
    let packed = b"x\0y\0button\0pointerType\0";
    let field_count: u32 = 4;
    // Unique shape_id for PointerEvent — must not collide with other
    // shape-allocated objects (grep `0x7FFF_FF` in perry-runtime to verify).
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF30,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    );

    let interned = intern_pointer_type_strings();
    let type_str_bits = match pointer_type {
        POINTER_TYPE_TOUCH => interned.touch.to_bits(),
        POINTER_TYPE_PEN => interned.pen.to_bits(),
        _ => interned.mouse.to_bits(),
    };

    js_object_set_field(obj, 0, JSValue::number(x));
    js_object_set_field(obj, 1, JSValue::number(y));
    js_object_set_field(obj, 2, JSValue::number(button as f64));
    js_object_set_field(obj, 3, JSValue::from_bits(type_str_bits));

    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::js_object_get_field;
    use crate::value::POINTER_TAG;

    /// Strip the POINTER_TAG and confirm the underlying object carries
    /// the four expected fields in the documented order. Guards against
    /// silently reordering the packed-keys list — call sites in every
    /// platform crate depend on the field-index → name mapping.
    #[test]
    fn pointer_event_layout_matches_documented_order() {
        let nb = js_pointer_event_new(10.5, 20.25, 2, POINTER_TYPE_PEN);
        let bits = nb.to_bits();
        // Pointer-tagged, not just an f64.
        assert_eq!(bits & 0xFFFF_0000_0000_0000, POINTER_TAG);
        let obj_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut crate::object::ObjectHeader;

        let x = unsafe { js_object_get_field(obj_ptr, 0) };
        let y = unsafe { js_object_get_field(obj_ptr, 1) };
        let button = unsafe { js_object_get_field(obj_ptr, 2) };
        // Field 3 (pointerType) is a string — exercise only that the
        // value carries the STRING_TAG so we know we wrote *a* string.
        let pt = unsafe { js_object_get_field(obj_ptr, 3) };

        assert_eq!(f64::from_bits(x.bits()), 10.5);
        assert_eq!(f64::from_bits(y.bits()), 20.25);
        assert_eq!(f64::from_bits(button.bits()), 2.0);
        assert_eq!(
            pt.bits() & 0xFFFF_0000_0000_0000,
            crate::value::STRING_TAG,
            "pointerType slot should carry STRING_TAG"
        );
    }

    /// Touch + mouse + pen each produce a *different* interned
    /// pointerType string. Catches accidental fallthrough in the match
    /// inside `js_pointer_event_new`.
    #[test]
    fn pointer_type_discriminator_picks_distinct_strings() {
        let mouse = js_pointer_event_new(0.0, 0.0, 0, POINTER_TYPE_MOUSE);
        let touch = js_pointer_event_new(0.0, 0.0, 0, POINTER_TYPE_TOUCH);
        let pen = js_pointer_event_new(0.0, 0.0, 0, POINTER_TYPE_PEN);
        let pt = |nb: f64| {
            let obj = (nb.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::object::ObjectHeader;
            unsafe { js_object_get_field(obj, 3) }.bits()
        };
        let m = pt(mouse);
        let t = pt(touch);
        let p = pt(pen);
        assert_ne!(m, t);
        assert_ne!(t, p);
        assert_ne!(m, p);
    }
}
