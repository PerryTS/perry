use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::ObjectHeader;
use crate::value::JSValue;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn text(value: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64)
}

fn type_options(scope: &RuntimeHandleScope, value: f64) -> f64 {
    let value = scope.root_nanbox_f64(value);
    let options = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let key = scope.root_string_ptr(js_string_from_bytes(b"type".as_ptr(), 4));
    options.with_mut_ptr(|options| {
        key.with_const_ptr(|key| {
            crate::object::js_object_set_field_by_name(options, key, value.get_nanbox_f64())
        })
    });
    options
        .with_mut_ptr(|options: *mut ObjectHeader| crate::value::js_nanbox_pointer(options as i64))
}

fn property(object: f64, name: &str) -> f64 {
    let scope = RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let key = scope.root_string_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
    key.with_const_ptr(|key| {
        crate::object::js_object_get_field_by_name_f64(
            JSValue::from_bits(object.get_nanbox_f64().to_bits()).as_pointer::<ObjectHeader>(),
            key,
        )
    })
}

fn property_text(scope: &RuntimeHandleScope, object: &RuntimeHandle<'_>, name: &str) -> String {
    let value = scope.root_nanbox_f64(property(object.get_nanbox_f64(), name));
    crate::builtins::jsvalue_string_content(value.get_nanbox_f64()).unwrap()
}

fn thrown<'a>(scope: &'a RuntimeHandleScope, path: f64, options: f64) -> RuntimeHandle<'a> {
    let path = scope.root_nanbox_f64(path);
    let options = scope.root_nanbox_f64(options);
    let result = crate::exception::catch_js_throw(|| {
        js_fs_open_as_blob(path.get_nanbox_f64(), options.get_nanbox_f64())
    });
    scope.root_nanbox_f64(result.expect_err("validation must throw before returning a Promise"))
}

fn fulfilled<'a>(scope: &'a RuntimeHandleScope, value: f64) -> RuntimeHandle<'a> {
    let promise = scope.root_nanbox_f64(value);
    let tagged = JSValue::from_bits(promise.get_nanbox_f64().to_bits());
    assert!(tagged.is_pointer());
    let pointer = tagged.as_pointer::<crate::Promise>().cast_mut();
    assert_eq!(crate::promise::js_is_promise(pointer), 1);
    assert_eq!(crate::promise::js_promise_state(pointer), 1);
    scope.root_nanbox_f64(crate::promise::js_promise_value(pointer))
}

struct OwnedData {
    directory: PathBuf,
    file: PathBuf,
    missing: PathBuf,
}

impl OwnedData {
    fn new() -> Self {
        static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "perry-openasblob-{}-{stamp}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("valid.txt");
        std::fs::write(&file, b"valid").unwrap();
        let missing = directory.join("missing.txt");
        Self {
            directory,
            file,
            missing,
        }
    }

    fn remove(self) {
        assert_eq!(std::fs::read(&self.file).unwrap(), b"valid");
        assert!(!self.missing.exists());
        std::fs::remove_file(self.file).unwrap();
        std::fs::remove_dir(self.directory).unwrap();
    }
}

fn initialize() {
    crate::gc::gc_init();
    crate::object::static_shapes::js_shape_run_static_seed();
}

#[test]
fn falsy_nonstring_types_throw_before_path_validation() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    let scope = RuntimeHandleScope::new();
    for (value, received) in [
        (f64::from_bits(crate::value::TAG_NULL), "null"),
        (
            f64::from_bits(crate::value::TAG_FALSE),
            "type boolean (false)",
        ),
        (0.0, "type number (0)"),
        (f64::NAN, "type number (NaN)"),
    ] {
        let options = scope.root_nanbox_f64(type_options(&scope, value));
        let error = thrown(&scope, 1.0, options.get_nanbox_f64());
        assert_eq!(property_text(&scope, &error, "name"), "TypeError");
        assert_eq!(
            property_text(&scope, &error, "code"),
            "ERR_INVALID_ARG_TYPE"
        );
        assert_eq!(
            property_text(&scope, &error, "message"),
            format!("The \"options.type\" property must be of type string. Received {received}")
        );
    }
}

#[test]
fn options_and_type_validation_precede_path_validation_and_stat() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    let scope = RuntimeHandleScope::new();
    let data = OwnedData::new();
    let missing = scope.root_nanbox_f64(text(data.missing.to_str().unwrap()));
    let options = scope.root_nanbox_f64(type_options(
        &scope,
        f64::from_bits(crate::value::TAG_FALSE),
    ));
    let error = thrown(&scope, missing.get_nanbox_f64(), options.get_nanbox_f64());
    assert_eq!(
        property_text(&scope, &error, "code"),
        "ERR_INVALID_ARG_TYPE"
    );
    assert_eq!(
        property_text(&scope, &error, "message"),
        "The \"options.type\" property must be of type string. Received type boolean (false)"
    );
    let error = thrown(&scope, 1.0, f64::from_bits(crate::value::TAG_NULL));
    assert_eq!(
        property_text(&scope, &error, "code"),
        "ERR_INVALID_ARG_TYPE"
    );
    assert_eq!(
        property_text(&scope, &error, "message"),
        "The \"options\" argument must be of type object. Received null"
    );
    let error = thrown(&scope, 1.0, f64::from_bits(crate::value::TAG_UNDEFINED));
    assert_eq!(
        property_text(&scope, &error, "code"),
        "ERR_INVALID_ARG_TYPE"
    );
    assert_eq!(
        property_text(&scope, &error, "message"),
        "The \"path\" argument must be of type string or an instance of Buffer or URL. Received type number (1)"
    );
    data.remove();
}

#[test]
fn missing_path_keeps_stat_error_metadata_without_mutation() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    let scope = RuntimeHandleScope::new();
    let data = OwnedData::new();
    let missing = data.missing.to_str().unwrap();
    let error = thrown(
        &scope,
        text(missing),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert_eq!(property_text(&scope, &error, "name"), "Error");
    assert_eq!(property_text(&scope, &error, "code"), "ENOENT");
    assert_eq!(property_text(&scope, &error, "syscall"), "stat");
    assert_eq!(property_text(&scope, &error, "path"), missing);
    assert_eq!(property(error.get_nanbox_f64(), "errno"), -2.0);
    assert_eq!(
        property_text(&scope, &error, "message"),
        format!("ENOENT: no such file or directory, stat '{missing}'")
    );
    assert!(JSValue::from_bits(property(error.get_nanbox_f64(), "dest").to_bits()).is_undefined());
    data.remove();
}

#[test]
fn valid_types_resolve_promises_with_file_content_and_original_casing() {
    let _lock = crate::gc::global_side_table_test_lock();
    initialize();
    let scope = RuntimeHandleScope::new();
    let data = OwnedData::new();
    let path = scope.root_nanbox_f64(text(data.file.to_str().unwrap()));
    for case in 0..4 {
        let expected_type = if case == 3 { "Text/Plain" } else { "" };
        let options = if case == 0 {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            let value = if case == 1 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                text(expected_type)
            };
            type_options(&scope, value)
        };
        let options = scope.root_nanbox_f64(options);
        let result = crate::exception::catch_js_throw(|| {
            js_fs_open_as_blob(path.get_nanbox_f64(), options.get_nanbox_f64())
        })
        .expect("valid options must return a Promise without throwing");
        let blob = fulfilled(&scope, result);
        assert_eq!(property_text(&scope, &blob, "type"), expected_type);
        assert_eq!(property(blob.get_nanbox_f64(), "size"), 5.0);
        let method = scope.root_nanbox_f64(property(blob.get_nanbox_f64(), "text"));
        let content = crate::closure::js_closure_call0(
            JSValue::from_bits(method.get_nanbox_f64().to_bits()).as_pointer::<ClosureHeader>(),
            crate::closure::plain_call_receiver(),
        );
        let content = fulfilled(&scope, content);
        assert_eq!(
            crate::builtins::jsvalue_string_content(content.get_nanbox_f64()).as_deref(),
            Some("valid")
        );
    }
    data.remove();
}
