use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_ui_kit::UIView;
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
        let seg_cls = objc2::runtime::AnyClass::get(c"UISegmentedControl").unwrap();
        let obj: *mut AnyObject = msg_send![seg_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, init];
        let view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let handle = super::register_widget(view);
        PICKER_ITEMS.with(|pi| pi.borrow_mut().insert(handle, Vec::new()));
        PICKER_SELECTED.with(|ps| ps.borrow_mut().insert(handle, 0));
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
                let ns_title = NSString::from_str(title);
                unsafe {
                    let _: () = msg_send![&*view, insertSegmentWithTitle: &*ns_title, atIndex: index as u64, animated: false];
                }
            }
        });
    }
}

pub fn set_selected(handle: i64, index: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, setSelectedSegmentIndex: index];
        }
        PICKER_SELECTED.with(|ps| {
            ps.borrow_mut().insert(handle, index);
        });
    }
}

pub fn get_selected(handle: i64) -> i64 {
    PICKER_SELECTED.with(|ps| ps.borrow().get(&handle).copied().unwrap_or(-1))
}
