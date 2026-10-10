//! A construct site's resolved entry, guarded by the callee's shape and template.
//!
//! Four native words, no heap pointers: shape, template identity plus capture
//! slot, compiled code, and signature. Captures come from the current evaluation.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

type Constructor = (usize, u32, u32);

/// A primed class shape already proves the constructor kind. Validate that
/// proof before repeating function/closure/class classification. The header
/// check rejects arrays whose capacity word could resemble an object ShapeId.
#[inline]
pub(super) unsafe fn validated_class_entry(
    site: *const AtomicU64,
    value: f64,
) -> Option<Constructor> {
    if site.is_null() {
        return None;
    }
    let shape = (*site).load(Ordering::Relaxed);
    if shape == 0 || shape > u32::MAX as u64 {
        return None;
    }
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let object = value.as_pointer::<ObjectHeader>();
    let header = crate::value::addr_class::try_read_gc_header(object as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || crate::object::shapes::object_shape_stamp(object) as u64 != shape
        || (*object).class_id != (*site.add(1)).load(Ordering::Relaxed) as u32
    {
        return None;
    }
    let code = (*site.add(2)).load(Ordering::Relaxed) as usize;
    let signature = (*site.add(3)).load(Ordering::Relaxed);
    Some((code, signature as u32, (signature >> 32) as u32))
}

pub(super) unsafe fn site_constructor(
    site: *const AtomicU64,
    object: *const ObjectHeader,
) -> Option<Constructor> {
    if site.is_null() {
        return None;
    }
    let shape = crate::object::shapes::object_shape_stamp(object);
    let cid = (*object).class_id;
    let hit = (*site).load(Ordering::Relaxed) == shape as u64
        && (*site.add(1)).load(Ordering::Relaxed) as u32 == cid;
    let entry = resolved_site_constructor(site, shape as u64, cid)?;
    if !hit && entry.2 != 0 {
        let slot = crate::object::shapes::shape_record_by_id(shape)
            .and_then(|record| record.own_data_slot_of_value(0, b"__perry_ctor_caps"))
            .flatten()
            .or_else(|| {
                // Class marking changes dispatch kind, while its own keys
                // retain the ordinary slot layout. The general own-slot
                // reader deliberately declines this kind; prove only this
                // compiler-owned capture data slot from the same descriptor.
                let d = crate::object::shapes::shape_descriptor_by_id(shape)?;
                if d.object_kind != crate::object::shapes::ShapeObjectKind::Class
                    || d.summary & crate::object::key_attrs::SUMMARY_ACCESSOR != 0
                {
                    return None;
                }
                (0..d.logical_key_count.min(d.live_inline_slot_count))
                    .rev()
                    .find(|&slot| {
                        crate::string::js_string_key_matches_bytes(
                            d.keys_view().get(slot),
                            b"__perry_ctor_caps",
                        )
                    })
            })
            .and_then(|slot| slot.checked_add(1))
            .unwrap_or(0);
        (*site.add(1)).store(cid as u64 | (slot as u64) << 32, Ordering::Relaxed);
    }
    Some(entry)
}

/// A data-slot proof to revalidate after receiver/prototype allocation.
pub(super) unsafe fn capture_slot(site: *const AtomicU64) -> Option<(u32, u32)> {
    if site.is_null() {
        return None;
    }
    let shape = (*site).load(Ordering::Relaxed);
    let slot = ((*site.add(1)).load(Ordering::Relaxed) >> 32) as u32;
    (shape <= u32::MAX as u64 && slot != 0).then(|| (shape as u32, slot - 1))
}

pub(super) unsafe fn resolved_site_constructor(
    site: *const AtomicU64,
    shape: u64,
    cid: u32,
) -> Option<Constructor> {
    if site.is_null() {
        return None;
    }
    if shape == 0 || cid == 0 {
        return None;
    }
    // ShapeIds are agent-local, so codegen emits this site thread-local in
    // programs with workers. Template identity distinguishes classes whose
    // own keys happen to yield the same shape.
    if (*site).load(Ordering::Relaxed) == shape
        && (*site.add(1)).load(Ordering::Relaxed) as u32 == cid
    {
        let code = (*site.add(2)).load(Ordering::Relaxed) as usize;
        let signature = (*site.add(3)).load(Ordering::Relaxed);
        return Some((code, signature as u32, (signature >> 32) as u32));
    }
    let entry = super::super::super::class_constructors::lookup_class_constructor(cid)?;
    (*site.add(2)).store(entry.0 as u64, Ordering::Relaxed);
    (*site.add(3)).store(entry.1 as u64 | (entry.2 as u64) << 32, Ordering::Relaxed);
    (*site.add(1)).store(cid as u64, Ordering::Relaxed);
    (*site).store(shape, Ordering::Relaxed);
    Some(entry)
}

/// The generic `new` boundary with a per-use validated class entry. Ordinary
/// functions and exotic constructors use the same existing construct dispatcher.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_new_function_construct_site(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    site: *const AtomicU64,
) -> f64 {
    construct_function_impl(func_value, args_ptr, args_len, site)
}

#[used(compiler)]
static KEEP_CONSTRUCT_SITE: unsafe extern "C-unwind" fn(
    f64,
    *const f64,
    usize,
    *const AtomicU64,
) -> f64 = js_new_function_construct_site;

/// The parent value is the heritage captured at class evaluation. Resolve its
/// own entry through the same guarded construct-site record; exotic and
/// implicit parents retain the existing superclass dispatcher.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_class_value_super_construct_site(
    parent: f64,
    this: f64,
    args_ptr: *const f64,
    args_len: usize,
    site: *const AtomicU64,
) -> f64 {
    let entry = validated_class_entry(site, parent).or_else(|| {
        if is_class_object_value(parent) {
            let object = JSValue::from_bits(parent.to_bits()).as_pointer::<ObjectHeader>();
            site_constructor(site, object)
        } else {
            None
        }
    });
    if let Some(entry) = entry {
        let object = JSValue::from_bits(parent.to_bits()).as_pointer::<ObjectHeader>();
        let this_value = JSValue::from_bits(this.to_bits());
        if this_value.is_pointer() {
            let receiver = this_value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
            if crate::value::addr_class::try_read_gc_header(receiver as usize)
                .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT)
            {
                let cid = (*object).class_id;
                let capture_slot = capture_slot(site);
                let scope = crate::gc::RuntimeHandleScope::new();
                let parent = scope.root_nanbox_f64(parent);
                let receiver = scope.root_raw_mut_ptr(receiver);
                let _new_target = crate::object::SuperNewTargetScope::bind(&scope, this);
                prepare_super_class_receiver(receiver, parent);
                return receiver.with_mut_ptr::<ObjectHeader, _>(|receiver| {
                    super::super::super::class_constructors::construct_class_object_resolved(
                        parent.get_nanbox_f64(),
                        cid,
                        receiver,
                        args_ptr,
                        args_len,
                        Some(entry),
                        capture_slot,
                    )
                });
            }
        }
    }
    crate::object::global_this::js_fetch_or_value_super(parent, this, args_ptr, args_len)
}

#[used(compiler)]
static KEEP_SUPER_CONSTRUCT_SITE: unsafe extern "C-unwind" fn(
    f64,
    f64,
    *const f64,
    usize,
    *const AtomicU64,
) -> f64 = js_class_value_super_construct_site;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primed_class_entry_checks_identity_shape_and_receiver_header() {
        unsafe {
            let cid = 0x6f26;
            crate::object::class_constructors::js_register_class_constructor(cid, 16, 3, 0);
            let scope = crate::gc::RuntimeHandleScope::new();
            let class = scope.root_raw_mut_ptr(js_object_alloc(cid as u32, 0));
            class.with_mut_ptr::<ObjectHeader, _>(|class| {
                crate::object::class_registry::js_object_mark_class(class as i64);
            });
            let site = [const { AtomicU64::new(0) }; 4];
            class.with_mut_ptr::<ObjectHeader, _>(|class| {
                let entry = site_constructor(site.as_ptr(), class);
                let value = crate::value::js_nanbox_pointer(class as i64);
                assert_eq!(validated_class_entry(site.as_ptr(), value), entry);
                (*class).class_id += 1;
                assert_eq!(validated_class_entry(site.as_ptr(), value), None);
                (*class).class_id -= 1;
                site[0].fetch_add(1, Ordering::Relaxed);
                assert_eq!(validated_class_entry(site.as_ptr(), value), None);
                site_constructor(site.as_ptr(), class);
            });
            assert_eq!(validated_class_entry(site.as_ptr(), 22.0), None);
            // An array's +4 is capacity, not ShapeId. Give it the same word
            // without allocating or collecting while its capacity is changed.
            let array = crate::array::js_array_alloc(0);
            let capacity = (*array).capacity;
            let length = (*array).length;
            (*array).length = cid as u32;
            (*array).capacity = site[0].load(Ordering::Relaxed) as u32;
            let result =
                validated_class_entry(site.as_ptr(), crate::value::js_nanbox_pointer(array as i64));
            (*array).capacity = capacity;
            (*array).length = length;
            assert_eq!(result, None);
        }
    }

    #[test]
    fn construct_site_retains_a_revalidated_capture_data_slot() {
        unsafe {
            let cid = 0x6f25;
            crate::object::class_constructors::js_register_class_constructor(cid, 16, 2, 1);
            let scope = crate::gc::RuntimeHandleScope::new();
            let first = scope.root_raw_mut_ptr(js_object_alloc(cid as u32, 0));
            let second = scope.root_raw_mut_ptr(js_object_alloc(cid as u32, 0));
            let key = b"__perry_ctor_caps";
            let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                key.as_ptr(),
                key.len() as u32,
            ));
            for class in [first, second] {
                class.with_mut_ptr::<ObjectHeader, _>(|class| {
                    crate::object::class_registry::js_object_mark_class(class as i64);
                });
            }
            for (class, value) in [(first, 11.0), (second, 22.0)] {
                class.with_mut_ptr::<ObjectHeader, _>(|class| {
                    key.with_const_ptr::<crate::StringHeader, _>(|key| {
                        crate::object::js_object_set_field_by_name(class, key, value);
                    });
                });
            }
            let site = [const { AtomicU64::new(0) }; 4];
            for (class, expected) in [(first, 11.0), (second, 22.0), (first, 11.0)] {
                class.with_mut_ptr::<ObjectHeader, _>(|class| {
                    assert_eq!(site_constructor(site.as_ptr(), class), Some((16, 2, 1)));
                    let (shape, slot) = capture_slot(site.as_ptr()).expect("own capture data slot");
                    assert_eq!(shape, crate::object::shapes::object_shape_stamp(class));
                    assert_eq!(
                        crate::object::js_object_get_field(class, slot).as_number(),
                        expected
                    );
                });
            }
        }
    }

    #[test]
    fn construct_site_revalidates_shape_and_template_identity() {
        unsafe {
            let first = 0x6f21;
            let second = 0x6f22;
            crate::object::class_constructors::js_register_class_constructor(first, 16, 3, 0);
            crate::object::class_constructors::js_register_class_constructor(second, 32, 2, 1);
            let object = js_object_alloc(first as u32, 4);
            let site = [const { AtomicU64::new(0) }; 4];
            let entry = site_constructor(site.as_ptr(), object);
            assert_eq!(entry, Some((16, 3, 0)));
            assert_ne!(site[0].load(Ordering::Relaxed), 0, "site must have primed");
            assert_eq!(site_constructor(site.as_ptr(), object), entry);
            // Keep the object's shape and replace only its template identity.
            (*object).class_id = second as u32;
            assert_eq!(site_constructor(site.as_ptr(), object), Some((32, 2, 1)));
            // A stale shape proof must be refreshed even when the template is
            // unchanged. Poison the memo's code word to make this observable.
            site[0].store(u64::MAX, Ordering::Relaxed);
            site[2].store(48, Ordering::Relaxed);
            assert_eq!(site_constructor(site.as_ptr(), object), Some((32, 2, 1)));
            // A class with no own constructor cannot inherit a previous memo.
            (*object).class_id = 0x6f23;
            assert_eq!(site_constructor(site.as_ptr(), object), None);
        }
    }
}
