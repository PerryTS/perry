//! GTK4 DatePicker widget — wraps `gtk4::Calendar` (issue #4772).
//!
//! GTK has no native compact date-field control (no `SysDateTimePick32` /
//! `UIDatePicker.compact` equivalent), so the `DatePicker` widget reuses
//! `gtk4::Calendar` — the same primitive backing the `Calendar` widget.
//! Behavior is otherwise identical to the macOS / iOS / Windows / Android
//! twins: `connect_day_selected` fires when the user picks a date and we
//! format the selection as `yyyy-MM-dd` (POSIX-locale ISO).

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    static DATE_PICKERS: RefCell<HashMap<i64, gtk4::Calendar>> = RefCell::new(HashMap::new());
}

fn iso_date(cal: &gtk4::Calendar) -> String {
    let dt: glib::DateTime = cal.date();
    format!(
        "{:04}-{:02}-{:02}",
        dt.year(),
        dt.month(),
        dt.day_of_month()
    )
}

pub fn create(year: i64, month: i64, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let cal = gtk4::Calendar::new();

    // gtk4::Calendar.set_year/set_month exist as setters.
    if year > 0 {
        cal.set_year(year as i32);
    }
    if (1..=12).contains(&month) {
        // GTK months are 0-based.
        cal.set_month((month - 1) as i32);
    }

    if on_change != 0.0 {
        let on = on_change;
        cal.connect_day_selected(move |c| {
            let iso = iso_date(c);
            let bytes = iso.as_bytes();
            unsafe {
                let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                let arg = js_nanbox_string(header as i64);
                let closure_ptr = js_nanbox_get_pointer(on) as *const u8;
                js_closure_call1(closure_ptr, arg);
            }
        });
    }

    let handle = super::register_widget(cal.clone().upcast());
    DATE_PICKERS.with(|m| m.borrow_mut().insert(handle, cal));
    handle
}

pub fn set_date(handle: i64, year: i64, month: i64, day: i64) {
    let cal = DATE_PICKERS.with(|m| m.borrow().get(&handle).cloned());
    let Some(cal) = cal else { return };
    let target = match glib::DateTime::from_local(
        year as i32,
        month.clamp(1, 12) as i32,
        day.clamp(1, 31) as i32,
        0,
        0,
        0.0,
    ) {
        Ok(dt) => dt,
        Err(_) => return,
    };
    cal.select_day(&target);
}

pub fn get_selected_date(handle: i64) -> f64 {
    let cal = DATE_PICKERS.with(|m| m.borrow().get(&handle).cloned());
    let Some(cal) = cal else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    let iso = iso_date(&cal);
    let bytes = iso.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}
