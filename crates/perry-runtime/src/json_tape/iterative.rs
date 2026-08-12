//! Heap-stack materialization for JSON documents too deep for recursive paths.

use super::*;

enum BuildFrame {
    Array(Vec<JSValue>),
    Object {
        keys: Vec<*mut crate::StringHeader>,
        values: Vec<JSValue>,
    },
}

impl BuildFrame {
    fn push_value(&mut self, value: JSValue) -> bool {
        match self {
            Self::Array(values) => {
                values.push(value);
                true
            }
            Self::Object { keys, values } if values.len() < keys.len() => {
                values.push(value);
                true
            }
            Self::Object { .. } => false,
        }
    }
}

unsafe fn finish_frame(frame: BuildFrame) -> Option<JSValue> {
    match frame {
        BuildFrame::Array(values) => {
            let capacity = u32::try_from(values.len()).ok()?;
            let mut array = crate::array::js_array_alloc(capacity);
            for value in values {
                array = crate::array::js_array_push(array, value);
            }
            Some(JSValue::object_ptr(array as *mut u8))
        }
        BuildFrame::Object { keys, values } => {
            if keys.len() != values.len() {
                return None;
            }
            let object = crate::object::js_object_alloc(0, 0);
            for (key, value) in keys.into_iter().zip(values) {
                crate::object::js_object_set_field_by_name(
                    object,
                    key,
                    f64::from_bits(value.bits()),
                );
            }
            Some(JSValue::object_ptr(object as *mut u8))
        }
    }
}

fn attach_value(frames: &mut [BuildFrame], root: &mut Option<JSValue>, value: JSValue) -> bool {
    if let Some(parent) = frames.last_mut() {
        parent.push_value(value)
    } else if root.is_none() {
        *root = Some(value);
        true
    } else {
        false
    }
}

/// Materialize a validated tape without consuming one native stack frame per
/// JSON container. Runtime GC must be suppressed by the caller: partially
/// built values live in this function's heap-backed work stack until their
/// parent container is complete.
pub(crate) unsafe fn materialize_iterative(tape: &[TapeEntry], bytes: &[u8]) -> Option<JSValue> {
    let source = TapeSource::Borrowed { tape, bytes };
    let mut frames = Vec::new();
    let mut root = None;

    for entry in tape.iter().copied() {
        match entry.kind {
            KIND_OBJ_START => frames.push(BuildFrame::Object {
                keys: Vec::new(),
                values: Vec::new(),
            }),
            KIND_ARR_START => frames.push(BuildFrame::Array(Vec::new())),
            KIND_KEY => {
                let Some(BuildFrame::Object { keys, values }) = frames.last_mut() else {
                    return None;
                };
                if keys.len() != values.len() {
                    return None;
                }
                let key = decode_key_to_interned_string(&source, entry.offset as usize);
                if key.is_null() {
                    return None;
                }
                keys.push(key);
            }
            KIND_STRING => {
                let value = materialize_string_value(&source, entry.offset as usize);
                if !attach_value(&mut frames, &mut root, value) {
                    return None;
                }
            }
            KIND_NUMBER => {
                let value = materialize_number(&source, entry.offset as usize);
                if !attach_value(&mut frames, &mut root, value) {
                    return None;
                }
            }
            KIND_TRUE | KIND_FALSE | KIND_NULL => {
                let value = match entry.kind {
                    KIND_TRUE => JSValue::bool(true),
                    KIND_FALSE => JSValue::bool(false),
                    _ => JSValue::null(),
                };
                if !attach_value(&mut frames, &mut root, value) {
                    return None;
                }
            }
            KIND_OBJ_END | KIND_ARR_END => {
                let frame = frames.pop()?;
                if !matches!(
                    (&frame, entry.kind),
                    (BuildFrame::Object { .. }, KIND_OBJ_END)
                        | (BuildFrame::Array(_), KIND_ARR_END)
                ) {
                    return None;
                }
                let value = finish_frame(frame)?;
                if !attach_value(&mut frames, &mut root, value) {
                    return None;
                }
            }
            _ => return None,
        }
    }

    if frames.is_empty() {
        root
    } else {
        None
    }
}
