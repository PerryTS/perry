//! `Temporal.<Type>.prototype` population — real accessor + method properties.
//!
//! A `Temporal.*` value is a NaN-boxed cell with no JS `[[Prototype]]` link, so
//! instance getter reads / method calls dispatch through the runtime's brand
//! arms (see [`crate::temporal::dispatch`]) rather than the prototype chain.
//! Test262 nonetheless *introspects* the prototype directly —
//! `Object.getOwnPropertyDescriptor(Temporal.PlainTime.prototype, "hour").get`,
//! `Temporal.PlainTime.prototype.add.length`, `isConstructor(...)`,
//! `Object.isExtensible(...)`, etc. — so the prototype object must carry the
//! real accessor getters and method functions with spec-correct attributes.
//!
//! Both the getter and method bodies are **generic**: one shared thunk each
//! reads the property/method name back off its own closure (set via
//! [`set_bound_native_closure_name`]) and routes to the per-type dispatch
//! router. Each getter brand-checks `this` (a non-Temporal receiver throws
//! `TypeError`, matching the spec's `RequireInternalSlot`).

use super::*;
use crate::temporal::dispatch as tdispatch;
/// Read a closure's installed `name` dynamic-prop as a Rust string.
fn closure_name(c: *const crate::closure::ClosureHeader) -> String {
    let name_val = crate::closure::closure_get_dynamic_prop(c as usize, "name");
    let jv = crate::value::JSValue::from_bits(name_val.to_bits());
    if !jv.is_string() {
        return String::new();
    }
    let ptr = jv.as_string_ptr();
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// Throw `TypeError: get/call Temporal.<Type>.prototype.<member> called on an
/// incompatible receiver` — the brand-check failure shared by every accessor /
/// method when `this` is not the matching Temporal value.
fn throw_brand(member: &str) -> ! {
    let msg = format!("Temporal.prototype.{member} called on an incompatible receiver");
    crate::object::throw_object_type_error(msg.as_bytes())
}

/// Shared accessor getter for every `Temporal.<Type>.prototype` field. Reads
/// its `this` argument, derives the property name from the closure (`"get hour"` →
/// `"hour"`), and routes to the brand router; a non-Temporal `this` throws.
pub(super) extern "C" fn temporal_proto_getter_thunk(
    c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let recv = f64::from_bits(this.bits());
    let full = closure_name(c);
    let prop = full.strip_prefix("get ").unwrap_or(&full);
    match tdispatch::get_property(recv, prop) {
        Some(v) => v,
        None => throw_brand(prop),
    }
}

/// Shared method thunk for every `Temporal.<Type>.prototype` method. Reads
/// its `this` argument, derives the method name from the closure, brand-checks the
/// receiver, and routes to the brand router with the rest-array args.
pub(super) extern "C" fn temporal_proto_method_thunk(
    c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    let recv = f64::from_bits(this.bits());
    let name = closure_name(c);
    if crate::temporal::temporal_kind(recv).is_none() {
        throw_brand(&name);
    }
    let args = super::global_this::global_this_rest_array_values(rest);
    tdispatch::call_method(recv, &name, &args)
}

/// Install a brand-checking accessor getter (`{ get, set: undefined,
/// enumerable: false, configurable: true }`) onto a Temporal prototype. Mirrors
/// `regex_proto_thunks::install_getter`.
fn install_getter(proto_obj: *mut ObjectHeader, name: &str) {
    if proto_obj.is_null() {
        return;
    }
    unsafe {
        let closure = crate::closure::js_closure_alloc(
            crate::fn_info!(#[realm(realm_decl! { "Temporal.Duration.prototype.blank.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.days.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.hours.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.microseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.milliseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.minutes.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.months.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.nanoseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.seconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.sign.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.weeks.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Duration.prototype.years.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Instant.prototype.epochMilliseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.Instant.prototype.epochNanoseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.calendarId.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.day.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.dayOfWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.dayOfYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.daysInMonth.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.daysInWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.daysInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.era.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.eraYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.inLeapYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.month.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.monthCode.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.monthsInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.weekOfYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.year.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDate.prototype.yearOfWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.calendarId.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.day.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.dayOfWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.dayOfYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.daysInMonth.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.daysInWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.daysInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.era.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.eraYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.hour.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.inLeapYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.microsecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.millisecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.minute.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.month.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.monthCode.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.monthsInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.nanosecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.second.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.weekOfYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.year.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainDateTime.prototype.yearOfWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainMonthDay.prototype.calendarId.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainMonthDay.prototype.day.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainMonthDay.prototype.monthCode.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainTime.prototype.hour.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainTime.prototype.microsecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainTime.prototype.millisecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainTime.prototype.minute.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainTime.prototype.nanosecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainTime.prototype.second.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.calendarId.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.daysInMonth.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.daysInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.era.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.eraYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.inLeapYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.month.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.monthCode.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.monthsInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.PlainYearMonth.prototype.year.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.calendarId.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.day.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.dayOfWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.dayOfYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.daysInMonth.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.daysInWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.daysInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.epochMilliseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.epochNanoseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.era.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.eraYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.hour.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.hoursInDay.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.inLeapYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.microsecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.millisecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.minute.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.month.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.monthCode.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.monthsInYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.nanosecond.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.offset.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.offsetNanoseconds.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.second.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.timeZoneId.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.weekOfYear.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.year.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; "Temporal.ZonedDateTime.prototype.yearOfWeek.<get>", births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; })] temporal_proto_getter_thunk, 0; with_declared(0)),
            0,
        );
        if closure.is_null() {
            return;
        }
        super::native_module::set_bound_native_closure_name(closure, &format!("get {name}"));
        super::native_module::set_builtin_closure_length(closure as usize, 0);
        super::native_module::set_builtin_closure_non_constructable(closure as usize);
        let getter_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        // #6809: Temporal cell reads use the brand dispatcher and direct
        // prototype reads use the per-owner descriptor marker installed by
        // this helper. Keep startup gate-neutral.
        super::object_ops::install_builtin_getter(proto_obj, name, getter_bits);
        super::set_builtin_property_attrs(
            closure as usize,
            "name".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        super::set_builtin_property_attrs(
            closure as usize,
            "length".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
    }
}

/// Install one variadic method (spec `length` = `spec_length`) onto a Temporal
/// prototype, sharing the generic method thunk.
fn install_method(proto_obj: *mut ObjectHeader, name: &str, spec_length: u32) {
    super::global_this::install_proto_method_rest_with_length(
        proto_obj,
        name,
        crate::fn_info!(#[realm(realm_decl! { Temporal.Duration.prototype.abs, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Duration.prototype.negated, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Duration.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Duration.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Duration.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Duration.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Instant.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Instant.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Instant.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Instant.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDate.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDate.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDate.prototype.toPlainDateTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDate.prototype.toPlainMonthDay, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDate.prototype.toPlainYearMonth, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDate.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDate.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDateTime.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDateTime.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDateTime.prototype.toPlainDate, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDateTime.prototype.toPlainTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDateTime.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDateTime.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainDateTime.prototype.withPlainTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainMonthDay.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainMonthDay.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainMonthDay.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainMonthDay.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainTime.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainTime.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainTime.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainTime.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainYearMonth.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainYearMonth.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainYearMonth.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.PlainYearMonth.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.startOfDay, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.toInstant, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.toJSON, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.toLocaleString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.toPlainDate, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.toPlainDateTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.toPlainTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.toString, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.valueOf, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.ZonedDateTime.prototype.withPlainTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError]; Temporal.Duration.prototype.add, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_duration_prototype_add_metadata); Temporal.Duration.prototype.round, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_duration_prototype_round_metadata); Temporal.Duration.prototype.subtract, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_duration_prototype_subtract_metadata); Temporal.Duration.prototype.total, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_duration_prototype_total_metadata); Temporal.Duration.prototype.with, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_duration_prototype_with_metadata); Temporal.Instant.prototype.add, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_instant_prototype_add_metadata); Temporal.Instant.prototype.equals, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_instant_prototype_equals_metadata); Temporal.Instant.prototype.round, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_instant_prototype_round_metadata); Temporal.Instant.prototype.since, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_instant_prototype_since_metadata); Temporal.Instant.prototype.subtract, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_instant_prototype_subtract_metadata); Temporal.Instant.prototype.toZonedDateTimeISO, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_instant_prototype_tozoneddatetimeiso_metadata); Temporal.Instant.prototype.until, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_instant_prototype_until_metadata); Temporal.PlainDate.prototype.add, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_add_metadata); Temporal.PlainDate.prototype.equals, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_equals_metadata); Temporal.PlainDate.prototype.since, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_since_metadata); Temporal.PlainDate.prototype.subtract, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_subtract_metadata); Temporal.PlainDate.prototype.toZonedDateTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_tozoneddatetime_metadata); Temporal.PlainDate.prototype.until, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_until_metadata); Temporal.PlainDate.prototype.with, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_with_metadata); Temporal.PlainDate.prototype.withCalendar, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindate_prototype_withcalendar_metadata); Temporal.PlainDateTime.prototype.add, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_add_metadata); Temporal.PlainDateTime.prototype.equals, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_equals_metadata); Temporal.PlainDateTime.prototype.round, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_round_metadata); Temporal.PlainDateTime.prototype.since, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_since_metadata); Temporal.PlainDateTime.prototype.subtract, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_subtract_metadata); Temporal.PlainDateTime.prototype.toZonedDateTime, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_tozoneddatetime_metadata); Temporal.PlainDateTime.prototype.until, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_until_metadata); Temporal.PlainDateTime.prototype.with, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_with_metadata); Temporal.PlainDateTime.prototype.withCalendar, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaindatetime_prototype_withcalendar_metadata); Temporal.PlainMonthDay.prototype.equals, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainmonthday_prototype_equals_metadata); Temporal.PlainMonthDay.prototype.toPlainDate, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainmonthday_prototype_toplaindate_metadata); Temporal.PlainMonthDay.prototype.with, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainmonthday_prototype_with_metadata); Temporal.PlainTime.prototype.add, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaintime_prototype_add_metadata); Temporal.PlainTime.prototype.equals, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaintime_prototype_equals_metadata); Temporal.PlainTime.prototype.round, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaintime_prototype_round_metadata); Temporal.PlainTime.prototype.since, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaintime_prototype_since_metadata); Temporal.PlainTime.prototype.subtract, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaintime_prototype_subtract_metadata); Temporal.PlainTime.prototype.until, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaintime_prototype_until_metadata); Temporal.PlainTime.prototype.with, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plaintime_prototype_with_metadata); Temporal.PlainYearMonth.prototype.add, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainyearmonth_prototype_add_metadata); Temporal.PlainYearMonth.prototype.equals, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainyearmonth_prototype_equals_metadata); Temporal.PlainYearMonth.prototype.since, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainyearmonth_prototype_since_metadata); Temporal.PlainYearMonth.prototype.subtract, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainyearmonth_prototype_subtract_metadata); Temporal.PlainYearMonth.prototype.toPlainDate, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainyearmonth_prototype_toplaindate_metadata); Temporal.PlainYearMonth.prototype.until, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainyearmonth_prototype_until_metadata); Temporal.PlainYearMonth.prototype.with, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_plainyearmonth_prototype_with_metadata); Temporal.ZonedDateTime.prototype.add, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_add_metadata); Temporal.ZonedDateTime.prototype.equals, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_equals_metadata); Temporal.ZonedDateTime.prototype.getTimeZoneTransition, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_gettimezonetransition_metadata); Temporal.ZonedDateTime.prototype.round, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_round_metadata); Temporal.ZonedDateTime.prototype.since, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_since_metadata); Temporal.ZonedDateTime.prototype.subtract, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_subtract_metadata); Temporal.ZonedDateTime.prototype.until, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_until_metadata); Temporal.ZonedDateTime.prototype.with, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_with_metadata); Temporal.ZonedDateTime.prototype.withCalendar, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_withcalendar_metadata); Temporal.ZonedDateTime.prototype.withTimeZone, births = [Array, BigInt, Error, Number, RangeError, String, Temporal, TypeError], spec = {length = 1}, emit = metadata(realm_temporal_zoneddatetime_prototype_withtimezone_metadata); })] temporal_proto_method_thunk, 1; with_rest(0), with_flags(crate::closure::FN_BUILTIN)),
        spec_length,
    );
}

/// Resolve a Temporal constructor closure's `.prototype` object pointer,
/// creating + caching it on the closure if absent.
///
/// This populator runs *inside* the globalThis singleton build (Temporal is one
/// of the namespaces installed there), at which point `GLOBAL_THIS_READY` is
/// still false. So we must NOT route through `js_function_prototype_value_for_read`
/// — its `ensure_function_prototype_object` reads `globalThis.Object.prototype`
/// via `js_get_global_this()`, which would spin forever waiting for the very
/// build we're in. Instead we allocate the prototype directly and stamp it onto
/// the closure's `prototype` dynamic prop; a later `Temporal.X.prototype` read
/// (post-init) finds exactly this cached object. The `[[Prototype]]` →
/// `Object.prototype` link is intentionally skipped to stay re-entrancy-free.
fn ctor_prototype(ctor: *mut crate::closure::ClosureHeader) -> *mut ObjectHeader {
    if ctor.is_null() {
        return std::ptr::null_mut();
    }
    // Reuse an already-stamped prototype (idempotent install).
    let existing = crate::closure::closure_get_dynamic_prop(ctor as usize, "prototype");
    let ejv = crate::value::JSValue::from_bits(existing.to_bits());
    if ejv.is_pointer() {
        let p = ejv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        if !p.is_null() {
            return p;
        }
    }
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return std::ptr::null_mut();
    }
    // Stamp the prototype onto the closure's `prototype` dynamic prop ONLY — NOT
    // the GC-scanned class-prototype cache. `ensure_function_prototype_object`'s
    // Temporal gate returns this object for `new`/`.prototype` reads, so it is
    // never overwritten, while staying reachable solely through globalThis (the
    // cache would dangle across the test-suite's arena-fixture swaps → SIGSEGV).
    crate::closure::closure_set_dynamic_prop(
        ctor as usize,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
    );
    // A constructor's `.prototype` is `{ writable: false, enumerable: false,
    // configurable: false }` per spec (Temporal.X.prototype prop-desc tests).
    super::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        super::PropertyAttrs::new(false, false, false),
    );
    proto
}

/// Populate `ctor.prototype` with `getters` (accessors), `methods`
/// (`(name, spec_length)`), a `@@toStringTag` of `tag`, and a `constructor`
/// back-reference. Shared by every Temporal type.
pub(super) fn populate_prototype(
    ctor: *mut crate::closure::ClosureHeader,
    tag: &str,
    getters: &[&str],
    methods: &[(&str, u32)],
) {
    let proto = ctor_prototype(ctor);
    if proto.is_null() {
        return;
    }
    for g in getters {
        install_getter(proto, g);
    }
    for (m, len) in methods {
        install_method(proto, m, *len);
    }
    // `constructor` — `{ writable: true, enumerable: false, configurable: true }`.
    let ctor_val = crate::value::js_nanbox_pointer(ctor as i64);
    let ckey = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    super::define_builtin_data_property(
        proto,
        ckey,
        ctor_val,
        "constructor".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );
    super::global_this::set_intrinsic_to_string_tag(proto, tag);
}
