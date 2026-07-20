use crate::ffi::js_get_string_pointer_unified;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

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

pub(crate) struct TextFieldBinding {
    pub(crate) textfield_handle: i64,
    /// True while `state_set` is mid-flight writing this textfield, OR while the
    /// textfield's own change observer is invoking user code that may call
    /// `state.set`. Either way, the state→textfield write inside
    /// `state_set` is skipped so the field's cursor / selection doesn't
    /// reset on every keystroke. See #1198.
    pub(crate) suppress: Cell<bool>,
}

/// A part of a multi-state text template.
enum TextPart {
    Literal(String),
    StateRef(i64), // state handle
}

struct MultiTextBinding {
    text_handle: i64,
    parts: Vec<TextPart>,
}

struct VisibilityBinding {
    show_handle: i64,
    hide_handle: i64, // 0 = no widget to hide
}

struct ForEachBinding {
    container_handle: i64,
    render_closure: f64, // NaN-boxed closure ptr
}

thread_local! {
    static STATES: RefCell<Vec<StateEntry>> = const { RefCell::new(Vec::new()) };
    /// Map from state_handle -> list of text bindings to update when state changes
    static TEXT_BINDINGS: RefCell<HashMap<i64, Vec<TextBinding>>> = RefCell::new(HashMap::new());
    /// Map from state_handle -> slider bindings (two-way)
    static SLIDER_BINDINGS: RefCell<HashMap<i64, Vec<SliderBinding>>> = RefCell::new(HashMap::new());
    /// Map from state_handle -> toggle bindings (two-way)
    static TOGGLE_BINDINGS: RefCell<HashMap<i64, Vec<ToggleBinding>>> = RefCell::new(HashMap::new());
    /// Map from state_handle -> textfield bindings (two-way)
    static TEXTFIELD_BINDINGS: RefCell<HashMap<i64, Vec<Rc<TextFieldBinding>>>> = RefCell::new(HashMap::new());
    /// Map from textfield_handle -> binding, used by the textfield change
    /// observer in `widgets/textfield.rs` to suppress the echo-back during
    /// user-initiated edits (#1198).
    pub(crate) static TEXTFIELD_TO_BINDING: RefCell<HashMap<i64, Rc<TextFieldBinding>>> = RefCell::new(HashMap::new());
    /// All multi-state text bindings
    static MULTI_TEXT_BINDINGS: RefCell<Vec<MultiTextBinding>> = const { RefCell::new(Vec::new()) };
    /// Map from state_handle -> indices into MULTI_TEXT_BINDINGS
    static MULTI_TEXT_INDEX: RefCell<HashMap<i64, Vec<usize>>> = RefCell::new(HashMap::new());
    /// Map from state_handle -> visibility bindings
    static VISIBILITY_BINDINGS: RefCell<HashMap<i64, Vec<VisibilityBinding>>> = RefCell::new(HashMap::new());
    /// Map from state_handle -> forEach bindings
    static FOR_EACH_BINDINGS: RefCell<HashMap<i64, Vec<ForEachBinding>>> = RefCell::new(HashMap::new());
    /// Map from state_handle -> onChange callbacks
    static ON_CHANGE_CALLBACKS: RefCell<HashMap<i64, Vec<f64>>> = RefCell::new(HashMap::new());
}

/// Extract a &str from a *const StringHeader pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Check if a f64 value is a NaN-boxed string. Accepts heap
/// `STRING_TAG` (0x7FFF) and inline SSO `SHORT_STRING_TAG` (0x7FF9).
fn is_nanboxed_string(value: f64) -> bool {
    let tag = value.to_bits() >> 48;
    tag == 0x7FFF || tag == 0x7FF9
}

/// Extract the string content from a NaN-boxed string value. Routes
/// through `js_get_string_pointer_unified` so SSO inline strings get
/// materialized to a heap StringHeader instead of being misread as a
/// pointer (the SSO payload bytes look like a bogus address to
/// `js_nanbox_get_pointer`).
fn extract_nanboxed_string(value: f64) -> String {
    let ptr = unsafe { js_get_string_pointer_unified(value) as *const u8 };
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

/// Create a new state cell with an initial value. Returns state handle (1-based).
pub fn state_create(initial: f64) -> i64 {
    STATES.with(|s| {
        let mut states = s.borrow_mut();
        states.push(StateEntry { value: initial });
        states.len() as i64 // 1-based handle
    })
}

/// Get the current value of a state cell.
pub fn state_get(handle: i64) -> f64 {
    STATES.with(|s| {
        let states = s.borrow();
        let idx = (handle - 1) as usize;
        if idx < states.len() {
            states[idx].value
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001) // undefined
        }
    })
}

/// Set a new value on a state cell and update all bound widgets.
pub fn state_set(handle: i64, value: f64) {
    STATES.with(|s| {
        let mut states = s.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < states.len() {
            states[idx].value = value;
        }
    });

    let formatted = format_value(value);

    // Update bound text widgets (single-state)
    TEXT_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                let text = format!("{}{}{}", binding.prefix, formatted, binding.suffix);
                widgets::text::set_text_str(binding.text_handle, &text);
            }
        }
    });

    // Update multi-state text bindings
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

    // Update slider bindings (two-way)
    SLIDER_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                widgets::slider::set_value(binding.slider_handle, value);
            }
        }
    });

    // Update toggle bindings (two-way)
    TOGGLE_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                let on = if value != 0.0 && !value.is_nan() {
                    1
                } else {
                    0
                };
                widgets::toggle::set_state(binding.toggle_handle, on);
            }
        }
    });

    // Update textfield bindings (two-way). Skip bindings whose
    // suppress flag is set — that means the update originated from
    // the textfield itself (user typing) and writing back would
    // reset the cursor / selection. See #1198.
    TEXTFIELD_BINDINGS.with(|b| {
        if let Some(bindings) = b.borrow().get(&handle) {
            for binding in bindings {
                if binding.suppress.get() {
                    continue;
                }
                binding.suppress.set(true);
                widgets::textfield::set_text_str(binding.textfield_handle, &formatted);
                binding.suppress.set(false);
            }
        }
    });

    // Update visibility bindings (conditional rendering)
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
            .map(|bindings| {
                bindings
                    .iter()
                    .map(|b| (b.container_handle, b.render_closure))
                    .collect()
            })
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
    let callbacks_snapshot: Vec<f64> =
        ON_CHANGE_CALLBACKS.with(|cbs| cbs.borrow().get(&handle).cloned().unwrap_or_default());
    for closure_f64 in callbacks_snapshot {
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, value);
        }
    }
}

/// Check if a f64 value is truthy in JavaScript sense.
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

/// Rebuild multi-state text from template parts by reading current state values.
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

/// Render ForEach children by calling the closure for each index.
fn render_for_each(container: i64, closure: f64, count: f64) {
    let n = count as i64;
    let closure_ptr = unsafe { js_nanbox_get_pointer(closure) } as *const u8;
    for i in 0..n {
        let child_f64 = unsafe { js_closure_call1(closure_ptr, i as f64) };
        let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
        widgets::add_child(container, child_handle);
    }
}

/// Bind a text widget to a state cell with prefix and suffix strings.
pub fn bind_text_numeric(
    state_handle: i64,
    text_handle: i64,
    prefix_ptr: *const u8,
    suffix_ptr: *const u8,
) {
    let prefix = str_from_header(prefix_ptr).to_string();
    let suffix = str_from_header(suffix_ptr).to_string();
    TEXT_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(TextBinding {
                text_handle,
                prefix,
                suffix,
            });
    });
}

/// Bind a slider widget to a state cell (two-way binding).
pub fn bind_slider(state_handle: i64, slider_handle: i64) {
    SLIDER_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(SliderBinding { slider_handle });
    });
}

/// Bind a toggle widget to a state cell (two-way binding).
pub fn bind_toggle(state_handle: i64, toggle_handle: i64) {
    TOGGLE_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(ToggleBinding { toggle_handle });
    });
}

/// Bind a textfield to a state cell (two-way binding).
///
/// The state→textfield direction lives here: `state_set` walks
/// `TEXTFIELD_BINDINGS` and rewrites the field. The textfield→state
/// direction stays user-wired through the closure passed to
/// `TextField(placeholder, onChange)` (typically
/// `(v) => state.set(v)`). The piece this binding adds for two-way
/// safety is the suppress flag: while the textfield's change observer
/// is invoking that user closure, the state_set echo-back is
/// skipped so the field's cursor / selection isn't reset. See #1198.
pub fn bind_textfield(state_handle: i64, textfield_handle: i64) {
    let binding = Rc::new(TextFieldBinding {
        textfield_handle,
        suppress: Cell::new(false),
    });
    TEXTFIELD_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(binding.clone());
    });
    TEXTFIELD_TO_BINDING.with(|m| {
        m.borrow_mut().insert(textfield_handle, binding);
    });

    // Seed the textfield with the current state value (matches the
    // GTK4 / Windows behavior so a non-empty initial state shows up
    // in the field on first paint).
    let value = state_get(state_handle);
    let text = format_value(value);
    widgets::textfield::set_text_str(textfield_handle, &text);
}

/// Bind a text widget to multiple states with a template.
pub fn bind_text_template(
    text_handle: i64,
    num_parts: i32,
    types_ptr: *const i32,
    values_ptr: *const i64,
) {
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

/// Bind visibility of widgets to a state cell (conditional rendering).
pub fn bind_visibility(state_handle: i64, show_handle: i64, hide_handle: i64) {
    VISIBILITY_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(VisibilityBinding {
                show_handle,
                hide_handle,
            });
    });
    // Set initial visibility
    let value = state_get(state_handle);
    let truthy = is_truthy_f64(value);
    widgets::set_hidden(show_handle, !truthy);
    if hide_handle != 0 {
        widgets::set_hidden(hide_handle, truthy);
    }
}

/// Initialize a ForEach binding: create initial children and register for updates.
pub fn for_each_init(container_handle: i64, state_handle: i64, render_closure: f64) {
    let count = state_get(state_handle);
    render_for_each(container_handle, render_closure, count);

    FOR_EACH_BINDINGS.with(|b| {
        b.borrow_mut()
            .entry(state_handle)
            .or_default()
            .push(ForEachBinding {
                container_handle,
                render_closure,
            });
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
