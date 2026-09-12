//! Public execution witnesses: complete JS values, original-input identity,
//! moving collection and actual JS longjmp cleanup, not Rust panic alone.
use super::*;
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::regex::perex_api;
use crate::regex::perex_memory::MemoryBudget;
use crate::regex::perex_owner::HeapSubject;
use crate::regex::perex_runtime as host;
use crate::regex::RegExpHeader;
use crate::string::StringHeader;
use crate::value::{js_nanbox_pointer, TAG_NULL, TAG_UNDEFINED};
use perex::binding::BoundSubject;
use perex::Budget;

pub(super) fn register_host_roots() {
    // The isolation guard removes the production registry. Restore both the
    // transient handles and the metadata families these public operations use.
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::array::scan_template_raw_roots_mut);
    // gc_init registers this in production. The isolation guard clears that
    // registry, so restore the intrinsic-address rewrites before callbacks
    // access arrays/objects after a protected copying collection.
    gc_register_mutable_root_scanner(crate::array::scan_prototype_addr_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_overflow_fields_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::descriptor_state::scan_descriptor_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_exotic_expando_roots_mut);
    gc_register_mutable_root_scanner(crate::regex::scan_last_exec_groups_root_mut);
    gc_register_mutable_root_scanner(crate::object::scan_implicit_this_roots_mut);
    gc_register_mutable_root_scanner(crate::closure::scan_singleton_closure_roots_mut);
    gc_register_mutable_root_scanner(crate::closure::scan_closure_dynamic_props_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    gc_register_mutable_root_scanner(crate::symbol::scan_symbol_side_table_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_current_new_target_root_mut);
    gc_register_mutable_root_scanner(crate::iter_result::scan_iter_result_keys_roots_mut);
}

fn text<'s>(scope: &'s RuntimeHandleScope, bytes: &[u8]) -> RuntimeHandle<'s> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        bytes.as_ptr(),
        bytes.len() as u32,
    ))
}

fn regex<'s>(scope: &'s RuntimeHandleScope, pattern: &str, flags: &str) -> RuntimeHandle<'s> {
    let pattern = text(scope, pattern.as_bytes());
    let flags = text(scope, flags.as_bytes());
    scope.root_raw_mut_ptr(pattern.with_const_ptr::<StringHeader, _>(|pattern| {
        flags.with_const_ptr::<StringHeader, _>(|flags| crate::regex::js_regexp_new(pattern, flags))
    }))
}

/// `js_regexp_exec` on the receiver and subject the handles currently hold.
fn exec(receiver: &RuntimeHandle<'_>, input: &RuntimeHandle<'_>) -> *mut ArrayHeader {
    receiver.with_mut_ptr::<RegExpHeader, _>(|receiver| {
        input.with_const_ptr::<StringHeader, _>(|input| {
            crate::regex::js_regexp_exec(receiver, input)
        })
    })
}

/// `js_regexp_test` on the receiver and subject the handles currently hold.
fn test(receiver: &RuntimeHandle<'_>, input: &RuntimeHandle<'_>) -> i32 {
    receiver.with_const_ptr::<RegExpHeader, _>(|receiver| {
        input.with_const_ptr::<StringHeader, _>(|input| {
            crate::regex::js_regexp_test(receiver, input)
        })
    })
}

fn address<T>(handle: &RuntimeHandle<'_>) -> usize {
    handle.with_const_ptr(|p: *const T| p as usize)
}

fn bytes(value: f64) -> Vec<u8> {
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (data, len) = crate::string::str_bytes_from_jsvalue(value, &mut scratch)
        .expect("expected a capture string");
    unsafe { std::slice::from_raw_parts(data, len as usize).to_vec() }
}

fn item(array: &RuntimeHandle<'_>, index: u32) -> f64 {
    array.with_const_ptr::<ArrayHeader, _>(|array| crate::array::js_array_get_f64(array, index))
}

fn array<'s>(scope: &'s RuntimeHandleScope, value: f64) -> RuntimeHandle<'s> {
    scope.root_raw_mut_ptr(crate::value::js_nanbox_get_pointer(value) as *mut ArrayHeader)
}

fn named(array: &RuntimeHandle<'_>, name: &str) -> f64 {
    array
        .with_const_ptr::<ArrayHeader, _>(|array| unsafe {
            crate::array::array_named_property_get_by_name(array, name)
        })
        .unwrap_or_else(|| panic!("exec result must have the named property {name}"))
}

fn field(object: &RuntimeHandle<'_>, name: &str) -> f64 {
    let scope = RuntimeHandleScope::new();
    let key = text(&scope, name.as_bytes());
    let object =
        crate::value::js_nanbox_get_pointer(object.get_nanbox_f64()) as *const ObjectHeader;
    key.with_const_ptr::<StringHeader, _>(|key| {
        f64::from_bits(crate::object::js_object_get_field_by_name(object, key).bits())
    })
}

#[test]
fn perex_public_exec_preserves_half_pairs_unset_groups_indices_and_input_identity() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "(?<letter>.)(?<optional>x)?", "d");
    let input = text(&scope, "😀".as_bytes());
    let result = exec(&receiver, &input);
    assert!(!result.is_null());
    let result = scope.root_raw_mut_ptr(result);
    assert!(result.with_const_ptr::<ArrayHeader, _>(|result| unsafe {
        crate::array::array_named_property_names(result, true)
            .iter()
            .any(|name| name == "indices")
    }));
    let original = address::<ArrayHeader>(&result);
    gc_collect_minor();
    assert_ne!(address::<ArrayHeader>(&result), original);
    assert_eq!(bytes(item(&result, 0)), [0xed, 0xa0, 0xbd]);
    assert_eq!(bytes(item(&result, 1)), [0xed, 0xa0, 0xbd]);
    assert_eq!(item(&result, 2).to_bits(), TAG_UNDEFINED);
    assert_eq!(named(&result, "index"), 0.0);
    assert_eq!(
        named(&result, "input").to_bits(),
        input.with_const_ptr(|input: *const StringHeader| {
            crate::value::js_nanbox_string(input as i64).to_bits()
        })
    );
    let groups = scope.root_nanbox_f64(named(&result, "groups"));
    assert_eq!(
        crate::object::js_object_get_prototype_of(groups.get_nanbox_f64()).to_bits(),
        TAG_NULL
    );
    assert_eq!(bytes(field(&groups, "letter")), [0xed, 0xa0, 0xbd]);
    assert_eq!(field(&groups, "optional").to_bits(), TAG_UNDEFINED);
    let indices = array(&scope, named(&result, "indices"));
    let index_groups = scope.root_nanbox_f64(named(&indices, "groups"));
    assert_eq!(
        crate::object::js_object_get_prototype_of(index_groups.get_nanbox_f64()).to_bits(),
        TAG_NULL
    );
    let pair = array(&scope, item(&indices, 1));
    assert_eq!((item(&pair, 0), item(&pair, 1)), (0.0, 1.0));
    assert_eq!(
        field(&index_groups, "letter").to_bits(),
        pair.with_const_ptr(|pair: *const ArrayHeader| js_nanbox_pointer(pair as i64).to_bits())
    );
    assert_eq!(field(&index_groups, "optional").to_bits(), TAG_UNDEFINED);
    assert_eq!(item(&indices, 2).to_bits(), TAG_UNDEFINED);
}

#[test]
fn perex_public_exec_uses_installed_program_and_exact_duplicate_or_astral_names() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_host_roots();
    for (pattern, key, subject, capture) in [
        ("(?<same>a)|(?<same>b)", "same", "b", 2),
        ("(?<𝒜>a)", "𝒜", "a", 1),
        ("(?<__proto__>a)", "__proto__", "a", 1),
    ] {
        let scope = RuntimeHandleScope::new();
        // This incompatible legacy source cannot produce these results. The
        // witness tests execution of the installed Perex program; constructor
        // support for these patterns is deliberately not inferred from it.
        let receiver = regex(&scope, "NEVER", "d");
        let source = text(&scope, pattern.as_bytes());
        let source = BoundSubject::new(unsafe { HeapSubject::new(source).unwrap() }).unwrap();
        let memory = MemoryBudget::new(1 << 20);
        let mut budget = Budget::new(1_000_000);
        let program = host::compile(
            &scope,
            &source,
            crate::regex::validate_and_canonicalize_flags("d"),
            &mut budget,
            &memory,
            1 << 20,
            &mut host::poll,
        )
        .unwrap();
        unsafe {
            program.install(&receiver);
        }
        let before =
            receiver.with_const_ptr::<RegExpHeader, _>(|r| unsafe { (*r).perex_program as usize });
        let input = text(&scope, subject.as_bytes());
        let cycles = copying_minor_cycles();
        let found = receiver
            .with_mut_ptr::<RegExpHeader, _>(|receiver| {
                input.with_const_ptr::<StringHeader, _>(|input| {
                    perex_api::execute(receiver, input, true, &mut || {
                        gc_collect_minor();
                        Ok(())
                    })
                })
            })
            .unwrap()
            .unwrap();
        let result = scope.root_raw_mut_ptr(found.array);
        assert!(copying_minor_cycles() > cycles);
        assert_ne!(
            receiver.with_const_ptr::<RegExpHeader, _>(|r| unsafe { (*r).perex_program as usize }),
            before
        );
        let groups = scope.root_nanbox_f64(named(&result, "groups"));
        assert_eq!(bytes(field(&groups, key)), subject.as_bytes());
        assert_eq!(
            crate::object::js_object_get_prototype_of(groups.get_nanbox_f64()).to_bits(),
            TAG_NULL
        );
        let indices = array(&scope, named(&result, "indices"));
        let groups = scope.root_nanbox_f64(named(&indices, "groups"));
        let named_pair = field(&groups, key);
        assert_eq!(named_pair.to_bits(), item(&indices, capture).to_bits());
    }
}

#[test]
fn perex_public_exec_and_test_share_lastindex_and_empty_match_behavior() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = text(&scope, b"ba");
    let receiver = regex(&scope, "a", "g");
    assert_eq!(test(&receiver, &input), 1);
    assert_eq!(
        receiver.with_const_ptr(|p| crate::regex::regex_last_index_offset(p)),
        2
    );
    assert_eq!(test(&receiver, &input), 0);
    assert_eq!(
        receiver.with_const_ptr(|p| crate::regex::regex_last_index_offset(p)),
        0
    );
    let result = exec(&receiver, &input);
    assert!(!result.is_null());
    let result = scope.root_raw_mut_ptr(result);
    assert_eq!(named(&result, "index"), 1.0);
    assert_eq!(named(&result, "groups").to_bits(), TAG_UNDEFINED);
    let receiver = regex(&scope, "(?:)", "g");
    let input = text(&scope, "😀".as_bytes());
    receiver.with_mut_ptr::<RegExpHeader, _>(|receiver| unsafe {
        (*receiver).last_index = 1.0f64.to_bits();
    });
    let result = exec(&receiver, &input);
    assert!(!result.is_null());
    let result = scope.root_raw_mut_ptr(result);
    assert_eq!(bytes(item(&result, 0)), b"");
    assert_eq!(named(&result, "index"), 1.0);
    assert_eq!(
        receiver.with_const_ptr(|p| crate::regex::regex_last_index_offset(p)),
        1
    );
}

#[test]
fn perex_public_throwing_lastindex_write_releases_native_scratch_and_roots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "(a)", "gd");
    let input = text(&scope, b"a");
    assert_eq!(test(&receiver, &input), 1);
    receiver.with_mut_ptr::<RegExpHeader, _>(|receiver| {
        unsafe { (*receiver).last_index = 0.0f64.to_bits() };
        crate::object::set_property_attrs(
            receiver as usize,
            "lastIndex".to_string(),
            crate::object::PropertyAttrs::new(false, false, false),
        );
    });
    let live = external_side_live_bytes();
    let roots = RuntimeHandleScope::active_len_for_tests();
    let thrown = crate::exception::catch_js_throw(|| exec(&receiver, &input))
        .expect_err("matching must throw when its lastIndex write is forbidden");
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(
        external_side_live_bytes(),
        live,
        "capture/scratch owners must survive the local trap and then be dropped"
    );
    let thrown = scope.root_nanbox_f64(thrown);
    assert_eq!(bytes(field(&thrown, "name")), b"TypeError");
    assert_eq!(
        receiver.with_const_ptr(|p| crate::regex::regex_last_index_offset(p)),
        0
    );
}

extern "C" fn throw_on_coercion(_closure: *const crate::closure::ClosureHeader) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(731.0)
}

#[test]
fn perex_public_nonglobal_test_propagates_lastindex_coercion_throw() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_host_roots();
    let scope = RuntimeHandleScope::new();
    let receiver = regex(&scope, "a", "");
    let input = text(&scope, b"a");
    let coercer = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    let key = text(&scope, b"valueOf");
    let fp = throw_on_coercion as *const u8;
    crate::closure::js_register_closure_arity(fp, 0);
    let closure = crate::closure::js_closure_alloc_singleton(fp);
    coercer.with_mut_ptr::<ObjectHeader, _>(|coercer| {
        key.with_const_ptr::<StringHeader, _>(|key| {
            crate::object::js_object_set_field_by_name(
                coercer,
                key,
                js_nanbox_pointer(closure as i64),
            )
        })
    });
    // Re-read after the store above, which may have grown and moved `coercer`.
    coercer.with_mut_ptr::<ObjectHeader, _>(|coercer| {
        receiver.with_mut_ptr::<RegExpHeader, _>(|receiver| unsafe {
            (*receiver).last_index = js_nanbox_pointer(coercer as i64).to_bits();
        })
    });
    let before = address::<StringHeader>(&input);
    let roots = RuntimeHandleScope::active_len_for_tests();
    let error = crate::exception::catch_js_throw(|| test(&receiver, &input)).unwrap_err();
    assert_eq!(error, 731.0);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_ne!(address::<StringHeader>(&input), before);
}
