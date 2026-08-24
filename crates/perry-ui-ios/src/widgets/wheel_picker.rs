//! Native iOS wheel picker backed by `UIPickerView` (issue #5873).

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    static ITEMS: RefCell<HashMap<i64, Vec<String>>> = RefCell::new(HashMap::new());
    static SELECTED: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
    static CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    // UIPickerView keeps its data source and delegate weakly.
    static DELEGATES: RefCell<HashMap<i64, Retained<PerryWheelPickerDelegate>>> = RefCell::new(HashMap::new());
}

pub(crate) fn scan_ios_wheel_picker_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    CALLBACKS.with(|callbacks| {
        for callback in callbacks.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(callback);
        }
    });
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

struct WheelPickerDispatch {
    closure: f64,
    index: f64,
}

unsafe extern "C" fn callback_trampoline(context: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let payload = Box::from_raw(context as *mut WheelPickerDispatch);
        let closure = js_nanbox_get_pointer(payload.closure) as *const u8;
        js_closure_call1(closure, payload.index);
    });
}

fn queue_callback(handle: i64, index: i64) {
    let callback = CALLBACKS.with(|m| m.borrow().get(&handle).copied());
    let Some(closure) = callback.filter(|value| *value != 0.0) else {
        return;
    };
    let payload = Box::new(WheelPickerDispatch {
        closure,
        index: index as f64,
    });
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const _ as *const std::ffi::c_void,
            Box::into_raw(payload) as *mut std::ffi::c_void,
            callback_trampoline,
        );
    }
}

pub struct PerryWheelPickerDelegateIvars {
    handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryWheelPickerDelegateIOS"]
    #[ivars = PerryWheelPickerDelegateIvars]
    pub struct PerryWheelPickerDelegate;

    impl PerryWheelPickerDelegate {
        #[unsafe(method(numberOfComponentsInPickerView:))]
        fn number_of_components(&self, _picker: &AnyObject) -> i64 {
            1
        }

        #[unsafe(method(pickerView:numberOfRowsInComponent:))]
        fn number_of_rows(&self, _picker: &AnyObject, _component: i64) -> i64 {
            let handle = self.ivars().handle.get();
            ITEMS.with(|m| m.borrow().get(&handle).map_or(0, |items| items.len() as i64))
        }

        #[unsafe(method(pickerView:titleForRow:forComponent:))]
        fn title_for_row(
            &self,
            _picker: &AnyObject,
            row: i64,
            _component: i64,
        ) -> *mut AnyObject {
            let handle = self.ivars().handle.get();
            let title = ITEMS.with(|m| {
                m.borrow()
                    .get(&handle)
                    .and_then(|items| items.get(row as usize).cloned())
            });
            title.map_or(std::ptr::null_mut(), |title| {
                Retained::into_raw(NSString::from_str(&title)) as *mut AnyObject
            })
        }

        #[unsafe(method(pickerView:didSelectRow:inComponent:))]
        fn did_select_row(&self, _picker: &AnyObject, row: i64, _component: i64) {
            let handle = self.ivars().handle.get();
            SELECTED.with(|m| m.borrow_mut().insert(handle, row));
            queue_callback(handle, row);
        }
    }
);

impl PerryWheelPickerDelegate {
    fn new(handle: i64) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryWheelPickerDelegateIvars {
            handle: Cell::new(handle),
        });
        unsafe { msg_send![super(this), init] }
    }
}

use perry_ffi::copy_string_from_raw as str_from_header;

pub fn create(on_change: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let picker: Retained<AnyObject> = msg_send![AnyClass::get(c"UIPickerView").unwrap(), new];
        let view: Retained<UIView> = Retained::cast_unchecked(picker);
        let handle = super::register_widget(view.clone());

        ITEMS.with(|m| m.borrow_mut().insert(handle, Vec::new()));
        SELECTED.with(|m| m.borrow_mut().insert(handle, -1));
        CALLBACKS.with(|m| m.borrow_mut().insert(handle, on_change));

        let delegate = PerryWheelPickerDelegate::new(handle);
        let _: () = msg_send![&*view, setDataSource: &*delegate];
        let _: () = msg_send![&*view, setDelegate: &*delegate];
        DELEGATES.with(|m| m.borrow_mut().insert(handle, delegate));
        handle
    }
}

pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = unsafe { str_from_header(title_ptr) }.to_string();
    let first = ITEMS.with(|m| {
        let mut all = m.borrow_mut();
        let Some(items) = all.get_mut(&handle) else {
            return false;
        };
        items.push(title);
        items.len() == 1
    });
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, reloadAllComponents];
            if first {
                let _: () = msg_send![&*view, selectRow: 0i64, inComponent: 0i64, animated: false];
            }
        }
    }
    if first {
        SELECTED.with(|m| m.borrow_mut().insert(handle, 0));
    }
}

pub fn set_selected(handle: i64, index: i64) {
    let valid = ITEMS.with(|m| {
        m.borrow()
            .get(&handle)
            .is_some_and(|items| index >= 0 && (index as usize) < items.len())
    });
    if !valid {
        return;
    }
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, selectRow: index, inComponent: 0i64, animated: false];
        }
        SELECTED.with(|m| m.borrow_mut().insert(handle, index));
    }
}

pub fn get_selected(handle: i64) -> i64 {
    SELECTED.with(|m| m.borrow().get(&handle).copied().unwrap_or(-1))
}
