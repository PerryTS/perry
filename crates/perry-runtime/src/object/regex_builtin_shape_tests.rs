use super::*;

#[test]
fn regexp_builtin_shape_rejects_overrides() {
    const CHILD: &str = "PERRY_TEST_REGEXP_BUILTIN_SHAPE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        for kind in [
            "own", "flags", "symbol", "exec", "test", "order", "accessor",
        ] {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "object::regex_read_sites::tests::regexp_builtin_shape_rejects_overrides",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env(CHILD, kind)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{kind}: {}
{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        return;
    }
    let _stable = crate::gc::GcSuppressScope::new();
    let new = || crate::regex::test_construct_regexp_and_exec_once("a", "g");
    let value = |r| crate::value::js_nanbox_pointer(r as i64);
    unsafe {
        let re = new();
        assert!(builtin_behavior(value(re)), "pristine proof must be live");
        assert!(
            builtin_behavior(value(re)),
            "the primed shape proof must hit"
        );
        let proto = object_receiver(crate::regex::intrinsic_prototype()).unwrap();
        let other = super::super::js_object_get_field_by_name_f64(
            proto,
            crate::string::js_string_from_str("toString"),
        );
        match std::env::var(CHILD).unwrap().as_str() {
            "own" => {
                super::super::js_object_set_field_by_name(
                    re,
                    crate::string::js_string_from_str("exec"),
                    other,
                );
                assert!(
                    !builtin_behavior(value(re)),
                    "own exec must retire receiver proof"
                );
                let fresh = new();
                assert!(
                    builtin_behavior(value(fresh)),
                    "own override is receiver-local"
                );
                let symbol = crate::symbol::well_known_symbol("replace") as usize;
                super::super::shaped_symbols::define(fresh as usize, symbol, other.to_bits(), 0);
                assert!(
                    !builtin_behavior(value(fresh)),
                    "own symbol must retire receiver proof"
                );
            }
            "flags" => {
                super::super::set_accessor_descriptor(
                    proto as usize,
                    "flags".into(),
                    super::super::AccessorDescriptor {
                        get: other.to_bits(),
                        set: 0,
                    },
                );
                assert!(
                    !builtin_behavior(value(re)),
                    "prototype getter must retire holder proof"
                );
                assert!(
                    !flag_getters(value(re)),
                    "a replaced flags accessor must retire the flag proof"
                );
            }
            "symbol" => {
                let symbol = crate::symbol::well_known_symbol("replace") as usize;
                let entry = super::super::shaped_symbols::entry(proto as usize, symbol).unwrap();
                let before = super::super::shapes::object_shape_stamp(proto);
                super::super::shaped_symbols::define(
                    proto as usize,
                    symbol,
                    other.to_bits(),
                    entry,
                );
                assert_ne!(
                    super::super::shapes::object_shape_stamp(proto),
                    before,
                    "a value-only symbol store must revoke its ConstFn shape"
                );
                assert!(
                    !builtin_behavior(value(re)),
                    "prototype symbol value must retire holder proof"
                );
            }
            "test" => {
                super::super::js_object_set_field_by_name(
                    proto,
                    crate::string::js_string_from_str("test"),
                    other,
                );
                assert!(
                    !builtin_behavior(value(re)),
                    "test must retire the aggregate proof"
                );
                assert!(!test(value(re)), "the window shortcut must observe test");
                assert!(
                    flag_getters(value(re)),
                    "a write that touches no flag accessor keeps the flag proof"
                );
                assert!(
                    exec_is_builtin(value(re)),
                    "the narrower exec proof remains valid"
                );
            }
            "exec" => {
                super::super::js_object_set_field_by_name(
                    proto,
                    crate::string::js_string_from_str("exec"),
                    other,
                );
                assert!(
                    !builtin_behavior(value(re)),
                    "prototype exec value must retire holder proof"
                );
                assert!(flag_getters(value(re)), "exec is not a flag accessor");
            }
            "order" => {
                let getters = [
                    "hasIndices",
                    "global",
                    "ignoreCase",
                    "multiline",
                    "dotAll",
                    "unicode",
                    "unicodeSets",
                    "sticky",
                ];
                let infos = [
                    crate::fn_info!(get_d, 0),
                    crate::fn_info!(get_g, 0),
                    crate::fn_info!(get_i, 0),
                    crate::fn_info!(get_m, 0),
                    crate::fn_info!(get_s, 0),
                    crate::fn_info!(get_u, 0),
                    crate::fn_info!(get_v, 0),
                    crate::fn_info!(get_y, 0),
                ];
                for (name, info) in getters.into_iter().zip(infos) {
                    let getter = crate::closure::js_closure_alloc(info, 0);
                    super::super::set_accessor_descriptor(
                        re as usize,
                        name.into(),
                        super::super::AccessorDescriptor {
                            get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
                            set: 0,
                        },
                    );
                }
                assert!(!builtin_behavior(value(re)));
                crate::regex::perex_match_search::flags(value(re)).unwrap();
                READ_ORDER.with(|order| {
                    assert_eq!(
                        *order.borrow(),
                        [
                            "hasIndices",
                            "global",
                            "ignoreCase",
                            "multiline",
                            "dotAll",
                            "unicode",
                            "unicodeSets",
                            "sticky"
                        ]
                    )
                });
            }
            "accessor" => {
                // An unrelated write retires the aggregate fact only; the
                // proof of the flag accessors survives until one is replaced.
                super::super::js_object_set_field_by_name(
                    proto,
                    crate::string::js_string_from_str("unrelated"),
                    other,
                );
                assert!(!builtin_behavior(value(re)));
                assert!(flag_getters(value(re)));
                let getter = crate::closure::js_closure_alloc(crate::fn_info!(get_g, 0), 0);
                super::super::set_accessor_descriptor(
                    proto as usize,
                    "global".into(),
                    super::super::AccessorDescriptor {
                        get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
                        set: 0,
                    },
                );
                assert!(
                    !flag_getters(value(re)),
                    "a replaced flag getter retires the flag proof"
                );
                crate::regex::perex_match_search::flags(value(re)).unwrap();
                READ_ORDER.with(|order| assert!(order.borrow().contains(&"global")));
            }
            _ => unreachable!(),
        }
    }
}

crate::perry_thread_local! {
    static READ_ORDER: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
}
macro_rules! getter {
    ($name:ident, $key:literal) => {
        extern "C" fn $name(
            _: *const crate::closure::ClosureHeader,
            _: crate::closure::JsThis,
        ) -> f64 {
            READ_ORDER.with(|order| order.borrow_mut().push($key));
            f64::from_bits(crate::value::TAG_FALSE)
        }
    };
}
getter!(get_d, "hasIndices");
getter!(get_g, "global");
getter!(get_i, "ignoreCase");
getter!(get_m, "multiline");
getter!(get_s, "dotAll");
getter!(get_u, "unicode");
getter!(get_v, "unicodeSets");
getter!(get_y, "sticky");
