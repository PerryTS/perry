//! macOS DatePicker widget (issue #4772).
//!
//! The compact, space-saving complement to the graphical month-grid
//! `Calendar` widget. Wraps `NSDatePicker` with
//! `NSDatePickerStyleTextFieldAndStepper` (a date field with up/down
//! steppers), elements limited to year/month/day so the clock face is
//! hidden. The user gets a compact date field with a selection callback.
//!
//! `onChange` fires with the selected date as an ISO `yyyy-MM-dd` string
//! (POSIX-locale formatter, stable across user locales) — matching the
//! `Calendar` twin.

use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::NSObject;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    static DATEPICKER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

pub struct PerryDatePickerTargetIvars {
    pub handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryDatePickerTarget"]
    #[ivars = PerryDatePickerTargetIvars]
    pub struct PerryDatePickerTarget;

    impl PerryDatePickerTarget {
        #[unsafe(method(dateChanged:))]
        fn date_changed(&self, _sender: &AnyObject) {
            let handle = self.ivars().handle.get();
            let addr = self as *const Self as usize;
            crate::catch_callback_panic("date picker callback", std::panic::AssertUnwindSafe(|| {
                let cb = DATEPICKER_CALLBACKS.with(|m| m.borrow().get(&addr).copied());
                let Some(callback) = cb else { return };
                let Some(view) = super::get_widget(handle) else { return };
                unsafe {
                    let date_obj: *mut AnyObject = msg_send![&*view, dateValue];
                    if date_obj.is_null() {
                        return;
                    }
                    let iso = format_iso_date(date_obj);
                    let bytes = iso.as_bytes();
                    let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                    let arg = js_nanbox_string(header as i64);
                    let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
                    js_closure_call1(closure_ptr, arg);
                }
            }));
        }
    }
);

impl PerryDatePickerTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryDatePickerTargetIvars {
            handle: Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

unsafe fn format_iso_date(date_obj: *mut AnyObject) -> String {
    // Use NSDateFormatter with "yyyy-MM-dd" — fixed-locale POSIX so
    // the JS side gets a parsable string regardless of user locale.
    let fmt_cls = AnyClass::get(c"NSDateFormatter").unwrap();
    let alloc: *mut AnyObject = msg_send![fmt_cls, alloc];
    let fmt: Retained<AnyObject> = Retained::from_raw(msg_send![alloc, init]).unwrap();
    let fmt_str = objc2_foundation::NSString::from_str("yyyy-MM-dd");
    let _: () = msg_send![&*fmt, setDateFormat: &*fmt_str];
    // Force locale to POSIX so the format is stable.
    let locale_cls = AnyClass::get(c"NSLocale").unwrap();
    let posix_id = objc2_foundation::NSString::from_str("en_US_POSIX");
    let alloc_l: *mut AnyObject = msg_send![locale_cls, alloc];
    let locale: Retained<AnyObject> =
        Retained::from_raw(msg_send![alloc_l, initWithLocaleIdentifier: &*posix_id]).unwrap();
    let _: () = msg_send![&*fmt, setLocale: &*locale];
    let str_obj: Retained<objc2_foundation::NSString> = msg_send![&*fmt, stringFromDate: date_obj];
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

/// Create an `NSDatePicker` configured as a compact text-field-and-stepper
/// date field. Elements are limited to year-month-day (no clock face).
pub fn create(year: i64, month: i64, on_change: f64) -> i64 {
    unsafe {
        let cls = AnyClass::get(c"NSDatePicker").unwrap();
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(140.0, 24.0),
        );
        let raw: *mut AnyObject = msg_send![alloc, initWithFrame: frame];
        let picker: Retained<AnyObject> = Retained::from_raw(raw).unwrap();

        // NSDatePickerStyleTextFieldAndStepper = 0 (compact field).
        let _: () = msg_send![&*picker, setDatePickerStyle: 0u64];
        // Limit to year/month/day — drop the clock face.
        // NSDatePickerElementFlagYearMonthDay = 0xC0
        let _: () = msg_send![&*picker, setDatePickerElements: 0xC0u64];

        // Initial date.
        let initial_year = if year > 0 { year } else { 2026 };
        let initial_month = if (1..=12).contains(&month) { month } else { 1 };
        let initial_date = make_date(initial_year, initial_month, 1);
        if !initial_date.is_null() {
            let _: () = msg_send![&*picker, setDateValue: initial_date];
        }

        let view: Retained<NSView> = Retained::cast_unchecked(picker);
        let handle = super::register_widget(view);

        let target = PerryDatePickerTarget::new();
        target.ivars().handle.set(handle);
        let target_addr = Retained::as_ptr(&target) as usize;
        DATEPICKER_CALLBACKS.with(|m| {
            m.borrow_mut().insert(target_addr, on_change);
        });

        let picker_view = super::get_widget(handle).unwrap();
        let sel = Sel::register(c"dateChanged:");
        let _: () = msg_send![&*picker_view, setTarget: &*target];
        let _: () = msg_send![&*picker_view, setAction: sel];

        std::mem::forget(target);
        handle
    }
}

/// Programmatically set the selected date (1-based month + day).
pub fn set_date(handle: i64, year: i64, month: i64, day: i64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    unsafe {
        let date = make_date(year, month, day);
        if !date.is_null() {
            let _: () = msg_send![&*view, setDateValue: date];
        }
    }
}

/// Get the selected date as a NaN-boxed STRING in `yyyy-MM-dd` form.
pub fn get_selected_date(handle: i64) -> f64 {
    let Some(view) = super::get_widget(handle) else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    unsafe {
        let date_obj: *mut AnyObject = msg_send![&*view, dateValue];
        if date_obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let iso = format_iso_date(date_obj);
        let bytes = iso.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(header as i64)
    }
}
