use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_ui_kit::UIView;
use objc2_foundation::{NSString, MainThreadMarker};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static PICKER_ITEMS: RefCell<HashMap<i64, Vec<String>>> = RefCell::new(HashMap::new());
    static PICKER_SELECTED: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
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

pub fn create(_label_ptr: *const u8, _on_change: f64, _style: i64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        // TODO(visionos): replace UILabel stub with a real picker once the
        // segmented-control-backed path is stable in the simulator. See PR #127.
        // TODO(visionos): wire `style` through once this migrates off the
        // UILabel fallback and back onto a native picker implementation.
        let label = str_from_header(_label_ptr);
        let label_cls = objc2::runtime::AnyClass::get(c"UILabel").unwrap();
        let obj: *mut AnyObject = msg_send![label_cls, new];
        let text = NSString::from_str(label);
        let _: () = msg_send![obj, setText: &*text];
        let view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let handle = super::register_widget(view);
        PICKER_ITEMS.with(|pi| pi.borrow_mut().insert(handle, Vec::new()));
        PICKER_SELECTED.with(|ps| ps.borrow_mut().insert(handle, 0));
        #[cfg(feature = "geisterhand")]
        {
            extern "C" { fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8); }
            perry_geisterhand_register(handle, 4, 1, _on_change, _label_ptr);
        }
        handle
    }
}

pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(view) = super::get_widget(handle) {
        PICKER_ITEMS.with(|pi| {
            let mut items = pi.borrow_mut();
            if let Some(list) = items.get_mut(&handle) {
                let index = list.len();
                list.push(title.to_string());
                if index == 0 {
                    let ns_title = NSString::from_str(title);
                    unsafe {
                        let _: () = msg_send![&*view, setText: &*ns_title];
                    }
                }
            }
        });
    }
}

pub fn set_selected(handle: i64, index: i64) {
    if let Some(view) = super::get_widget(handle) {
        PICKER_SELECTED.with(|ps| {
            ps.borrow_mut().insert(handle, index);
        });
        PICKER_ITEMS.with(|pi| {
            if let Some(items) = pi.borrow().get(&handle) {
                if let Some(item) = items.get(index.max(0) as usize) {
                    let ns_title = NSString::from_str(item);
                    unsafe {
                        let _: () = msg_send![&*view, setText: &*ns_title];
                    }
                }
            }
        });
    }
}

pub fn get_selected(handle: i64) -> i64 {
    PICKER_SELECTED.with(|ps| {
        ps.borrow().get(&handle).copied().unwrap_or(-1)
    })
}
