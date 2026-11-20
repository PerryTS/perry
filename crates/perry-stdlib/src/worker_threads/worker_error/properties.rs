use super::*;
use perry_runtime::gc::RuntimeHandle;

#[derive(Default)]
pub(super) struct Properties {
    attrs: Vec<(&'static str, [bool; 3])>,
    code: Option<SerializedValue>,
    cause: Option<Box<Properties>>,
    circular: bool,
}

impl Properties {
    fn record(&mut self, key: &'static str, flags: [bool; 3]) {
        if let Some((_, saved)) = self.attrs.iter_mut().find(|(name, _)| *name == key) {
            *saved = flags;
        } else {
            self.attrs.push((key, flags));
        }
    }

    pub(super) fn restore(&self, value: f64) {
        let scope = RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        if self.circular {
            perry_runtime::object::js_object_set_prototype_of(
                value.get_nanbox_f64(),
                f64::from_bits(JSValue::null().bits()),
            );
            return;
        }
        if let Some(code) = &self.code {
            let code = scope.root_nanbox_f64(f64::from_bits(unsafe {
                deserialize_nanbox_on_current_thread(code)
            }));
            let key = js_string_from_bytes(b"code".as_ptr(), 4);
            perry_runtime::object::js_object_set_field_by_name(
                perry_runtime::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as *mut _,
                key,
                code.get_nanbox_f64(),
            );
        }
        if let Some(cause) = &self.cause {
            cause.restore(get_object_field_from_value(value.get_nanbox_f64(), "cause"));
        }
        for (key, [enumerable, configurable, writable]) in &self.attrs {
            let property =
                scope.root_nanbox_f64(get_object_field_from_value(value.get_nanbox_f64(), key));
            let descriptor = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
            set_object_field(
                descriptor.get_raw_mut_ptr(),
                "value",
                property.get_nanbox_f64(),
            );
            for (name, flag) in [
                ("enumerable", enumerable),
                ("configurable", configurable),
                ("writable", writable),
            ] {
                set_object_field(
                    descriptor.get_raw_mut_ptr(),
                    name,
                    f64::from_bits(JSValue::bool(*flag).bits()),
                );
            }
            let key = scope.root_nanbox_f64(string_value(key));
            perry_runtime::object::js_object_define_property(
                value.get_nanbox_f64(),
                key.get_nanbox_f64(),
                object_value(descriptor.get_raw_mut_ptr()),
            );
        }
    }
}

fn field(value: f64, key: &str) -> f64 {
    get_object_field_from_value(value, key)
}

fn has(value: f64, key: &str) -> bool {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_nanbox_f64(string_value(key));
    perry_runtime::object::js_object_has_own(value.get_nanbox_f64(), key.get_nanbox_f64()).to_bits()
        == JSValue::bool(true).bits()
}

pub(super) fn project<'s>(
    thrown: &RuntimeHandle<'s>,
    scope: &'s RuntimeHandleScope,
    seen: &mut Vec<RuntimeHandle<'s>>,
) -> Result<WorkerErrorMessage, ()> {
    let kind = string_header_to_string(perry_runtime::builtins::js_value_typeof(
        thrown.get_nanbox_f64(),
    ));
    let admitted = kind.as_deref() == Some("object")
        && diagnostic_string(unsafe {
            perry_runtime::object::object_prototype_to_string(thrown.get_nanbox_f64())
        })
        .as_deref()
            == Some(b"[object Error]");
    let Some(family) = admitted
        .then(|| error_family(thrown.get_nanbox_f64()))
        .flatten()
    else {
        return Ok(WorkerErrorMessage {
            value: cloned_error(thrown.get_nanbox_f64()),
            properties: Properties::default(),
        });
    };
    if seen
        .iter()
        .any(|value| value.get_nanbox_u64() == thrown.get_nanbox_u64())
    {
        return Ok(WorkerErrorMessage {
            value: SerializedValue::Object {
                final_constfn: None,
                class_id: 0,
                parent_class_id: 0,
                fields: Vec::new(),
                keys: Some(Vec::new()),
            },
            properties: Properties {
                circular: true,
                ..Properties::default()
            },
        });
    }
    seen.push(scope.root_nanbox_f64(thrown.get_nanbox_f64()));
    let mut owners = Vec::new();
    let mut owner = scope.root_nanbox_f64(thrown.get_nanbox_f64());
    while owner.get_nanbox_u64() != JSValue::null().bits() && !is_undefined(owner.get_nanbox_f64())
    {
        if owners
            .iter()
            .any(|prior: &RuntimeHandle<'_>| prior.get_nanbox_u64() == owner.get_nanbox_u64())
        {
            break;
        }
        owners.push(scope.root_nanbox_f64(owner.get_nanbox_f64()));
        owner = scope.root_nanbox_f64(
            perry_runtime::exception::catch_js_throw(|| {
                perry_runtime::object::js_object_get_prototype_of(owner.get_nanbox_f64())
            })
            .map_err(|_| ())?,
        );
    }
    let mut message = None;
    let mut stack = None;
    let mut cause = None;
    let mut properties = Properties::default();
    let mut code_seen = false;
    let mut code_value = None;
    // Enumerate each owner only after its ancestors: a getter may change
    // the descendant's keys, descriptors, or diagnostic values.
    for owner in owners.iter().rev() {
        let names = scope.root_nanbox_f64(
            perry_runtime::exception::catch_js_throw(|| {
                perry_runtime::object::js_object_get_own_property_names(owner.get_nanbox_f64())
            })
            .map_err(|_| ())?,
        );
        let len = perry_runtime::array::js_array_length(
            perry_runtime::value::js_nanbox_get_pointer(names.get_nanbox_f64()) as *const _,
        );
        for index in 0..len {
            let key = scope.root_nanbox_f64(perry_runtime::array::js_array_get_f64(
                perry_runtime::value::js_nanbox_get_pointer(names.get_nanbox_f64()) as *const _,
                index,
            ));
            let name = string_value_to_string(key.get_nanbox_f64()).unwrap_or_default();
            let descriptor = scope.root_nanbox_f64(
                perry_runtime::exception::catch_js_throw(|| {
                    perry_runtime::object::js_object_get_own_property_descriptor(
                        owner.get_nanbox_f64(),
                        key.get_nanbox_f64(),
                    )
                })
                .map_err(|_| ())?,
            );
            if is_undefined(descriptor.get_nanbox_f64()) {
                continue;
            }
            code_seen |= name == "code";
            let accessor = has(descriptor.get_nanbox_f64(), "get");
            let flags = ["enumerable", "configurable", "writable"].map(|key| {
                perry_runtime::value::js_is_truthy(field(descriptor.get_nanbox_f64(), key)) != 0
            });
            let getter = scope.root_nanbox_f64(field(descriptor.get_nanbox_f64(), "get"));
            let value = if accessor && !is_undefined(getter.get_nanbox_f64()) && name != "__proto__"
            {
                match perry_runtime::exception::catch_js_throw(|| unsafe {
                    perry_runtime::closure::call_value(
                        getter.get_nanbox_f64(),
                        perry_runtime::closure::JsThis::from_f64(thrown.get_nanbox_f64()),
                        &[],
                    )
                }) {
                    Ok(value) => scope.root_nanbox_f64(value),
                    Err(_) if name == "cause" => return Err(()),
                    Err(_) => continue,
                }
            } else if accessor {
                if name == "cause" {
                    return Err(());
                }
                continue;
            } else {
                scope.root_nanbox_f64(field(descriptor.get_nanbox_f64(), "value"))
            };
            match name.as_str() {
                "message" => {
                    if let Some(text) = diagnostic_string(value.get_nanbox_f64()) {
                        message = Some(text);
                        properties.record("message", flags);
                    }
                }
                "stack" => {
                    if let Some(text) = diagnostic_string(value.get_nanbox_f64()) {
                        stack = Some(text);
                        properties.record("stack", flags);
                    }
                }
                "code" => {
                    code_value = Some(value);
                    properties.record("code", flags);
                }
                "cause" => {
                    let child = if error_family(value.get_nanbox_f64()).is_some() {
                        Some(project(&value, scope, seen)?)
                    } else {
                        cloned_diagnostic(value.get_nanbox_f64()).map(|value| WorkerErrorMessage {
                            value,
                            properties: Properties::default(),
                        })
                    };
                    if let Some(child) = child {
                        cause = Some(Box::new(child.value));
                        properties.cause = Some(Box::new(child.properties));
                        properties.record("cause", flags);
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(value) = code_value {
        // Keep the selected reference alive until later diagnostic getters
        // finish; those getters can still mutate the referenced object.
        properties.code = cloned_diagnostic(value.get_nanbox_f64());
        if properties.code.is_none() {
            properties.attrs.retain(|(key, _)| *key != "code");
        }
    }
    if !code_seen {
        // Native diagnostics can expose code through named access without a
        // reflected own key. Scripted code getters were already visited.
        let key = scope.root_raw_mut_ptr(js_string_from_bytes(b"code".as_ptr(), 4));
        let code = perry_runtime::exception::catch_js_throw(|| {
            perry_runtime::object::js_object_get_field_by_name_f64(
                perry_runtime::value::js_nanbox_get_pointer(thrown.get_nanbox_f64()) as *const _,
                key.get_raw_const_ptr(),
            )
        })
        .unwrap_or_else(|_| js_undefined());
        if !is_undefined(code) {
            properties.code = cloned_diagnostic(code);
        }
    }
    Ok(WorkerErrorMessage {
        value: SerializedValue::Error {
            name: family.to_vec(),
            message,
            stack,
            cause,
        },
        properties,
    })
}
