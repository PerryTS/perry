use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static PICKER_ITEMS: RefCell<HashMap<i64, Vec<String>>> = RefCell::new(HashMap::new());
    static PICKER_SELECTED: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
    static PICKER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    // dispatch_get_main_queue() is a macro; the actual symbol is _dispatch_main_q
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

/// Heap payload handed to the main-queue trampoline: the onChange closure and
/// the segment index selected when the value-changed event fired.
struct PickerDispatch {
    closure_f64: f64,
    index: f64,
}

unsafe extern "C" fn picker_callback_trampoline(context: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let payload = Box::from_raw(context as *mut PickerDispatch);
        let closure_ptr = js_nanbox_get_pointer(payload.closure_f64);
        js_closure_call1(closure_ptr as *const u8, payload.index);
    });
}

pub struct PerryPickerTargetIvars {
    callback_key: std::cell::Cell<usize>,
    handle: std::cell::Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryPickerTarget"]
    #[ivars = PerryPickerTargetIvars]
    pub struct PerryPickerTarget;

    impl PerryPickerTarget {
        #[unsafe(method(segmentChanged:))]
        fn segment_changed(&self, sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            let handle = self.ivars().handle.get();
            PICKER_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    let index: i64 = unsafe { msg_send![sender, selectedSegmentIndex] };
                    // Keep the cached selection in sync so get_selected() reflects
                    // a user tap, not just programmatic set_selected().
                    PICKER_SELECTED.with(|ps| {
                        ps.borrow_mut().insert(handle, index);
                    });
                    // Dispatch async to the main queue so the JS runtime isn't
                    // re-entered synchronously inside UIKit's valueChanged
                    // processing (mirrors the button path; avoids iOS-26 crashes).
                    let payload = Box::new(PickerDispatch {
                        closure_f64,
                        index: index as f64,
                    });
                    unsafe {
                        dispatch_async_f(
                            &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                            Box::into_raw(payload) as *mut std::ffi::c_void,
                            picker_callback_trampoline,
                        );
                    }
                }
            });
        }
    }
);

impl PerryPickerTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryPickerTargetIvars {
            callback_key: std::cell::Cell::new(0),
            handle: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
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
        let handle = super::register_widget(view.clone());
        PICKER_ITEMS.with(|pi| pi.borrow_mut().insert(handle, Vec::new()));
        PICKER_SELECTED.with(|ps| ps.borrow_mut().insert(handle, 0));

        // Wire the segmented control's valueChanged event to the onChange
        // callback. Without this the picker is inert for real users — tapping a
        // segment does nothing (#5201).
        let target = PerryPickerTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);
        target.ivars().handle.set(handle);
        PICKER_CALLBACKS.with(|c| c.borrow_mut().insert(target_addr, _on_change));
        let sel = Sel::register(c"segmentChanged:");
        // UIControlEventValueChanged = 1 << 12 = 4096
        let _: () = msg_send![&*view, addTarget: &*target, action: sel, forControlEvents: 4096u64];
        std::mem::forget(target);

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
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
