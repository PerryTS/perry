//! Materialization of constant trees with compiler-authored record layouts.
//! The schema table contains addresses of existing module root slots, never a
//! cached heap pointer. Every invocation creates fresh arrays and objects.

use crate::value::JSValue;

/// Keep in sync with perry-codegen::expr::literal_descriptor::SHAPE_TYPE.
#[repr(C)]
pub struct LiteralShape {
    class_id: u32,
    field_count: u32,
    keys_slot: *const u64,
    shape_id_slot: *const u32,
    raw_mask: *const u64,
    raw_mask_len: u32,
    pointer_mask: *const u64,
    pointer_mask_len: u32,
}

struct Reader<'a> {
    bytes: &'a [u8],
    shapes: &'a [LiteralShape],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(len)?;
        let bytes = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn value(&mut self, depth: usize) -> Option<JSValue> {
        if depth > 128 {
            return None;
        }
        match self.take(1)?[0] {
            0 => Some(JSValue::number(f64::from_le_bytes(
                self.take(8)?.try_into().ok()?,
            ))),
            1 => {
                let count = self.u32()?;
                if count as usize > self.bytes.len() - self.pos {
                    return None;
                }
                let array = super::js_array_alloc_literal(count);
                let mut numeric = count != 0;
                for i in 0..count {
                    numeric &= self.bytes.get(self.pos) == Some(&0);
                    let value = self.value(depth + 1)?;
                    // The common store maintains string sharing, array element
                    // facts, GC slot layout and write barriers.
                    unsafe { super::store_array_slot(array, i as usize, value.bits()) };
                }
                if numeric {
                    super::js_array_mark_numeric_f64_layout(array);
                }
                Some(JSValue::pointer(array as *const u8))
            }
            2 => Some(JSValue::bool(true)),
            3 => Some(JSValue::bool(false)),
            4 => Some(JSValue::null()),
            5 => Some(JSValue::undefined()),
            6 => {
                let len = self.u32()?;
                let bytes = self.take(len as usize)?;
                let string = crate::string::js_string_from_bytes(bytes.as_ptr(), len);
                Some(JSValue::from_bits(
                    crate::value::js_nanbox_string(string as i64).to_bits(),
                ))
            }
            7 => {
                let index = self.u32()? as usize;
                let shape = self.shapes.get(index)?;
                if shape.keys_slot.is_null() || shape.shape_id_slot.is_null() {
                    return None;
                }
                let object = crate::object::js_object_alloc_class_inline_keys_stamped(
                    shape.class_id,
                    0,
                    shape.field_count,
                    unsafe { *shape.keys_slot } as *mut super::ArrayHeader,
                    unsafe { *shape.shape_id_slot },
                );
                for i in 0..shape.field_count {
                    let value = self.value(depth + 1)?;
                    crate::object::js_object_set_field(object, i, value);
                }
                // Use the very same immutable masks and typed ShapeId as new.
                // Validate the completed fields before enabling direct reads.
                crate::gc::js_gc_init_typed_shape_layout(
                    object as u64,
                    shape.field_count,
                    shape.raw_mask,
                    shape.raw_mask_len,
                    shape.pointer_mask,
                    shape.pointer_mask_len,
                );
                Some(JSValue::pointer(object as *const u8))
            }
            _ => None,
        }
    }
}

/// Inputs are compiler-owned static data. GC suppression protects partial
/// trees during recursive allocation, just as for the existing constant-array
/// descriptor and JSON parser. No user code runs in this materializer.
#[no_mangle]
pub extern "C" fn js_value_from_literal_descriptor(
    bytes: *const u8,
    len: u32,
    shapes: *const LiteralShape,
    shape_count: u32,
) -> f64 {
    if bytes.is_null() || shapes.is_null() || len == 0 || shape_count == 0 {
        return f64::from_bits(JSValue::undefined().bits());
    }
    let _suppress = crate::gc::GcSuppressScope::new();
    let mut reader = Reader {
        bytes: unsafe { std::slice::from_raw_parts(bytes, len as usize) },
        shapes: unsafe { std::slice::from_raw_parts(shapes, shape_count as usize) },
        pos: 0,
    };
    f64::from_bits(reader.value(0).unwrap_or_else(JSValue::undefined).bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_descriptor_preserves_shape_freshness_and_traced_children() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        let _verification = crate::gc::knob_overrides::VerifyEvacuationTestGuard::on();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        const RAW: &[u64] = &[1];
        const POINTERS: &[u64] = &[2];
        const CLASS_ID: u32 = 1017301;
        let keys =
            crate::object::js_build_class_keys_array(CLASS_ID, 2, b"id\0name\0".as_ptr(), 8) as u64;
        let shape_id = crate::gc::js_gc_typed_shape_id_for_keys(
            CLASS_ID,
            keys,
            2,
            RAW.as_ptr(),
            1,
            POINTERS.as_ptr(),
            1,
        );
        let shape = LiteralShape {
            class_id: CLASS_ID,
            field_count: 2,
            keys_slot: &keys,
            shape_id_slot: &shape_id,
            raw_mask: RAW.as_ptr(),
            raw_mask_len: 1,
            pointer_mask: POINTERS.as_ptr(),
            pointer_mask_len: 1,
        };
        // {id:-0, name:"snowman☃"}, using the public compiler/runtime ABI.
        let mut bytes = vec![7, 0, 0, 0, 0, 0];
        bytes.extend_from_slice(&(-0.0_f64).to_le_bytes());
        bytes.push(6);
        let name = "snowman☃".as_bytes();
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name);
        let scope = crate::gc::RuntimeHandleScope::new();
        let a = scope.root_nanbox_f64(js_value_from_literal_descriptor(
            bytes.as_ptr(),
            bytes.len() as u32,
            &shape,
            1,
        ));
        let b = scope.root_nanbox_f64(js_value_from_literal_descriptor(
            bytes.as_ptr(),
            bytes.len() as u32,
            &shape,
            1,
        ));
        assert_ne!(a.get_nanbox_f64().to_bits(), b.get_nanbox_f64().to_bits());
        let object = |handle: &crate::gc::RuntimeHandle<'_>| {
            JSValue::from_bits(handle.get_nanbox_f64().to_bits())
                .as_pointer::<crate::object::ObjectHeader>()
        };
        let a_ptr = object(&a);
        assert_eq!(unsafe { (*a_ptr).class_id }, CLASS_ID);
        assert_eq!(
            unsafe { crate::object::shapes::object_shape_stamp(a_ptr) },
            shape_id
        );
        let header = unsafe { crate::value::addr_class::try_read_gc_header(a_ptr as usize) }
            .expect("the descriptor must allocate a managed object");
        assert_ne!(header._reserved & crate::gc::GC_OBJ_TYPED_LAYOUT_INTACT, 0);
        assert_eq!(
            crate::object::js_object_get_field(a_ptr, 0).bits(),
            (-0.0_f64).to_bits()
        );
        assert!(crate::object::js_object_get_field(a_ptr, 1).is_string());
        crate::object::js_object_set_field(a_ptr as *mut _, 0, JSValue::number(99.0));
        let cycles = crate::gc::copying_minor_cycles();
        crate::gc::gc_collect_minor();
        assert!(crate::gc::copying_minor_cycles() > cycles);
        assert_ne!(object(&a), a_ptr, "the rooted record must actually move");
        assert_eq!(
            crate::object::js_object_get_field(object(&a), 0).as_number(),
            99.0
        );
        assert_eq!(
            crate::object::js_object_get_field(object(&b), 0).bits(),
            (-0.0_f64).to_bits()
        );
        let expected_name = scope.root_string_ptr(crate::string::js_string_from_bytes(
            name.as_ptr(),
            name.len() as u32,
        ));
        let actual_name = crate::object::js_object_get_field(object(&b), 1);
        assert_eq!(
            crate::value::js_jsvalue_equals(
                f64::from_bits(actual_name.bits()),
                crate::value::js_nanbox_string(
                    expected_name.get_raw_const_ptr::<crate::StringHeader>() as i64
                ),
            ),
            1
        );
    }

    #[test]
    fn literal_descriptor_rejects_truncated_data_and_invalid_shape_indices() {
        let shape = LiteralShape {
            class_id: 0,
            field_count: 0,
            keys_slot: std::ptr::null(),
            shape_id_slot: std::ptr::null(),
            raw_mask: std::ptr::null(),
            raw_mask_len: 0,
            pointer_mask: std::ptr::null(),
            pointer_mask_len: 0,
        };
        for bytes in [
            &[0_u8][..],
            &[6, 10, 0, 0, 0],
            &[7, 1, 0, 0, 0],
            &[1, 255, 255, 255, 255],
        ] {
            assert_eq!(
                js_value_from_literal_descriptor(bytes.as_ptr(), bytes.len() as u32, &shape, 1)
                    .to_bits(),
                JSValue::undefined().bits()
            );
        }
    }
}
