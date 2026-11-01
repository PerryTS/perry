//! One shared cold body for numeric array GET. No per-site receiver dispatch,
//! prototype walk or forwarding repair is emitted by the compiler.
use super::*;

/// Transient call arguments for private record repair or diagnostic sites.
/// This is caller-owned scratch, never an RS4GC root address or persistent state.
#[repr(C)]
struct ColdArgs {
    site: u64,
    flags: u64,
    source: f64,
}

/// Boxed inputs preserve exact receivers and keys. Small controls 0/2 retain
/// claimed/typed dispatch; 4 proves an absent own slot with a signed key.
/// A pointer control is the existing live-array guard's handle and proves an
/// absent nonnegative integer own slot. Bit 0 requests numeric coercion.
/// Pointer controls tagged 2/6 name `ColdArgs`, with 6 requesting head repair.
///
/// # Safety
/// The pointer absence control is emitted only after a live ordinary-array
/// guard with an existing nonnegative integer key proof. Tagged scratch must
/// name aligned, writable `ColdArgs`; its output is published after getter GC.
#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn perry_array_read_cold_shared(
    receiver: f64,
    key: f64,
    control: u64,
) -> f64 {
    // The no-GC ordinary miss needs no frame. Every other arm tail-enters the
    // rooted Rust implementation with its original SysV arguments intact.
    core::arch::naked_asm!(
        "cmp rdi, 15",
        "jbe 2f",
        "test dil, 7",
        "jne 2f",
        "test word ptr [rdi - 6], {flags}",
        "jne 2f",
        // A named layout (`ArrayHeader.named`) may hold element descriptors;
        // the exact entry reads its attribute summary.
        "cmp qword ptr [rdi + {named}], 0",
        "jne 2f",
        // The exported flag is interposable in ELF provider dylibs. Load
        // through the GOT instead of emitting a text relocation to its byte.
        "mov rax, qword ptr [rip + {invalidated}@GOTPCREL]",
        "cmp byte ptr [rax], 0",
        "jne 2f",
        "movabs rax, {undefined}",
        "movq xmm0, rax",
        "ret",
        "2:",
        "jmp {exact}",
        flags = const crate::gc::GC_ARRAY_CUSTOM_PROTO,
        named = const std::mem::offset_of!(ArrayHeader, named),
        undefined = const crate::value::TAG_UNDEFINED,
        invalidated = sym PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED,
        exact = sym read_cold_entry,
    );
}

#[cfg(not(all(target_arch = "x86_64", not(target_os = "windows"))))]
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn perry_array_read_cold_shared(
    receiver: f64,
    key: f64,
    control: u64,
) -> f64 {
    read_cold_entry(receiver, key, control)
}

#[cold]
#[inline(never)]
unsafe extern "C" fn read_cold_entry(receiver: f64, key: f64, control: u64) -> f64 {
    let (site, control, source) = if control > 15 && control & 3 == 2 {
        let args = (control & !7) as *mut ColdArgs;
        (
            (*args).site,
            (*args).flags,
            if control & 4 != 0 {
                ptr::addr_of_mut!((*args).source)
            } else {
                ptr::null_mut()
            },
        )
    } else {
        (0, control, ptr::null_mut())
    };
    let coerce = control & 1 != 0;
    let mode = if control > 15 {
        1
    } else {
        ((control & 6) != 0) as u32
    };
    let arr = if control > 15 {
        (control & !7) as *const ArrayHeader
    } else {
        (receiver.to_bits() & crate::value::POINTER_MASK) as *const ArrayHeader
    };
    // The x86-64 ordinary return was checked by the shared naked prefix.
    // Numeric consumers and portable targets keep the equivalent Rust check.
    let check_empty = !cfg!(all(target_arch = "x86_64", not(target_os = "windows"))) || coerce;
    if source.is_null() && check_empty && (control > 15 || (control & 6 == 4 && key >= 0.0)) {
        let flags = array_object_flags_resolved(arr);
        if flags & crate::gc::GC_ARRAY_CUSTOM_PROTO == 0
            && !array_has_element_descriptors_resolved(arr)
            && PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed)
                & super::INDEX_INVALIDATED
                == 0
        {
            return if coerce {
                f64::NAN
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
        }
    }
    read_cold_dispatch(receiver, key, site, mode, coerce, source)
}

#[cold]
#[inline(never)]
unsafe fn read_cold_dispatch(
    receiver: f64,
    key: f64,
    site: u64,
    mode: u32,
    coerce: bool,
    source: *mut f64,
) -> f64 {
    if !source.is_null() {
        // Publish the live receiver after any getter/coercion GC.
        let arr =
            clean_arr_ptr((receiver.to_bits() & crate::value::POINTER_MASK) as *const ArrayHeader);
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(if arr.is_null() {
            receiver
        } else {
            crate::value::js_nanbox_pointer(arr as i64)
        });
        let value = read_cold_dispatch(
            receiver.get_nanbox_f64(),
            key,
            site,
            mode,
            coerce,
            ptr::null_mut(),
        );
        // GC_STORE_AUDIT(STACK): source is ColdArgs.source in caller-owned scratch (indexing_read_cold.rs5-22,76-89), written after read_cold_dispatch and any getter GC from a RuntimeHandleScope-rooted receiver. Never a persistent heap edge.
        source.write(receiver.get_nanbox_f64());
        return value;
    }
    let value = plain_index_get(receiver, key).unwrap_or_else(|| {
        if mode != 0 {
            crate::typed_feedback::js_typed_feedback_array_index_get_fallback_boxed(
                site, receiver, key,
            )
        } else if receiver.to_bits() >> 48 == 0x7FF9 {
            crate::value::js_dyn_index_get(receiver, key)
        } else {
            js_array_get_index_or_string(
                (receiver.to_bits() & crate::value::POINTER_MASK) as *const ArrayHeader,
                key,
            )
        }
    });
    if coerce {
        crate::builtins::js_number_coerce(value)
    } else {
        value
    }
}

/// No allocation or registry probe on an ordinary hole/OOB miss. The same
/// canonical backing address as generated reads also covers shifted queues
/// and arrays with named storage. Sparse own values precede the prototype.
#[inline(always)]
unsafe fn plain_index_get(receiver: f64, key: f64) -> Option<f64> {
    let n = if key.to_bits() >> 48 == 0x7FFE {
        key.to_bits() as i32 as f64
    } else {
        key
    };
    if !(n >= 0.0 && n < u32::MAX as f64 && n == n.trunc()) {
        return None;
    }
    let bits = receiver.to_bits();
    let floor = crate::value::addr_class::HANDLE_BAND_MAX as u64;
    if bits.wrapping_sub(crate::value::POINTER_TAG | floor) >= (1u64 << 48) - floor {
        return None;
    }
    let mut arr = (bits & crate::value::POINTER_MASK) as *const ArrayHeader;
    let mut header =
        (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*header).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    let mut steps = 0;
    while (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        // Growth/evacuation stubs carry the live payload in their first word.
        // The existing array kind owns this word; admit its target before any
        // header read, exactly as the checked element machinery does.
        let target = *(arr as *const usize);
        if target < floor as usize
            || target as u64 >= (1u64 << 48)
            || target == arr as usize
            || steps == 64
        {
            return None;
        }
        arr = target as *const ArrayHeader;
        header = (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_ARRAY {
            return None;
        }
        steps += 1;
    }
    let index = n as u32;
    if array_has_element_descriptors(arr) {
        return Some(array_spec_get(arr, index));
    }
    if index < (*arr).length {
        if index < (*arr).capacity {
            let bits = *crate::array::array_elements_ptr(arr).add(index as usize);
            if bits != crate::value::TAG_HOLE {
                return Some(f64::from_bits(bits));
            }
        } else if let Some(value) = array_sparse_index_property_get(arr, index) {
            return Some(value);
        }
    }
    Some(proto_chain::array_miss_index_get(arr, index))
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ARRAY_READ_COLD: unsafe extern "C" fn(f64, f64, u64) -> f64 =
    perry_array_read_cold_shared;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_hole_and_oob_do_not_reenter_the_element_accessor() {
        unsafe {
            let arr = js_array_alloc(4);
            (*arr).length = 2;
            let receiver = crate::value::js_nanbox_pointer(arr as i64);
            let before = test_element_accessor_calls();
            for key in [0.0, 1.0, 2.0, 100.0, 2147483648.0, 4294967294.0] {
                assert_eq!(
                    perry_array_read_cold_shared(receiver, key, arr as u64).to_bits(),
                    crate::value::TAG_UNDEFINED,
                );
            }
            assert_eq!(test_element_accessor_calls(), before);
        }
    }

    #[test]
    fn frozen_descriptor_metadata_does_not_shadow_dense_data() {
        unsafe {
            let arr = js_array_alloc(2);
            let arr = js_array_push_f64(arr, 4.0);
            let arr = js_array_push_f64(arr, 45.0);
            let scope = crate::gc::RuntimeHandleScope::new();
            let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(arr as i64));
            crate::object::js_object_freeze(receiver.get_nanbox_f64());
            let arr = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64())
                as *const ArrayHeader;
            assert_eq!(array_spec_get(arr, 1), 45.0);
            assert_eq!(
                perry_array_read_cold_shared(receiver.get_nanbox_f64(), 1.0, 2),
                45.0
            );
        }
    }

    #[test]
    fn numeric_consumer_coerces_an_absent_element_after_lookup() {
        unsafe {
            let arr = js_array_alloc(0);
            let receiver = crate::value::js_nanbox_pointer(arr as i64);
            assert!(perry_array_read_cold_shared(receiver, 9.0, arr as u64 | 1).is_nan());
        }
    }

    #[test]
    fn private_record_source_is_repaired_by_the_shared_forwarding_arm() {
        unsafe {
            let old = js_array_alloc(1);
            let first = js_array_push_f64(old, 10.0);
            let mut live = first;
            for index in 1..=32 {
                live = js_array_push_f64(live, index as f64);
            }
            let mut source = crate::value::js_nanbox_pointer(first as i64);
            assert_ne!(first, live);
            let mut args = ColdArgs {
                site: 0,
                flags: 2,
                source,
            };
            assert_eq!(
                perry_array_read_cold_shared(source, 32.0, &mut args as *mut ColdArgs as u64 | 6),
                32.0
            );
            source = args.source;
            assert_eq!(
                source.to_bits(),
                crate::value::js_nanbox_pointer(live as i64).to_bits()
            );
            let stale = crate::value::js_nanbox_pointer(first as i64);
            let mut missing = ColdArgs {
                site: 0,
                flags: 4,
                source: stale,
            };
            assert_eq!(
                perry_array_read_cold_shared(
                    stale,
                    100.0,
                    &mut missing as *mut ColdArgs as u64 | 6
                )
                .to_bits(),
                crate::value::TAG_UNDEFINED
            );
            assert_eq!(
                missing.source.to_bits(),
                crate::value::js_nanbox_pointer(live as i64).to_bits()
            );
        }
    }
}
