use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_ui_kit::UIView;
use objc2_foundation::{NSString, MainThreadMarker};

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

pub fn form_create() -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let stack_cls = objc2::runtime::AnyClass::get(c"UIStackView").unwrap();
        let obj: *mut AnyObject = msg_send![stack_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, init];
        let stack: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let _: () = msg_send![&*stack, setAxis: 1i64]; // vertical
        let _: () = msg_send![&*stack, setSpacing: 16.0f64];
        super::register_widget(stack)
    }
}

pub fn section_create(title_ptr: *const u8) -> i64 {
    let title = str_from_header(title_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let stack_cls = objc2::runtime::AnyClass::get(c"UIStackView").unwrap();
        let obj: *mut AnyObject = msg_send![stack_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, init];
        let stack: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let _: () = msg_send![&*stack, setAxis: 1i64]; // vertical
        let _: () = msg_send![&*stack, setSpacing: 8.0f64];

        // Add title label if non-empty
        if !title.is_empty() {
            let label_cls = objc2::runtime::AnyClass::get(c"UILabel").unwrap();
            let lbl: *mut AnyObject = msg_send![label_cls, new];
            let label: Retained<UIView> = Retained::retain(lbl as *mut UIView).unwrap();
            let ns_title = NSString::from_str(title);
            let _: () = msg_send![&*label, setText: &*ns_title];
            let _: () = msg_send![&*stack, addArrangedSubview: &*label];
        }

        super::register_widget(stack)
    }
}
