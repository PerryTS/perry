use super::*;

extern "C" fn from_fixture(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _items: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn early_realm_bootstrap_does_not_preempt_error_static_program_atoms() {
    let _lock = crate::gc::global_side_table_test_lock();
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            let _no_move = crate::gc::GcSuppressScope::new();
            // The streams provider installs this static before module literals
            // register. Exercise its actual runtime entry on a fresh agent.
            crate::external_family::provide_from_for_tests(crate::fn_info!(
                from_fixture,
                1
            ));
            let ctor = crate::object::js_get_global_this_builtin_value(b"Error".as_ptr(), 5);
            let ctor_addr = crate::value::js_nanbox_get_pointer(ctor) as usize;
            for name in [
                "isError",
                "captureStackTrace",
                "prepareStackTrace",
                "stackTraceLimit",
            ] {
                let bytes = name.as_bytes();
                let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
                assert!(
                    !atom_lookup(bytes, hash)
                        .is_some_and(|atom| { is_program_atom(atom) && !is_image_string(atom) }),
                    "bootstrap minted a program atom for {name} before the program registered it"
                );
                // A separate immutable source image arrives after bootstrap.
                // Bypass the canonicalizing pool, as emitted literal images do.
                let image = program_atom_image(bytes.as_ptr(), bytes.len() as u32, hash);
                js_string_register_property_atom(image);
                assert_eq!(atom_lookup(bytes, hash), Some(image as *const StringHeader));
                let expected = crate::closure::closure_get_own_dynamic_prop(ctor_addr, name)
                    .expect("Error static is an own property");
                let probed = crate::object::layout_probe::perry_probe_get(
                    crate::object::layout_probe::test_dir(),
                    ctor_addr.wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
                    crate::JSValue::string_ptr(image).bits(),
                    hash,
                );
                assert_eq!(
                    probed.to_bits(),
                    expected.to_bits(),
                    "probe of Error.{name}"
                );
                if name == "stackTraceLimit" {
                    assert_eq!(expected, 10.0);
                } else {
                    assert!(crate::closure::is_closure_bits(expected.to_bits()));
                }
            }
            let bag = (*(ctor_addr as *const crate::closure::ClosureHeader)).props;
            let descriptor = crate::object::shapes::object_shape_descriptor(bag).unwrap();
            assert_eq!(descriptor.logical_key_count, 7);
            assert_eq!(
                descriptor.live_inline_slot_count, 7,
                "one complete static bag"
            );
        })
        .expect("spawn fresh realm")
        .join()
        .expect("early realm bootstrap invariant");
}
