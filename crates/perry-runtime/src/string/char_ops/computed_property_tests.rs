use super::*;
use crate::value::{js_dyn_index_get, js_nanbox_string, JSValue};

fn string(value: &str) -> f64 {
    js_nanbox_string(js_string_from_str(value) as i64)
}

#[test]
fn computed_string_method_is_the_prototype_function() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(crate::object::builtin_prototype_value("String"));
    for receiver in [
        string("abcdef"),
        f64::from_bits(JSValue::try_short_string(b"abc").unwrap().bits()),
    ] {
        let receiver = scope.root_nanbox_f64(receiver);
        for name in ["charAt", "trim", "toUpperCase", "toString", "constructor"] {
            let key = scope.root_nanbox_f64(string(name));
            let expected = scope.root_nanbox_f64(crate::proxy::js_reflect_get(
                proto.get_nanbox_f64(),
                key.get_nanbox_f64(),
                proto.get_nanbox_f64(),
            ));
            assert_ne!(
                expected.get_nanbox_f64().to_bits(),
                crate::value::TAG_UNDEFINED,
                "prototype has {name}"
            );
            for get in [js_string_index_get_boxed, js_dyn_index_get] {
                let actual = get(receiver.get_nanbox_f64(), key.get_nanbox_f64());
                assert_eq!(
                    actual.to_bits(),
                    expected.get_nanbox_f64().to_bits(),
                    "{name} must preserve method identity"
                );
            }
        }
    }
}

#[test]
fn computed_string_own_properties_keep_index_semantics() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(string("abcdef"));
    for get in [js_string_index_get_boxed, js_dyn_index_get] {
        assert_eq!(get(receiver.get_nanbox_f64(), string("length")), 6.0);
        for key in [1.0, string("1")] {
            let value = get(receiver.get_nanbox_f64(), key);
            assert_eq!(
                crate::builtins::jsvalue_string_content(value).as_deref(),
                Some("b")
            );
        }
        for key in [
            -1.0,
            1.5,
            6.0,
            f64::NAN,
            f64::INFINITY,
            string("01"),
            string("1.0"),
            string("missing"),
        ] {
            assert_eq!(
                get(receiver.get_nanbox_f64(), key).to_bits(),
                crate::value::TAG_UNDEFINED
            );
        }
    }
}
