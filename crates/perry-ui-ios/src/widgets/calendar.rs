//! iOS Calendar widget — `UIDatePicker` in inline-graphical mode
//! (issue #481 / iOS parity work).
//!
//! `UICalendarView` (iOS 16+) is the natural primitive but limits the
//! deployment target. `UIDatePicker.preferredDatePickerStyle = .inline`
//! gives a calendar grid on iOS 14+ and matches the macOS impl's
//! NSDatePicker graphical-style output.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    static CALENDAR_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

pub struct PerryCalendarTargetIvars {
    pub handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryCalendarTarget"]
    #[ivars = PerryCalendarTargetIvars]
    pub struct PerryCalendarTarget;

    impl PerryCalendarTarget {
        #[unsafe(method(dateChanged:))]
        fn date_changed(&self, sender: &AnyObject) {
            let addr = self as *const Self as usize;
            let handle = self.ivars().handle.get();
            let cb = CALENDAR_CALLBACKS.with(|m| m.borrow().get(&addr).copied());
            let Some(callback) = cb else { return };
            let _ = handle;
            unsafe {
                let date_obj: *mut AnyObject = msg_send![sender, date];
                if date_obj.is_null() {
                    return;
                }
                let iso = format_iso_date(date_obj);
                let bytes = iso.as_bytes();
                let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                let arg = js_nanbox_string(header as i64);
                let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
                js_closure_call1(closure_ptr, arg);
            }
        }
    }
);

impl PerryCalendarTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryCalendarTargetIvars {
            handle: Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

unsafe fn format_iso_date(date_obj: *mut AnyObject) -> String {
    let fmt_cls = AnyClass::get(c"NSDateFormatter").unwrap();
    let alloc: *mut AnyObject = msg_send![fmt_cls, alloc];
    let fmt: Retained<AnyObject> = Retained::from_raw(msg_send![alloc, init]).unwrap();
    let fmt_str = NSString::from_str("yyyy-MM-dd");
    let _: () = msg_send![&*fmt, setDateFormat: &*fmt_str];
    let locale_cls = AnyClass::get(c"NSLocale").unwrap();
    let posix_id = NSString::from_str("en_US_POSIX");
    let alloc_l: *mut AnyObject = msg_send![locale_cls, alloc];
    let locale: Retained<AnyObject> =
        Retained::from_raw(msg_send![alloc_l, initWithLocaleIdentifier: &*posix_id]).unwrap();
    let _: () = msg_send![&*fmt, setLocale: &*locale];
    let str_obj: Retained<NSString> = msg_send![&*fmt, stringFromDate: date_obj];
    str_obj.to_string()
}

unsafe fn make_date(year: i64, month: i64, day: i64) -> *mut AnyObject {
    let comp_cls = AnyClass::get(c"NSDateComponents").unwrap();
    let alloc: *mut AnyObject = msg_send![comp_cls, alloc];
    let comps: *mut AnyObject = msg_send![alloc, init];
    let _: () = msg_send![comps, setYear: year];
    let _: () = msg_send![comps, setMonth: month];
    let _: () = msg_send![comps, setDay: day];
    let cal_cls = AnyClass::get(c"NSCalendar").unwrap();
    let cal: *mut AnyObject = msg_send![cal_cls, currentCalendar];
    msg_send![cal, dateFromComponents: comps]
}

pub fn create(year: i64, month: i64, on_change: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let cls = AnyClass::get(c"UIDatePicker").unwrap();
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let raw: *mut AnyObject = msg_send![alloc, init];
        let picker: Retained<AnyObject> = Retained::from_raw(raw).expect("UIDatePicker init nil");

        // UIDatePickerMode.date = 1
        let _: () = msg_send![&*picker, setDatePickerMode: 1i64];
        // UIDatePickerStyle.inline = 3 (iOS 14+ graphical calendar grid).
        let _: () = msg_send![&*picker, setPreferredDatePickerStyle: 3i64];

        let initial_year = if year > 0 { year } else { 2026 };
        let initial_month = if (1..=12).contains(&month) { month } else { 1 };
        let initial = make_date(initial_year, initial_month, 1);
        if !initial.is_null() {
            let _: () = msg_send![&*picker, setDate: initial];
        }

        let view: Retained<UIView> = Retained::cast_unchecked(picker);
        let handle = super::register_widget(view);

        let target = PerryCalendarTarget::new();
        target.ivars().handle.set(handle);
        let target_addr = Retained::as_ptr(&target) as usize;
        CALENDAR_CALLBACKS.with(|m| {
            m.borrow_mut().insert(target_addr, on_change);
        });

        let picker_view = super::get_widget(handle).unwrap();
        let sel = Sel::register(c"dateChanged:");
        // UIControlEventValueChanged = 1 << 12 = 4096
        let _: () =
            msg_send![&*picker_view, addTarget: &*target, action: sel, forControlEvents: 4096u64];
        std::mem::forget(target);
        handle
    }
}

pub fn set_date(handle: i64, year: i64, month: i64, day: i64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    unsafe {
        let date = make_date(year, month, day);
        if !date.is_null() {
            let _: () = msg_send![&*view, setDate: date];
        }
    }
}

pub fn get_selected_date(handle: i64) -> f64 {
    let Some(view) = super::get_widget(handle) else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    unsafe {
        let date_obj: *mut AnyObject = msg_send![&*view, date];
        if date_obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let iso = format_iso_date(date_obj);
        let bytes = iso.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}
