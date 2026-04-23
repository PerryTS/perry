use std::cell::RefCell;
use std::collections::HashMap;

use crate::widgets;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

struct StateEntry {
    value: f64,
}

struct TextBinding {
    text_handle: i64,
    prefix: String,
    suffix: String,
}

struct SliderBinding {
    slider_handle: i64,
}

struct ToggleBinding {
    toggle_handle: i64,
}

struct TextFieldBinding {
    textfield_handle: i64,
}

enum TextPart {
    Literal(String),
    StateRef(i64),
}

struct MultiTextBinding {
    text_handle: i64,
    parts: Vec<TextPart>,
}

struct VisibilityBinding {
    show_handle: i64,
    hide_handle: i64,
}

struct ForEachBinding {
    container_handle: i64,
    render_closure: f64,
}

thread_local! {
    static STATES: RefCell<Vec<StateEntry>> = RefCell::new(Vec::new());
    static TEXT_BINDINGS: RefCell<HashMap<i64, Vec<TextBinding>>> = RefCell::new(HashMap::new());
    static SLIDER_BINDINGS: RefCell<HashMap<i64, Vec<SliderBinding>>> = RefCell::new(HashMap::new());
    static TOGGLE_BINDINGS: RefCell<HashMap<i64, Vec<ToggleBinding>>> = RefCell::new(HashMap::new());
    static TEXTFIELD_BINDINGS: RefCell<HashMap<i64, Vec<TextFieldBinding>>> = RefCell::new(HashMap::new());
    static MULTI_TEXT_BINDINGS: RefCell<Vec<MultiTextBinding>> = RefCell::new(Vec::new());
    static MULTI_TEXT_INDEX: RefCell<HashMap<i64, Vec<usize>>> = RefCell::new(HashMap::new());
    static VISIBILITY_BINDINGS: RefCell<HashMap<i64, Vec<VisibilityBinding>>> = RefCell::new(HashMap::new());
    static FOR_EACH_BINDINGS: RefCell<HashMap<i64, Vec<ForEachBinding>>> = RefCell::new(HashMap::new());
    static ON_CHANGE_CALLBACKS: RefCell<HashMap<i64, Vec<f64>>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Check if a f64 value is a NaN-boxed string (STRING_TAG = 0x7FFF).
fn is_nanboxed_string(value: f64) -> bool {
    let bits = value.to_bits();
    (bits >> 48) == 0x7FFF
}

/// Extract the string content from a NaN-boxed string value.
fn extract_nanboxed_string(value: f64) -> String {
    let ptr = unsafe { js_nanbox_get_pointer(value) } as *const u8;
    str_from_header(ptr).to_string()
}

fn format_value(value: f64) -> String {
    if is_nanboxed_string(value) {
        extract_nanboxed_string(value)
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

pub fn state_create(initial: f64) -> i64 {
    STATES.with(|s| {
        let mut states = s.borrow_mut();
        states.push(StateEntry { value: initial });
        states.len() as i64
    })
}

pub fn state_get(handle: i64) -> f64 {
    STATES.with(|s| {
        let states = s.borrow();
        let idx = (handle - 1) as usize;
        if idx < states.len() {
            states[idx].value
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
    })
}

pub fn state_set(handle: i64, value: f64) {
    STATES.with(|s| {
        let mut states = s.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < states.len() {
            states[idx].value = value;
        }
    });

    let formatted = format_value(value);

    TEXT_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                let text = format!("{}{}{}", binding.prefix, formatted, binding.suffix);
                widgets::text::set_text_str(binding.text_handle, &text);
            }
        }
    });

    MULTI_TEXT_INDEX.with(|idx| {
        if let Some(binding_indices) = idx.borrow().get(&handle) {
            MULTI_TEXT_BINDINGS.with(|bindings| {
                let bindings = bindings.borrow();
                for &bi in binding_indices {
                    if bi < bindings.len() {
                        let binding = &bindings[bi];
                        let text = rebuild_multi_text(&binding.parts);
                        widgets::text::set_text_str(binding.text_handle, &text);
                    }
                }
            });
        }
    });

    SLIDER_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                widgets::slider::set_value(binding.slider_handle, value);
            }
        }
    });

    TOGGLE_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                let on = if value != 0.0 && !value.is_nan() { 1 } else { 0 };
                widgets::toggle::set_state(binding.toggle_handle, on);
            }
        }
    });

    // Update textfield bindings (two-way)
    TEXTFIELD_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                widgets::textfield::set_text_str(binding.textfield_handle, &formatted);
            }
        }
    });

    VISIBILITY_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            let truthy = is_truthy_f64(value);
            for binding in bindings {
                widgets::set_hidden(binding.show_handle, !truthy);
                if binding.hide_handle != 0 {
                    widgets::set_hidden(binding.hide_handle, truthy);
                }
            }
        }
    });

    // Update forEach bindings (dynamic lists).
    // Clone into local Vec to release borrow before calling user closures
    // (render_for_each invokes js_closure_call1 which could re-enter state code).
    let foreach_snapshot: Vec<(i64, f64)> = FOR_EACH_BINDINGS.with(|b| {
        b.borrow()
            .get(&handle)
            .map(|bindings| bindings.iter().map(|b| (b.container_handle, b.render_closure)).collect())
            .unwrap_or_default()
    });
    for (container, closure) in foreach_snapshot {
        widgets::clear_children(container);
        render_for_each(container, closure, value);
    }

    // Invoke onChange callbacks.
    // Clone callbacks into a local Vec before invoking, so the immutable borrow
    // on ON_CHANGE_CALLBACKS is released before user code runs. Without this,
    // a callback that registers new onChange handlers (e.g. perry-react's
    // _scheduleRerender → re-render → new useState → sig.onChange) would try
    // borrow_mut while the immutable borrow is still held → RefCell panic.
    let callbacks_snapshot: Vec<f64> = ON_CHANGE_CALLBACKS.with(|cbs| {
        cbs.borrow()
            .get(&handle)
            .map(|v| v.clone())
            .unwrap_or_default()
    });
    for closure_f64 in callbacks_snapshot {
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe { js_closure_call1(closure_ptr, value); }
    }
}

fn is_truthy_f64(value: f64) -> bool {
    let bits = value.to_bits();
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

    if bits == TAG_FALSE || bits == TAG_UNDEFINED || bits == TAG_NULL {
        return false;
    }
    if value == 0.0 || value.is_nan() {
        return false;
    }
    true
}

fn rebuild_multi_text(parts: &[TextPart]) -> String {
    let mut result = String::new();
    for part in parts {
        match part {
            TextPart::Literal(s) => result.push_str(s),
            TextPart::StateRef(state_handle) => {
                let val = state_get(*state_handle);
                result.push_str(&format_value(val));
            }
        }
    }
    result
}

fn render_for_each(container: i64, closure: f64, count: f64) {
    let n = count as i64;
    let closure_ptr = unsafe { js_nanbox_get_pointer(closure) } as *const u8;
    for i in 0..n {
        let child_f64 = unsafe { js_closure_call1(closure_ptr, i as f64) };
        let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
        widgets::add_child(container, child_handle);
    }
}

pub fn bind_text_numeric(state_handle: i64, text_handle: i64, prefix_ptr: *const u8, suffix_ptr: *const u8) {
    let prefix = str_from_header(prefix_ptr).to_string();
    let suffix = str_from_header(suffix_ptr).to_string();
    TEXT_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(TextBinding { text_handle, prefix, suffix });
    });
}

pub fn bind_slider(state_handle: i64, slider_handle: i64) {
    SLIDER_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(SliderBinding { slider_handle });
    });
}

pub fn bind_toggle(state_handle: i64, toggle_handle: i64) {
    TOGGLE_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(ToggleBinding { toggle_handle });
    });
}

pub fn bind_text_template(text_handle: i64, num_parts: i32, types_ptr: *const i32, values_ptr: *const i64) {
    let mut parts = Vec::new();
    let mut state_handles = Vec::new();

    for i in 0..num_parts as usize {
        let part_type = unsafe { *types_ptr.add(i) };
        let part_value = unsafe { *values_ptr.add(i) };

        if part_type == 0 {
            let s = str_from_header(part_value as *const u8).to_string();
            parts.push(TextPart::Literal(s));
        } else {
            state_handles.push(part_value);
            parts.push(TextPart::StateRef(part_value));
        }
    }

    MULTI_TEXT_BINDINGS.with(|bindings| {
        let mut bindings = bindings.borrow_mut();
        let idx = bindings.len();
        bindings.push(MultiTextBinding { text_handle, parts });

        MULTI_TEXT_INDEX.with(|index| {
            let mut index = index.borrow_mut();
            for &sh in &state_handles {
                index.entry(sh).or_default().push(idx);
            }
        });
    });
}

pub fn bind_visibility(state_handle: i64, show_handle: i64, hide_handle: i64) {
    VISIBILITY_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(VisibilityBinding { show_handle, hide_handle });
    });
    let value = state_get(state_handle);
    let truthy = is_truthy_f64(value);
    widgets::set_hidden(show_handle, !truthy);
    if hide_handle != 0 {
        widgets::set_hidden(hide_handle, truthy);
    }
}

pub fn for_each_init(container_handle: i64, state_handle: i64, render_closure: f64) {
    let count = state_get(state_handle);
    render_for_each(container_handle, render_closure, count);

    FOR_EACH_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(ForEachBinding { container_handle, render_closure });
    });
}

/// Bind a textfield to a state cell (two-way binding).
pub fn bind_textfield(state_handle: i64, textfield_handle: i64) {
    TEXTFIELD_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(TextFieldBinding { textfield_handle });
    });
}

/// Register a callback to be invoked whenever a state cell changes.
pub fn state_on_change(state_handle: i64, callback: f64) {
    ON_CHANGE_CALLBACKS.with(|cbs| {
        cbs.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(callback);
    });
}
