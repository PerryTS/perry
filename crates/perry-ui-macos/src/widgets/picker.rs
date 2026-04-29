use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSPopUpButton, NSView};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static PICKER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

pub struct PerryPickerTargetIvars {
    pub handle: std::cell::Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryPickerTarget"]
    #[ivars = PerryPickerTargetIvars]
    pub struct PerryPickerTarget;

    impl PerryPickerTarget {
        #[unsafe(method(selectionChanged:))]
        fn selection_changed(&self, _sender: &AnyObject) {
            let handle = self.ivars().handle.get();
            let addr = self as *const Self as usize;
            crate::catch_callback_panic("picker callback", std::panic::AssertUnwindSafe(|| {
                PICKER_CALLBACKS.with(|cbs| {
                    if let Some(&callback) = cbs.borrow().get(&addr) {
                        if let Some(view) = super::get_widget(handle) {
                            let index: i64 = unsafe { msg_send![&*view, indexOfSelectedItem] };
                            let closure_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
                            unsafe {
                                js_closure_call1(closure_ptr, index as f64);
                            }
                        }
                    }
                });
            }));
        }
    }
);

impl PerryPickerTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryPickerTargetIvars {
            handle: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
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

/// Create an NSPopUpButton (dropdown picker) with an onChange callback.
/// `_label_ptr` is a StringHeader pointer (unused for NSPopUpButton title).
/// `on_change` is a NaN-boxed closure called with the selected index.
/// `_style` is reserved for future segmented control support.
pub fn create(_label_ptr: *const u8, on_change: f64, _style: i64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let popup: Retained<NSPopUpButton> = msg_send![
            NSPopUpButton::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(200.0, 25.0),
            ), pullsDown: false
        ];

        let view: Retained<NSView> = Retained::cast_unchecked(popup);
        let handle = super::register_widget(view);

        // Set up target-action for selection changes
        let target = PerryPickerTarget::new();
        target.ivars().handle.set(handle);
        let target_addr = Retained::as_ptr(&target) as usize;

        PICKER_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let popup_view = super::get_widget(handle).unwrap();
        let sel = Sel::register(c"selectionChanged:");
        let _: () = msg_send![&*popup_view, setTarget: &*target];
        let _: () = msg_send![&*popup_view, setAction: sel];

        std::mem::forget(target);

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            unsafe {
                perry_geisterhand_register(handle, 4, 1, on_change, std::ptr::null());
            }
        }

        handle
    }
}

/// Add an item to the popup button's menu.
pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_title = NSString::from_str(title);
        unsafe {
            let _: () = msg_send![&*view, addItemWithTitle: &*ns_title];
        }
    }
}

/// Set the selected item by index.
pub fn set_selected(handle: i64, index: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, selectItemAtIndex: index];
        }
    }
}

/// Get the index of the currently selected item.
pub fn get_selected(handle: i64) -> i64 {
    if let Some(view) = super::get_widget(handle) {
        unsafe { msg_send![&*view, indexOfSelectedItem] }
    } else {
        -1
    }
}
