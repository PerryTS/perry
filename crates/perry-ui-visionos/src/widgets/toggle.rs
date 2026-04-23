use objc2::rc::Retained;
use objc2::msg_send;
use objc2_ui_kit::{UILabel, UIView};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TOGGLE_LABELS: RefCell<HashMap<i64, String>> = RefCell::new(HashMap::new());
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

/// Set the on/off state of an existing toggle widget.
pub fn set_state(handle: i64, on: i64) {
    if let Some(view) = super::get_widget(handle) {
        TOGGLE_LABELS.with(|labels| {
            if let Some(label) = labels.borrow().get(&handle) {
                let text = NSString::from_str(&format!("{}: {}", label, if on != 0 { "On" } else { "Off" }));
                unsafe {
                    let _: () = msg_send![&*view, setText: &*text];
                }
            }
        });
    }
}

/// Create a visionOS-safe toggle fallback.
///
/// visionOS is rejecting the UIKit toggle control path with an Objective-C
/// exception during construction in the simulator. For now render a static
/// label so apps stay alive while the rest of the UI backend is exercised.
pub fn create(label_ptr: *const u8, on_change: f64) -> i64 {
    // TODO(visionos): replace UILabel stub with UISwitch once sim-side
    // construction stops throwing Objective-C exceptions. See PR #127.
    let label = str_from_header(label_ptr);
    let _ = on_change;

    unsafe {
        let label_cls = match objc2::runtime::AnyClass::get(c"UILabel") {
            Some(cls) => cls,
            None => return 0,
        };
        let ns_label = NSString::from_str(&format!("{}: Off", label));
        let text_label: Retained<UILabel> = msg_send![label_cls, new];
        let _: () = msg_send![&*text_label, setText: &*ns_label];
        let view: Retained<UIView> = Retained::cast_unchecked(text_label);
        let handle = super::register_widget(view);
        TOGGLE_LABELS.with(|labels| {
            labels.borrow_mut().insert(handle, label.to_string());
        });

        #[cfg(feature = "geisterhand")]
        {
            extern "C" { fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8); }
            unsafe { perry_geisterhand_register(handle, 3, 1, on_change, label_ptr); }
        }

        handle
    }
}
