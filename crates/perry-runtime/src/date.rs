//! Date operations runtime support
//!
//! Provides JavaScript Date functionality using system time.
//!
//! A `Date` is a **reference type**: a NaN-boxed pointer to a 1-slot mutable
//! [`DateCell`] holding the millisecond timestamp. This gives JS-correct
//! aliasing semantics — a setter mutation made through any binding (alias,
//! function parameter, closure capture) is visible through all of them
//! (#2089). Getters dereference the cell; setters mutate it in place.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::date_layout::NANBOX_PTR_MASK;

/// Allocate a fresh Date cell holding `ts` and return it as a NaN-boxed
/// pointer (an f64 carrying POINTER_TAG). `ts` may be NaN — that is an
/// *Invalid Date*, still a real Date object for `typeof` / `instanceof`
/// (the cell pointer is what makes it a Date, independent of its time
/// value), so unlike the old value-type representation there is no need
/// for a separate NaN sentinel bit pattern.
pub fn alloc_date_cell(ts: f64) -> f64 {
    unsafe {
        let ptr = crate::arena::arena_alloc_gc(
            std::mem::size_of::<DateCell>(),
            8,
            crate::gc::GC_TYPE_DATE_CELL,
        ) as *mut DateCell;
        // MUST be explicit: the arena reuses free-list memory without zeroing,
        // and since #6759 phase 1 this cell is no longer pointer-free — the
        // collector now scans this slot, so leftover bytes would be followed
        // as a pointer. Since the rule-3 relayout it is also the word the
        // emitted read path loads as a ShapeId, so it is written FIRST: the
        // cell must never be observable carrying a dead tenant's shape.
        (*ptr).meta = std::ptr::null_mut();
        #[cfg(target_pointer_width = "32")]
        {
            // The arena does not zero reused memory, and a pointer-width store
            // above leaves this word as the previous tenant's (a dead object's
            // ShapeId makes the Date read as an ordinary object).
            (*ptr)._meta_padding = 0;
        }
        (*ptr).ts = ts;
        f64::from_bits(crate::value::JSValue::pointer(ptr as *const u8).bits())
    }
}

/// The canonical Invalid Date value — a fresh cell whose time value is NaN.
#[inline]
pub fn date_invalid() -> f64 {
    alloc_date_cell(f64::NAN)
}

/// The [[Prototype]] of a Date cell: the prototype recorded on the cell (an
/// instance of `class D extends Date` links `D.prototype`; `setPrototypeOf`
/// records its target, `null` included), else the realm's `Date.prototype`.
/// Reads the cell's own record only, so property lookup can use it without
/// re-entering the generic prototype resolution.
pub fn date_cell_prototype(value: f64) -> f64 {
    let addr = (value.to_bits() & NANBOX_PTR_MASK) as usize;
    if let Some(bits) = crate::object::prototype_chain::object_static_prototype(addr) {
        return f64::from_bits(bits);
    }
    let ctor = crate::object::js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
    let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
    if ctor_ptr == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype")
}

/// Mutate the `DateCell` `value` points at, writing `ts` into it. No-op if
/// `value` is not a cell (e.g. a raw timestamp). Returns `ts` — the numeric
/// millisecond value a JS Date setter evaluates to. `DateCell` is
/// pointer-free, so this raw f64 store needs no GC write barrier.
#[inline]
fn date_cell_store(value: f64, ts: f64) -> f64 {
    let bits = value.to_bits();
    if crate::value::JSValue::from_bits(bits).is_pointer() {
        let addr = (bits & NANBOX_PTR_MASK) as usize;
        if is_date_cell_addr(addr) {
            unsafe {
                (*(addr as *mut DateCell)).ts = ts;
            }
        }
    }
    ts
}

/// The string every Date string-method returns when the time value is
/// NaN. Matches `String(new Date(NaN))` / `new Date(NaN).toDateString()`
/// etc. Without this, the formatters cast NaN to `0i64` and emit a bogus
/// `1970-01-01…` string (the `<garbage>` reported in issue #748).
fn invalid_date_string() -> *mut crate::StringHeader {
    let s = "Invalid Date";
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

fn throw_invalid_time_value() -> ! {
    let msg = "Invalid time value";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_rangeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Coerce a value to a number for an ordered relational comparison
/// (`date1 < date2`, `date < ms`, …). A Date is a NaN-boxed `DateCell`
/// pointer whose raw bits are a NaN — bare `fcmp` would compare unordered
/// (always false) — so codegen routes non-statically-numeric relational
/// operands through here to dereference the timestamp first. Plain numbers
/// pass straight through.
#[no_mangle]
pub extern "C" fn js_date_coerce_number(value: f64) -> f64 {
    date_cell_timestamp(value)
}

/// Get current timestamp in milliseconds (Date.now())
#[no_mangle]
pub extern "C" fn js_date_now() -> f64 {
    crate::clock::wall_millis()
}

/// Create a new Date from current time, returning a NaN-boxed DateCell pointer.
#[no_mangle]
pub extern "C" fn js_date_new() -> f64 {
    alloc_date_cell(js_date_now())
}

/// Create a new Date from a timestamp (milliseconds since epoch).
/// A NaN timestamp produces a recognizable Invalid Date (a cell with NaN ts).
#[no_mangle]
pub extern "C" fn js_date_new_from_timestamp(timestamp: f64) -> f64 {
    alloc_date_cell(timestamp)
}

/// Create a new Date from a value that could be a number or a NaN-boxed string.
/// A string — heap `STRING_TAG` (0x7FFF) or inline SSO `SHORT_STRING_TAG`
/// (0x7FF9) — is parsed as a date. Otherwise treats the value as a numeric timestamp.
#[no_mangle]
pub extern "C" fn js_date_new_from_value(value: f64) -> f64 {
    let bits = value.to_bits();
    let tag = (bits >> 48) & 0xFFFF;
    let result = if tag == 0x7FFF || tag == 0x7FF9 {
        // NaN-boxed string, heap or inline SSO (#11519) — parse its bytes.
        crate::string::with_string_value_bytes(value, |bytes| match std::str::from_utf8(bytes) {
            Ok(s) => parse_date_string(s),
            Err(_) => f64::NAN,
        })
        .unwrap_or(f64::NAN)
    } else if is_date_value(value) {
        // `new Date(anotherDate)` copies the source's time value (and would
        // otherwise read the pointer bits as a bogus timestamp).
        date_cell_timestamp(value)
    } else {
        // Any other value → ToNumber timestamp. Booleans/null coerce
        // numerically, objects run a single valueOf, and Symbol/BigInt throw
        // (rather than silently producing an Invalid Date from raw pointer bits).
        jsvalue_to_number(value)
    };
    // `new Date(number)` applies TimeClip: `abs(t) > 8.64e15` → Invalid, and a
    // fractional timestamp truncates toward zero (`new Date(123.9).getTime()`
    // === 123). Copying another Date or parsing a string already yields an
    // integral in-range value, so this is idempotent for those paths.
    alloc_date_cell(time_clip(result))
}

/// Number of days from the civil date 1970-01-01 to `y-m-d` (m is 1-based,
/// d is 1-based). Howard Hinnant's `days_from_civil`, generalized to accept
/// arbitrary integer components so day/month overflow and underflow (e.g.
/// `day = 0`, `month = 13`, negative days) normalize the way the JS Date
/// MakeDay/MakeDate algorithm requires. Works for any year in range.
fn days_from_civil(year: i64, month: u32, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = month as i64;
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // day-of-year, day may be off-range
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // can be off [0,146096]
    era * 146097 + doe - 719468
}

/// Assemble a UTC millisecond timestamp from broken-down components using the
/// JS Date algorithm: month is 0-based and may be any integer (rolls into
/// the year), `day` may be 0/negative/overflow (normalized via day count),
/// and time fields likewise normalize. Mirrors ECMA-262 MakeDate(MakeDay,
/// MakeTime). All arithmetic is i64 so out-of-range components match Node.
fn make_utc_ms(year: i64, month0: i64, day: i64, hour: i64, min: i64, sec: i64, ms: i64) -> f64 {
    // Month rollover into the year (handles negative months too).
    let total_months = year * 12 + month0;
    let norm_year = total_months.div_euclid(12);
    let norm_month1 = (total_months.rem_euclid(12) + 1) as u32; // 1..=12
    let days = days_from_civil(norm_year, norm_month1, day);
    let secs = days * 86400 + hour * 3600 + min * 60 + sec;
    (secs * 1000 + ms) as f64
}

/// ECMA-262 `TimeClip(time)`: a constructed/derived millisecond time value is
/// `NaN` when it is non-finite or `abs(t) > 8.64e15`, otherwise it is truncated
/// toward zero (`ToIntegerOrInfinity`) with `-0` normalized to `+0`. Every
/// public Date construction/`UTC`/setter result path funnels its computed time
/// through this so out-of-range dates become Invalid and fractional inputs
/// (`new Date(123.9)`) drop their fraction, matching Node.
#[inline]
pub(crate) fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 {
        return f64::NAN;
    }
    let i = t.trunc();
    // Normalize -0 to +0 (TimeClip step 3: `ToInteger(time) + (+0)`).
    if i == 0.0 {
        0.0
    } else {
        i
    }
}

/// Apply the ECMA-262 `0..=99 => 1900 + y` rebasing for an integral year
/// when (and only when) it falls in that range.
#[inline]
fn rebase_two_digit_year(year: f64) -> f64 {
    // ECMAScript MakeFullYear: y = ToInteger(year) (truncate toward zero);
    // if 0 ≤ y ≤ 99 then the full year is 1900 + y. The truncation happens
    // BEFORE the range test, so `Date.UTC(-0.999999, 0)` rebases to 1900
    // (ToInteger(-0.999999) = 0), not the literal year 0 (test262
    // Date/UTC/year-offset). Non-finite years pass through unchanged.
    if !year.is_finite() {
        return year;
    }
    let yi = year.trunc();
    if (0.0..=99.0).contains(&yi) {
        1900.0 + yi
    } else {
        yi
    }
}

mod parse;
use parse::parse_date_string;

/// Format a calendar year for ISO 8601 (`toISOString` / `toJSON`): `0..=9999`
/// is 4 zero-padded digits ("2024"), every other year uses the expanded sign +
/// 6-digit form ("+275760" / "-271821" / "-000001"). Unlike [`format_year`],
/// which omits the `+` for the human `toString`/`toUTCString` forms.
fn iso_year(year: i64) -> String {
    if (0..=9999).contains(&year) {
        format!("{:04}", year)
    } else if year < 0 {
        format!("-{:06}", -year)
    } else {
        format!("+{:06}", year)
    }
}

/// Get timestamp from Date (date.getTime())
/// Since we store dates as timestamps, this is an identity function
#[no_mangle]
pub extern "C" fn js_date_get_time(timestamp: f64) -> f64 {
    date_cell_timestamp(timestamp)
}

/// Convert Date to ISO 8601 string (date.toISOString())
/// Returns a pointer to a StringHeader
#[no_mangle]
pub extern "C" fn js_date_to_iso_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    // Floor-divide so sub-epoch timestamps round toward -∞: `new Date(-1)` is
    // `1969-12-31T23:59:59.999Z`, not `1970-01-01T00:00:00.001Z`. Truncating
    // division (`/`, `%`) gave `secs = 0, millis = 1` for `-1`.
    let secs = ts_ms.div_euclid(1000);
    let millis = ts_ms.rem_euclid(1000) as u32;

    // Calculate date components from Unix timestamp
    // This is a simplified implementation - proper implementation would use chrono crate
    let (year, month, day, hour, minute, second) = timestamp_to_components(secs);

    // ISO 8601 "YYYY-MM-DDTHH:mm:ss.sssZ"; years outside 0..=9999 use the
    // expanded "±YYYYYY" form (see `iso_year`) — test262
    // Date/parse/time-value-maximum-range.
    let iso_string = format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        iso_year(year as i64),
        month,
        day,
        hour,
        minute,
        second,
        millis
    );

    crate::string::js_string_from_bytes(iso_string.as_ptr(), iso_string.len() as u32)
}

/// Date.prototype.toISOString() — unlike the shared ISO formatter used by
/// JSON internals, the public method must throw RangeError for Invalid Date.
#[no_mangle]
pub extern "C" fn js_date_to_iso_string_or_throw(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        throw_invalid_time_value();
    }
    js_date_to_iso_string(timestamp)
}

/// Get the full year (date.getFullYear()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_full_year(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, _, _, _, _, _, _) = timestamp_to_local_components(secs);
    year as f64
}

/// Get the month (0-11) (date.getMonth()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_month(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, month, _, _, _, _, _) = timestamp_to_local_components(secs);
    (month - 1) as f64 // JavaScript months are 0-indexed
}

/// Get the day of month (1-31) (date.getDate()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_date(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, day, _, _, _, _) = timestamp_to_local_components(secs);
    day as f64
}

/// Get the hour (0-23) (date.getHours()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_hours(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, _, _, _) = timestamp_to_local_components(secs);
    hour as f64
}

/// Get the minutes (0-59) (date.getMinutes()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_minutes(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, minute, _, _) = timestamp_to_local_components(secs);
    minute as f64
}

/// Get the seconds (0-59) (date.getSeconds()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_seconds(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, second, _) = timestamp_to_local_components(secs);
    second as f64
}

/// Get the milliseconds (0-999) (date.getMilliseconds())
#[no_mangle]
pub extern "C" fn js_date_get_milliseconds(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    ts_ms.rem_euclid(1000) as f64
}

/// Get the day of week (0-6, Sunday=0) in LOCAL time (date.getDay()).
#[no_mangle]
pub extern "C" fn js_date_get_day(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(secs);
    // Compute weekday from local-equivalent seconds
    let local_secs = secs + tz_offset;
    weekday_from_timestamp(local_secs) as f64
}

// =====================================================================================
// v0.4.69 — Date method gap fill: parse, UTC, getUTC*, setUTC*, valueOf, toLocale*, etc.
// =====================================================================================

/// Compute the UTC day of week (0=Sunday, 6=Saturday) for a second-precision timestamp.
fn weekday_from_timestamp(secs: i64) -> u32 {
    // 1970-01-01 was a Thursday (day 4 in JS day-of-week semantics).
    let days = if secs >= 0 {
        secs / 86400
    } else {
        (secs - 86399) / 86400 // floor division for negatives
    };
    let dow = (days + 4).rem_euclid(7);
    dow as u32
}

/// Date.parse(isoString) — parse an ISO 8601 string and return ms since epoch.
/// Returns NaN for invalid input.
#[no_mangle]
pub extern "C" fn js_date_parse(str_ptr: *const crate::StringHeader) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(bytes) {
            Ok(s) => parse_date_string(s),
            Err(_) => f64::NAN,
        }
    }
}

/// `Date.UTC(year, month?, day?, hour?, minute?, second?, ms?)` (#2826).
///
/// Codegen passes a buffer of NaN-boxed JS values plus the actual argument
/// count so the runtime can apply Node-correct defaults:
///   - `argc == 0` → NaN (the required `year` is `undefined`);
///   - omitted `month` → 0, omitted `day` → 1, omitted time fields → 0;
///   - integral `year` in `0..=99` is rebased to `1900 + year`;
///   - any provided component that coerces to NaN → NaN (Invalid);
///   - out-of-range components (day 0, month 12, …) normalize via the Date
///     MakeDay/MakeTime algorithm.
///
/// Returns the millisecond timestamp (a plain number, not a DateCell).
#[no_mangle]
pub extern "C" fn js_date_utc(args_ptr: *const f64, argc: i32) -> f64 {
    let argc = argc.max(0) as usize;
    if argc == 0 {
        return f64::NAN;
    }
    let args = unsafe { std::slice::from_raw_parts(args_ptr, argc) };
    let get = |i: usize, default: f64| -> f64 {
        if i < argc {
            jsvalue_to_number(args[i])
        } else {
            default
        }
    };
    let year = rebase_two_digit_year(get(0, f64::NAN));
    let month = get(1, 0.0);
    let day = get(2, 1.0);
    let hour = get(3, 0.0);
    let minute = get(4, 0.0);
    let second = get(5, 0.0);
    let ms = get(6, 0.0);
    // MakeDay/MakeTime return NaN when any component is non-finite (`Infinity`,
    // `-Infinity`, or `NaN`); a bare `is_nan` check would let `Infinity` through
    // and saturate `as i64` to a bogus timestamp, so reject every non-finite
    // component before assembling.
    if !year.is_finite()
        || !month.is_finite()
        || !day.is_finite()
        || !hour.is_finite()
        || !minute.is_finite()
        || !second.is_finite()
        || !ms.is_finite()
    {
        return f64::NAN;
    }
    time_clip(make_utc_ms(
        year as i64,
        month as i64,
        day as i64,
        hour as i64,
        minute as i64,
        second as i64,
        ms as i64,
    ))
}

/// Keepalive anchor for `js_date_utc` — codegen-only `#[no_mangle]` symbols
/// get dead-stripped by the auto-optimize whole-program LLVM bitcode rebuild
/// without a `#[used]` reference (see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_DATE_UTC: extern "C" fn(*const f64, i32) -> f64 = js_date_utc;

/// Coerce a NaN-boxed JS value to a number (ECMAScript ToNumber, restricted
/// to the inputs Date setters/constructors actually receive). `undefined`
/// becomes NaN; numbers pass through; numeric strings parse; everything else
/// is NaN.
/// ECMAScript `ToNumber` for Date arguments (constructor numeric arg, `Date.UTC`,
/// `Date.prototype.set*` components). Throws `TypeError` on Symbol/BigInt (per
/// spec), reads a Date's time value, and runs a *single* `valueOf`/`toString`
/// (`OrdinaryToPrimitive` number hint) for ordinary objects and arrays before
/// the primitive numeric coercion. Primitives use the ordinary string/boolean/
/// nullish coercion.
fn jsvalue_to_number(v: f64) -> f64 {
    // Symbol and BigInt are not numerically convertible.
    if unsafe { crate::symbol::js_is_symbol(v) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
    }
    if crate::value::JSValue::from_bits(v.to_bits()).is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    let bits = v.to_bits();
    let tag = (bits >> 48) & 0xFFFF;
    match tag {
        0x7FFF | 0x7FF9 => {
            // NaN-boxed string, heap or inline SSO (#11519).
            crate::string::with_string_value_bytes(v, |bytes| match std::str::from_utf8(bytes) {
                Ok(s) => {
                    let t = s.trim();
                    if t.is_empty() {
                        0.0
                    } else {
                        t.parse::<f64>().unwrap_or(f64::NAN)
                    }
                }
                Err(_) => f64::NAN,
            })
            .unwrap_or(f64::NAN)
        }
        0x7FFC => {
            // boxed sentinel: undefined / null / false / true
            match bits & 0xFF {
                0x01 => f64::NAN, // undefined
                0x02 => 0.0,      // null → 0
                0x03 => 0.0,      // false
                0x04 => 1.0,      // true
                _ => f64::NAN,
            }
        }
        0x7FFE => {
            // INT32
            ((bits & 0xFFFF_FFFF) as u32 as i32) as f64
        }
        0x7FFD => {
            // Pointer: a Date contributes its time value; any other object (and
            // arrays) coerce via OrdinaryToPrimitive(number) → ToNumber.
            if is_date_value(v) {
                return date_cell_timestamp(v);
            }
            match unsafe { crate::value::ordinary_to_primitive_number_for_add(v) } {
                crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => jsvalue_to_number(p),
                crate::value::OrdinaryToPrimitiveOutcome::DefaultString => f64::NAN,
                crate::value::OrdinaryToPrimitiveOutcome::TypeError => {
                    crate::collection_iter::throw_type_error(
                        "Cannot convert object to primitive value",
                    )
                }
            }
        }
        _ => v, // plain f64 double
    }
}

/// `Date.prototype.set*` family with optional trailing arguments (#2851).
///
/// `field` selects which component the *leading* argument sets:
///   0=FullYear 1=Month 2=Date 3=Hours 4=Minutes 5=Seconds 6=Milliseconds
///   7=Time (setTime). `is_utc != 0` selects the UTC rebuild, else local.
///
/// `args_ptr`/`argc` carry the NaN-boxed call arguments. Per Node:
///   - supplied components update; omitted *trailing* components keep their
///     current value;
///   - a leading `undefined` (e.g. `setHours()`) coerces to NaN and makes
///     the Date Invalid;
///   - any supplied component that coerces to NaN makes the Date Invalid.
/// The receiver cell is mutated in place; the numeric ms result is returned.
#[no_mangle]
pub extern "C" fn js_date_apply_setter(
    date: f64,
    is_utc: i32,
    field: i32,
    args_ptr: *const f64,
    argc: i32,
) -> f64 {
    let argc = argc.max(0) as usize;
    let args = if argc == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(args_ptr, argc) }
    };
    // Spec: every `Date.prototype.set*` reads `thisTimeValue` (the receiver's
    // current `[[DateValue]]`) BEFORE coercing any argument via ToNumber. A
    // user `valueOf` on an argument can re-enter and mutate this very cell
    // (test262 `set*/date-value-read-before-tonumber-when-date-is-{valid,
    // invalid}`); the timestamp captured here is the one the rebuild must use,
    // not whatever the cell holds after the ToNumber side effects. The brand
    // check (`this` must be a Date) happens earlier, in the reflective setter
    // thunks (`object::date_proto_thunks`) and on the codegen instance path.
    let captured = date_cell_timestamp(date);
    // setTime: single value, replaces the whole time. The old value is unused
    // (beyond the brand check above), so no read-before ordering applies.
    if field == 7 {
        let v = if argc == 0 {
            f64::NAN
        } else {
            jsvalue_to_number(args[0])
        };
        return date_cell_store(date, time_clip(v));
    }
    // setYear (annexB): like setFullYear but rebases a truncated year in
    // `0..=99` to `1900 + y`, operates in local time, and has no UTC variant.
    if field == 8 {
        let y_raw = if argc == 0 {
            f64::NAN
        } else {
            jsvalue_to_number(args[0])
        };
        let yyyy = if y_raw.is_nan() {
            f64::NAN
        } else {
            let yi = y_raw.trunc();
            if (0.0..=99.0).contains(&yi) {
                1900.0 + yi
            } else {
                y_raw
            }
        };
        return rebuild_local_with(
            date,
            captured,
            Some(yyyy),
            None,
            None,
            None,
            None,
            None,
            None,
        );
    }
    // Spec / V8 / node v26 observable order for the date/time setters (fields
    // 0..=6): `thisTimeValue` is read FIRST (the `captured` read above, before
    // any ToNumber). THEN every PRESENT argument is ToNumber-coerced
    // left-to-right — all of them, even when the time value is NaN (test262
    // `set*/arg-coercion-order`). ONLY after all coercions does a NaN time
    // value short-circuit to NaN. A `valueOf` on an argument that re-enters
    // and mutates this very Date is therefore NOT visible to the rebuild — the
    // `captured` timestamp is the one used (test262
    // `date-value-read-before-tonumber-when-date-is-{valid,invalid}`).
    let t = captured;
    let is_full_year = field == 0;
    // Coerce every present argument in source order, capturing all `valueOf`
    // side effects, BEFORE the NaN short-circuit below. An ABSENT trailing
    // argument stays `None` ("keep the current field"); a PRESENT argument —
    // even explicit `undefined` — is coerced (so `setMinutes(0, 0, undefined)`
    // yields NaN, not the retained millisecond; test262 set*/arg-*-to-number).
    let coerced: Vec<f64> = args.iter().map(|&a| jsvalue_to_number(a)).collect();
    // For every setter except setFullYear/setUTCFullYear, a NaN time value
    // short-circuits to NaN: the cell is NOT written (so a `valueOf`-installed
    // value persists). setFullYear substitutes +0 for a NaN time value.
    if t.is_nan() && !is_full_year {
        return f64::NAN;
    }
    // #4927: a NaN time value is passed through to the rebuild AS NaN — its
    // NaN branch seeds the spec's `t = +0` substitution from the RAW +0
    // components (1970-01-01 00:00:00), per Date.prototype.setFullYear step 2,
    // which skips LocalTime(t). Substituting 0.0 here ran the epoch through
    // timestamp_to_local_components, so a CET process revived
    // `new Date(NaN).setFullYear(2020)` to local 01:00 instead of 00:00.
    // The leading component is required: an omitted call (`setHours()`) coerces
    // `undefined` → NaN, so the leading slot is always `Some`.
    let lead = Some(coerced.first().copied().unwrap_or(f64::NAN));
    let opt = |i: usize| -> Option<f64> { coerced.get(i).copied() };
    // Map (field, positional index) → the seven rebuild slots.
    // Slots: year, month0, day, hour, minute, second, ms. The first slot of
    // each setter is the required leading component.
    let (year, month0, day, hour, minute, second, ms) = match field {
        0 => (lead, opt(1), opt(2), None, None, None, None), // setFullYear(y, mo?, d?)
        1 => (None, lead, opt(1), None, None, None, None),   // setMonth(mo, d?)
        2 => (None, None, lead, None, None, None, None),     // setDate(d)
        3 => (None, None, None, lead, opt(1), opt(2), opt(3)), // setHours(h, mi?, s?, ms?)
        4 => (None, None, None, None, lead, opt(1), opt(2)), // setMinutes(mi, s?, ms?)
        5 => (None, None, None, None, None, lead, opt(1)),   // setSeconds(s, ms?)
        6 => (None, None, None, None, None, None, lead),     // setMilliseconds(ms)
        _ => return date_cell_store(date, f64::NAN),
    };
    if is_utc != 0 {
        rebuild_with(date, t, year, month0, day, hour, minute, second, ms)
    } else {
        rebuild_local_with(date, t, year, month0, day, hour, minute, second, ms)
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_DATE_APPLY_SETTER: extern "C" fn(f64, i32, i32, *const f64, i32) -> f64 =
    js_date_apply_setter;

/// `new Date(year, month, day?, hour?, minute?, second?, ms?)` — local time.
///
/// JS semantics: month is 0-indexed, defaults are day=1, hour/min/sec/ms=0,
/// year < 100 is rebased to 1900+year. Arguments arrive as NaN-boxed f64
/// values — strings (which dayjs's parseDate uses, capturing regex groups
/// `r[1] = "2024"` etc.) need to be coerced via the string→number path so
/// that the `(year, month, ...)` form is taken instead of falling back to
/// the single-arg "timestamp" form. Without this, dayjs's `format()` ends
/// up reading garbage out of a `getTime()` like `1.9e+214` and reports the
/// year as `292278994`.
///
/// Returns a NaN-boxed `DateCell` pointer; `instanceof Date` recognizes it
/// by the cell's `GcHeader` type.
#[no_mangle]
pub extern "C" fn js_date_new_local_components(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    millisecond: f64,
) -> f64 {
    // Every argument codegen forwards is a *present* component: omitted
    // trailing parameters are padded with their ECMA-262 default literal
    // (`day = 1`, time fields = 0) at the call site, so here we run a plain
    // ToNumber on each — a *present* `undefined` (e.g. `new Date(1899, 11,
    // undefined)` or `DateValue(1899, 11)` where the wrapper forwards all seven
    // params) coerces to NaN and produces an Invalid Date, matching Node.
    let y = jsvalue_to_number(year);
    let m = jsvalue_to_number(month);
    let d = jsvalue_to_number(day);
    let h = jsvalue_to_number(hour);
    let mi = jsvalue_to_number(minute);
    let s = jsvalue_to_number(second);
    let ms = jsvalue_to_number(millisecond);
    // MakeDay/MakeTime yield NaN for any non-finite component (Infinity as well
    // as NaN), so reject all non-finite values before assembling.
    if !y.is_finite()
        || !m.is_finite()
        || !d.is_finite()
        || !h.is_finite()
        || !mi.is_finite()
        || !s.is_finite()
        || !ms.is_finite()
    {
        return alloc_date_cell(f64::NAN);
    }
    // ECMA-262: if 0 <= year < 100 (integral), year = 1900 + year.
    let year_f = rebase_two_digit_year(y);
    // Assemble the local-clock components (month 0-based, overflow
    // normalizes), then reinterpret as UTC by subtracting the local tz
    // offset at that instant.
    let local_ms = make_utc_ms(
        year_f as i64,
        m as i64,
        d as i64,
        h as i64,
        mi as i64,
        s as i64,
        ms as i64,
    );
    let local_secs = (local_ms as i64).div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(local_secs);
    alloc_date_cell(time_clip(local_ms - (tz_offset * 1000) as f64))
}

// --- UTC getters -----------------------------------------------------------
//
// Date cells store a UTC timestamp, but the regular getters intentionally
// convert that timestamp through `localtime`. UTC getters must decompose the
// stored timestamp directly; delegating to `getFullYear`/`getMonth`/`getDate`
// makes them silently timezone-dependent (#6967).

#[no_mangle]
pub extern "C" fn js_date_get_utc_day(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    weekday_from_timestamp(secs) as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_full_year(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let secs = (timestamp as i64).div_euclid(1000);
    let (year, _, _, _, _, _) = timestamp_to_components(secs);
    year as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_month(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let secs = (timestamp as i64).div_euclid(1000);
    let (_, month, _, _, _, _) = timestamp_to_components(secs);
    (month - 1) as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_date(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let secs = (timestamp as i64).div_euclid(1000);
    let (_, _, day, _, _, _) = timestamp_to_components(secs);
    day as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_hours(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, _, _) = timestamp_to_components(secs);
    hour as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_minutes(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, minute, _) = timestamp_to_components(secs);
    minute as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_seconds(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, second) = timestamp_to_components(secs);
    second as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_milliseconds(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    js_date_get_milliseconds(timestamp)
}

/// date.valueOf() — same as getTime(), returns ms timestamp.
#[no_mangle]
pub extern "C" fn js_date_value_of(timestamp: f64) -> f64 {
    // A static `.valueOf()` call is lowered to `DateValueOf` even when the
    // receiver is typed `any` and is actually a Temporal value (e.g. from
    // `Temporal.PlainDate.from(...)`). Every `Temporal.*` type's `valueOf` is a
    // hard `TypeError` (the spec bans implicit numeric coercion / ordering), so
    // route a Temporal receiver to its brand dispatch, which throws — rather
    // than returning the opaque cell as a pseudo-Date timestamp.
    if crate::temporal::is_temporal_value(timestamp) {
        return crate::temporal::hooked::call_method(timestamp, "valueOf", &[]);
    }
    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(timestamp) {
        return payload;
    }
    // A real Date cell yields its numeric timestamp.
    if is_date_value(timestamp) {
        return date_cell_timestamp(timestamp);
    }
    // Any other ordinary object: `obj.valueOf()` follows ordinary member
    // dispatch, so a user-defined own `valueOf` wins over the default
    // Object.prototype.valueOf (which returns the receiver). `Object(x)`
    // returns `x` unchanged, so `Object(x).valueOf()` must run x's own
    // `valueOf` (test262 built-ins/Object/S9.9_A6). Route through the shared
    // `valueOf` dispatch in `js_native_call_method`, which performs the own-
    // property lookup and falls back to the default for plain objects.
    if crate::value::JSValue::from_bits(timestamp.to_bits()).is_pointer() {
        return unsafe {
            crate::object::js_native_call_method(
                timestamp,
                b"valueOf".as_ptr() as *const i8,
                "valueOf".len(),
                std::ptr::null(),
                0,
            )
        };
    }

    date_cell_timestamp(timestamp)
}

/// date.getTimezoneOffset() — returns the difference in minutes between
/// UTC and the local timezone at the given instant. Positive for locales
/// west of UTC, negative for those east (matches the JS/Node convention).
#[no_mangle]
pub extern "C" fn js_date_get_timezone_offset(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset_secs) = timestamp_to_local_components(secs);
    // tz_offset_secs is "seconds east of UTC" (positive for east).
    // JS getTimezoneOffset returns "minutes west of UTC" — opposite sign,
    // minute granularity.
    (-tz_offset_secs / 60) as f64
}

// --- UTC setters: rebuild the timestamp with one component replaced ---
//
// `date` is the receiver — a NaN-boxed DateCell pointer. We dereference its
// current time value, rebuild, then write the new value back INTO the same
// cell so the mutation is visible through every alias/param/closure that
// holds this Date (#2089). The numeric ms is returned (what a JS setter
// evaluates to).

/// Rebuild a Date's timestamp in UTC with the provided component overrides.
/// Each override is an `Option<f64>`: `None` keeps the current field, `Some`
/// replaces it (and a `Some(NaN)` makes the whole Date Invalid). `month` is
/// **0-based** (JS convention), consistent with `js_date_apply_setter`.
#[allow(clippy::too_many_arguments)]
fn rebuild_with(
    date: f64,
    timestamp: f64,
    year: Option<f64>,
    month0: Option<f64>,
    day: Option<f64>,
    hour: Option<f64>,
    minute: Option<f64>,
    second: Option<f64>,
    millisecond: Option<f64>,
) -> f64 {
    let (cy, cm0, cd, ch, cmi, cs, cur_ms) = if timestamp.is_nan() {
        // Setting the year revives an Invalid Date (ECMA MakeDate seeds from
        // year/0/1); any other component setter on an Invalid Date returns NaN
        // WITHOUT writing `[[DateValue]]` (spec step "If t is NaN, return NaN"
        // precedes SetDateValue), so a `valueOf` that mutated the cell during
        // argument coercion keeps its effect (test262
        // `date-value-read-before-tonumber-when-date-is-invalid`).
        if year.is_none() {
            return f64::NAN;
        }
        (1970i64, 0i64, 1i64, 0i64, 0i64, 0i64, 0i64)
    } else {
        let ts_ms = timestamp as i64;
        let secs = ts_ms.div_euclid(1000);
        let cur_ms = ts_ms.rem_euclid(1000);
        let (cy, cm, cd, ch, cmi, cs) = timestamp_to_components(secs);
        (
            cy as i64,
            cm as i64 - 1,
            cd as i64,
            ch as i64,
            cmi as i64,
            cs as i64,
            cur_ms,
        )
    };
    // If any provided override is non-finite (NaN or ±Infinity), the Date
    // becomes Invalid.
    for o in [year, month0, day, hour, minute, second, millisecond]
        .into_iter()
        .flatten()
    {
        if !o.is_finite() {
            return date_cell_store(date, f64::NAN);
        }
    }
    let ms = make_utc_ms(
        year.map(|v| v as i64).unwrap_or(cy),
        month0.map(|v| v as i64).unwrap_or(cm0),
        day.map(|v| v as i64).unwrap_or(cd),
        hour.map(|v| v as i64).unwrap_or(ch),
        minute.map(|v| v as i64).unwrap_or(cmi),
        second.map(|v| v as i64).unwrap_or(cs),
        millisecond.map(|v| v as i64).unwrap_or(cur_ms),
    );
    // TimeClip the rebuilt value: a setter that overflows ±8.64e15 makes the
    // Date Invalid (`new Date(8.64e15).setHours(24)` → NaN).
    date_cell_store(date, time_clip(ms))
}

// --- Local-time setters (#1187 / #2851) ---
//
// `setHours` / `setDate` / `setMonth` / `setFullYear` / etc. interpret their
// arguments in the running process's *local* timezone, so the rebuild has to
// round-trip through `timestamp_to_local_components`. The local-clock
// components get reassembled with the requested components swapped, then we
// subtract the tz offset at that instant to land back at a true UTC epoch.
// Mirrors the conversion in `js_date_new_local_components`. Only setting the
// year revives an Invalid Date; other component setters leave it invalid.

/// Local-time analogue of [`rebuild_with`]. Same `Option<f64>` override
/// contract; `month` is 0-based.
#[allow(clippy::too_many_arguments)]
fn rebuild_local_with(
    date: f64,
    timestamp: f64,
    year: Option<f64>,
    month0: Option<f64>,
    day: Option<f64>,
    hour: Option<f64>,
    minute: Option<f64>,
    second: Option<f64>,
    millisecond: Option<f64>,
) -> f64 {
    let (cy, cm0, cd, ch, cmi, cs, cur_ms) = if timestamp.is_nan() {
        // See `rebuild_with`: a NaN time value with no year override returns NaN
        // without touching `[[DateValue]]`.
        if year.is_none() {
            return f64::NAN;
        }
        (1970i64, 0i64, 1i64, 0i64, 0i64, 0i64, 0i64)
    } else {
        let ts_ms = timestamp as i64;
        let secs = ts_ms.div_euclid(1000);
        let cur_ms = ts_ms.rem_euclid(1000);
        let (cy, cm, cd, ch, cmi, cs, _) = timestamp_to_local_components(secs);
        (
            cy as i64,
            cm as i64 - 1,
            cd as i64,
            ch as i64,
            cmi as i64,
            cs as i64,
            cur_ms,
        )
    };
    for o in [year, month0, day, hour, minute, second, millisecond]
        .into_iter()
        .flatten()
    {
        if !o.is_finite() {
            return date_cell_store(date, f64::NAN);
        }
    }
    let local_ms = make_utc_ms(
        year.map(|v| v as i64).unwrap_or(cy),
        month0.map(|v| v as i64).unwrap_or(cm0),
        day.map(|v| v as i64).unwrap_or(cd),
        hour.map(|v| v as i64).unwrap_or(ch),
        minute.map(|v| v as i64).unwrap_or(cmi),
        second.map(|v| v as i64).unwrap_or(cs),
        millisecond.map(|v| v as i64).unwrap_or(cur_ms),
    );
    let local_secs = (local_ms as i64).div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(local_secs);
    // TimeClip after the local→UTC adjustment so an overflowing local setter
    // (`new Date(8.64e15).setHours(24)`) makes the Date Invalid.
    date_cell_store(date, time_clip(local_ms - (tz_offset * 1000) as f64))
}

// --- String-returning Date methods ---

const WEEKDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Format a calendar year the way V8's date-to-string methods do: years in
/// `0..=9999` are zero-padded to 4 digits ("0020"), negative years carry a
/// leading `-` then the magnitude zero-padded to 4 digits ("-0001",
/// "-123456"), and years `> 9999` print in full ("10000"). `{:04}` alone
/// mis-pads negatives ("-001"), so the sign is handled explicitly. test262
/// Date/prototype/{toString,toDateString,toUTCString}/negative-year.
fn format_year(year: i32) -> String {
    if year < 0 {
        format!("-{:04}", -(year as i64))
    } else {
        format!("{:04}", year)
    }
}

/// `date.toString()` / `String(date)` / `` `${date}` `` — the full local
/// date+time string, e.g. "Mon Jan 15 2024 12:30:45 GMT+0100 (local)", or
/// "Invalid Date". #2089: Date is a reference type now, so the generic
/// object-to-string path would otherwise print "[object Object]" (the old
/// value-type rep printed the bare millisecond number — also non-conformant).
/// The timezone long-name isn't reproduced (Perry has no tz database), so this
/// is close to but not byte-identical with Node for valid dates; Invalid Date
/// matches exactly.
#[no_mangle]
pub extern "C" fn js_date_to_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, hour, minute, second, tz_offset) = timestamp_to_local_components(secs);
    let dow = weekday_from_timestamp(secs + tz_offset) as usize;
    let sign = if tz_offset >= 0 { '+' } else { '-' };
    let abs_off = tz_offset.abs();
    let off_h = abs_off / 3600;
    let off_m = (abs_off % 3600) / 60;
    let s = format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT{}{:02}{:02} (local)",
        WEEKDAY_NAMES[dow],
        MONTH_NAMES[(month - 1) as usize],
        day,
        format_year(year),
        hour,
        minute,
        second,
        sign,
        off_h,
        off_m
    );
    alloc_runtime_string(&s)
}

/// `date.toUTCString()` — e.g. "Sun, 23 Mar 2014 00:00:00 GMT" (UTC), or
/// "Invalid Date". Stored timestamps are already UTC, so components decode
/// directly without a tz offset. `toGMTString` is a legacy alias.
#[no_mangle]
pub extern "C" fn js_date_to_utc_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, hour, minute, second) = timestamp_to_components(secs);
    let dow = weekday_from_timestamp(secs) as usize;
    let s = format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        WEEKDAY_NAMES[dow],
        day,
        MONTH_NAMES[(month - 1) as usize],
        format_year(year),
        hour,
        minute,
        second
    );
    alloc_runtime_string(&s)
}

/// date.toDateString() — e.g. "Mon Jan 15 2024" (local time).
#[no_mangle]
pub extern "C" fn js_date_to_date_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, _, _, _, tz_offset) = timestamp_to_local_components(secs);
    let dow = weekday_from_timestamp(secs + tz_offset) as usize;
    let s = format!(
        "{} {} {:02} {}",
        WEEKDAY_NAMES[dow],
        MONTH_NAMES[(month - 1) as usize],
        day,
        format_year(year)
    );
    alloc_runtime_string(&s)
}

/// date.toTimeString() — e.g. "12:30:45 GMT+0100 (local)" (local time).
#[no_mangle]
pub extern "C" fn js_date_to_time_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, minute, second, tz_offset) = timestamp_to_local_components(secs);
    let sign = if tz_offset >= 0 { '+' } else { '-' };
    let abs_off = tz_offset.abs();
    let off_h = abs_off / 3600;
    let off_m = (abs_off % 3600) / 60;
    let s = format!(
        "{:02}:{:02}:{:02} GMT{}{:02}{:02} (local)",
        hour, minute, second, sign, off_h, off_m
    );
    alloc_runtime_string(&s)
}

/// date.toLocaleDateString() — simple en-US-style date (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_date_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, _, _, _, _) = timestamp_to_local_components(secs);
    let s = format!("{}/{}/{}", month, day, year);
    alloc_runtime_string(&s)
}

/// date.toLocaleTimeString() — simple H:MM:SS AM/PM en-US style (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_time_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, minute, second, _) = timestamp_to_local_components(secs);
    let (h12, suffix) = if hour == 0 {
        (12, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };
    let s = format!("{}:{:02}:{:02} {}", h12, minute, second, suffix);
    alloc_runtime_string(&s)
}

/// date.toLocaleString() — combined date and time (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, hour, minute, second, _) = timestamp_to_local_components(secs);
    let (h12, suffix) = if hour == 0 {
        (12, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };
    let s = format!(
        "{}/{}/{}, {}:{:02}:{:02} {}",
        month, day, year, h12, minute, second, suffix
    );
    alloc_runtime_string(&s)
}

/// date.toJSON() — null for Invalid Date, otherwise the ISO string.
#[no_mangle]
pub extern "C" fn js_date_to_json(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return std::ptr::null_mut();
    }
    js_date_to_iso_string(timestamp)
}

#[cfg(test)]
mod tests;

pub use crate::date_layout::{
    date_cell_timestamp, is_date_cell_addr, is_date_value, is_registered_date_bits, DateCell,
};

#[cfg(feature = "intl-datetime")]
pub(crate) use crate::calendar::{canonicalize_tzdb_name, install_compiled_tzdb};
pub(crate) use crate::calendar::{components_to_timestamp, timestamp_to_local_components};
pub use crate::calendar::{host_time_zone_name, timestamp_to_components, zone_offset_seconds};

pub use crate::clock::js_performance_now;

pub use crate::builtins::js_number_to_locale_string;

use crate::string::alloc_runtime_string;
