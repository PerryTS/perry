//! English month / weekday name tables for date formatting. Split out of
//! `date_collator.rs` to keep it under the 2000-line cap. Pure data, no deps.

pub(crate) const MONTH_FULL: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub(crate) const MONTH_ABBR: &[&str] = &[
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub(crate) const WEEKDAY_FULL: &[&str] = &[
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
pub(crate) const WEEKDAY_ABBR: &[&str] = &["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub(crate) const WEEKDAY_NARROW: &[&str] = &["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
