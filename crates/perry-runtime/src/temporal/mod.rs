//! TC39 **Temporal** API runtime support (umbrella #4686, foundation #4687).
//!
//! Perry writes *no* date/calendar/duration/timezone math: every `Temporal.*`
//! value wraps a pure-Rust [`temporal_rs`] struct (the same engine V8 uses) and
//! our code is binding glue. All nine Temporal reference types share **one** GC
//! tag — [`crate::gc::GC_TYPE_TEMPORAL`] — discriminated by the internal
//! [`TemporalValue`] enum, rather than burning a tag + malloc bucket per type.
//!
//! ## Representation
//!
//! A `Temporal.*` value is a **reference type**: a NaN-boxed POINTER
//! (`POINTER_TAG`) to a heap [`TemporalCell`], exactly like [`crate::date`]'s
//! `DateCell`. The cell is:
//! - **non-movable** — a NaN-boxed pointer lives in a plain f64/DOUBLE local
//!   that codegen does not shadow-root; the conservative stack scan keeps it
//!   alive and a stable address means the un-rooted pointer never goes stale
//!   across a GC.
//! - **`pointer_free`** from the GC's view — the embedded `temporal_rs` value is
//!   plain integers + `'static` calendar data, never a JSValue, so no write
//!   barrier and no tracing. Any Rust-heap a variant owns (a `ZonedDateTime`'s
//!   IANA timezone string) is released by [`finalize_temporal_cell_for_gc`] when
//!   the cell is swept (the `TemporalCleanup` finalize hook).
//!
//! Temporal values are immutable per spec — there are no setters, so unlike
//! `DateCell` the cell is never mutated in place after construction.

use crate::value::JSValue;

#[cfg(feature = "temporal")]
pub mod dispatch;
#[cfg(feature = "temporal")]
pub mod duration;
#[cfg(feature = "temporal")]
pub mod instant;
#[cfg(feature = "temporal")]
pub mod now;
#[cfg(feature = "temporal")]
pub mod options;
#[cfg(feature = "temporal")]
pub mod plain_date;
#[cfg(feature = "temporal")]
pub mod plain_date_time;
#[cfg(feature = "temporal")]
pub mod plain_month_day;
#[cfg(feature = "temporal")]
pub mod plain_time;
#[cfg(feature = "temporal")]
pub mod plain_year_month;
#[cfg(feature = "temporal")]
pub mod zoned_date_time;

const NANBOX_PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// The concrete `temporal_rs` value carried by a [`TemporalCell`]. The active
/// variant is the type's brand sub-kind; [`TemporalValue::kind`] exposes it
/// cheaply for dispatch without re-matching.
#[cfg(feature = "temporal")]
pub enum TemporalValue {
    Duration(temporal_rs::Duration),
    Instant(temporal_rs::Instant),
    PlainDate(temporal_rs::PlainDate),
    PlainTime(temporal_rs::PlainTime),
    PlainDateTime(temporal_rs::PlainDateTime),
    PlainYearMonth(temporal_rs::PlainYearMonth),
    PlainMonthDay(temporal_rs::PlainMonthDay),
    ZonedDateTime(temporal_rs::ZonedDateTime),
}

/// Temporal gated off: no `temporal_rs` linked, so a Temporal value can never be
/// constructed. The uninhabited twin keeps `TemporalCell` / the identity layer
/// type-checking without the engine dependency.
#[cfg(not(feature = "temporal"))]
pub enum TemporalValue {}

/// Stable sub-kind discriminator for a Temporal cell. Used by the brand checks
/// in `js_native_call_method` / `js_object_get_field_by_name` to route to the
/// right per-type dispatch without matching the whole enum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum TemporalKind {
    Duration = 0,
    Instant = 1,
    PlainDate = 2,
    PlainTime = 3,
    PlainDateTime = 4,
    PlainYearMonth = 5,
    PlainMonthDay = 6,
    ZonedDateTime = 7,
}

/// Reserved `instanceof` class IDs for the 8 Temporal reference types. A
/// Temporal value is a NaN-boxed pointer to a `TemporalCell` (no ObjectHeader /
/// class chain), so `instanceof` resolves these via a brand-kind probe rather
/// than the class-id walk. Keep in sync with the codegen `cid` match in
/// `perry-codegen/src/expr/instance_misc1.rs` and `object/instanceof.rs`.
pub const CLASS_ID_TEMPORAL_FIRST: u32 = 0xFFFF_0200;
pub const CLASS_ID_TEMPORAL_LAST: u32 = 0xFFFF_0207;

impl TemporalKind {
    /// The reserved `instanceof` class id for this brand kind.
    #[inline]
    pub fn class_id(self) -> u32 {
        CLASS_ID_TEMPORAL_FIRST + self as u32
    }
}

/// True if `value` is a Temporal cell whose brand kind matches the reserved
/// Temporal `class_id` (`CLASS_ID_TEMPORAL_FIRST..=CLASS_ID_TEMPORAL_LAST`).
/// Returns `false` for non-Temporal values or a mismatched kind.
#[inline]
pub fn temporal_value_matches_class_id(value: f64, class_id: u32) -> bool {
    match temporal_kind(value) {
        Some(k) => k.class_id() == class_id,
        None => false,
    }
}

#[cfg(feature = "temporal")]
impl TemporalValue {
    #[inline]
    pub fn kind(&self) -> TemporalKind {
        match self {
            TemporalValue::Duration(_) => TemporalKind::Duration,
            TemporalValue::Instant(_) => TemporalKind::Instant,
            TemporalValue::PlainDate(_) => TemporalKind::PlainDate,
            TemporalValue::PlainTime(_) => TemporalKind::PlainTime,
            TemporalValue::PlainDateTime(_) => TemporalKind::PlainDateTime,
            TemporalValue::PlainYearMonth(_) => TemporalKind::PlainYearMonth,
            TemporalValue::PlainMonthDay(_) => TemporalKind::PlainMonthDay,
            TemporalValue::ZonedDateTime(_) => TemporalKind::ZonedDateTime,
        }
    }

    /// The `Temporal.<X>` constructor name for this value's kind — used by
    /// `console.log` / `util.inspect` (`"Temporal.Duration <ISO>"`).
    #[inline]
    pub fn type_name(&self) -> &'static str {
        match self.kind() {
            TemporalKind::Duration => "Temporal.Duration",
            TemporalKind::Instant => "Temporal.Instant",
            TemporalKind::PlainDate => "Temporal.PlainDate",
            TemporalKind::PlainTime => "Temporal.PlainTime",
            TemporalKind::PlainDateTime => "Temporal.PlainDateTime",
            TemporalKind::PlainYearMonth => "Temporal.PlainYearMonth",
            TemporalKind::PlainMonthDay => "Temporal.PlainMonthDay",
            TemporalKind::ZonedDateTime => "Temporal.ZonedDateTime",
        }
    }
}

/// Temporal gated off: `TemporalValue` is uninhabited, so these methods can only
/// be reached via an impossible value — the `match *self {}` arms are exhaustive
/// over zero variants.
#[cfg(not(feature = "temporal"))]
impl TemporalValue {
    #[inline]
    pub fn kind(&self) -> TemporalKind {
        match *self {}
    }

    #[inline]
    pub fn type_name(&self) -> &'static str {
        match *self {}
    }
}

/// 1-slot heap cell holding a `temporal_rs` value behind a shared GC tag. See
/// the module docs for the non-movable / pointer-free rationale.
///
/// The value is **boxed**, not inlined. `temporal_rs` types carry `i128` fields
/// (→ 16-byte alignment), but arena cells sit 8 bytes past their address (the
/// `GcHeader` prefix), so an inlined 16-aligned payload would land at an
/// 8-aligned address and fault on an aligned 128-bit move. The `Box` keeps the
/// cell itself pointer-sized (8-aligned) and lets the system allocator give the
/// value proper 16-byte alignment. The `Box` pointer is a plain Rust-heap
/// pointer (never a JSValue), so the cell stays `pointer_free` for the GC; the
/// `TemporalCleanup` finalize hook drops it on sweep.
#[repr(C)]
pub struct TemporalCell {
    pub value: Box<TemporalValue>,
}

/// Allocate a fresh Temporal cell wrapping `value` and return it as a NaN-boxed
/// pointer (an f64 carrying `POINTER_TAG`).
#[cfg(feature = "temporal")]
pub fn alloc_temporal_cell(value: TemporalValue) -> f64 {
    let boxed = Box::new(value);
    unsafe {
        let ptr = crate::arena::arena_alloc_gc(
            std::mem::size_of::<TemporalCell>(),
            std::mem::align_of::<TemporalCell>(),
            crate::gc::GC_TYPE_TEMPORAL,
        ) as *mut TemporalCell;
        // `arena_alloc_gc` hands back uninitialized memory; `write` moves the
        // box pointer in without dropping the (garbage) prior contents.
        // GC_STORE_AUDIT(INIT): fresh cell; the Box payload is malloc-owned, not a GC edge.
        std::ptr::write(ptr, TemporalCell { value: boxed });
        // Address reuse after a sweep must not leak a collected cell's expando
        // properties (`Object.defineProperty` / plain assignment) onto this
        // fresh one — clear any stale side-table slot at this address, exactly
        // as Date/RegExp allocation does.
        crate::object::exotic_expando::expando_clear_on_alloc(ptr as usize);
        f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
    }
}

/// True if `addr` (a cleaned heap address, NOT NaN-boxed bits) points at a
/// `TemporalCell`. Mirrors [`crate::date::is_date_cell_addr`]: reject the
/// small-handle band first (registry ids are pointer-tagged but not real heap),
/// then read the `GcHeader.obj_type`.
#[inline]
pub fn is_temporal_cell_addr(addr: usize) -> bool {
    // #5625: small typed arrays and `Buffer`s are raw-`alloc`'d off the GC heap
    // with NO `GcHeader` prefix, so the word at `addr - GC_HEADER_SIZE` is
    // unrelated heap memory that can coincidentally hold the `GC_TYPE_TEMPORAL`
    // tag (observed: a 4-element typed array whose stale back-read obj_type was
    // 18 → `<TA>.slice()` mis-routed through Temporal dispatch and deref'd a
    // garbage cell). They are never `TemporalCell`s; reject them via the side
    // tables first, exactly as `crate::date::is_date_cell_addr` does.
    if crate::typedarray::is_offheap_sidetable_alloc(addr) {
        return false;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(header) => header.obj_type == crate::gc::GC_TYPE_TEMPORAL,
        None => false,
    }
}

/// True if `value` is any Temporal value — a NaN-boxed pointer to a
/// `TemporalCell`.
#[inline]
pub fn is_temporal_value(value: f64) -> bool {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return false;
    }
    is_temporal_cell_addr((bits & NANBOX_PTR_MASK) as usize)
}

/// Borrow the `TemporalValue` a Temporal pointer refers to, or `None` if
/// `value` is not a Temporal cell. The borrow is valid as long as the cell is
/// live (which the caller's stack reference guarantees).
#[inline]
#[allow(clippy::needless_lifetimes)]
pub fn temporal_value_ref<'a>(value: f64) -> Option<&'a TemporalValue> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let addr = (bits & NANBOX_PTR_MASK) as usize;
    if !is_temporal_cell_addr(addr) {
        return None;
    }
    // `&*box` derefs the `Box<TemporalValue>` to `&TemporalValue`.
    unsafe { Some(&*(*(addr as *const TemporalCell)).value) }
}

/// Like [`temporal_value_ref`] but also resolves `class X extends Temporal.<Type>`
/// subclass instances by reading the stashed `__perry_temporal_cell__` field.
/// Use this in `ToTemporalXxx` coercions so `compare`/`from`/`until`/`since`
/// use internal slots instead of property getters. (#5587)
#[cfg(feature = "temporal")]
#[inline]
#[allow(clippy::needless_lifetimes)]
pub fn temporal_value_ref_or_subclass<'a>(value: f64) -> Option<&'a TemporalValue> {
    temporal_value_ref(value).or_else(|| {
        let bits = value.to_bits();
        if !crate::value::JSValue::from_bits(bits).is_pointer() {
            return None;
        }
        let addr = (bits & NANBOX_PTR_MASK) as usize;
        unsafe { crate::object::temporal_subclass_cell(addr) }.and_then(temporal_value_ref)
    })
}

/// The brand sub-kind of a Temporal value, or `None` if not a Temporal cell.
#[inline]
pub fn temporal_kind(value: f64) -> Option<TemporalKind> {
    temporal_value_ref(value).map(TemporalValue::kind)
}

/// Drop the embedded `temporal_rs` value when a Temporal cell is swept,
/// releasing any Rust-heap it owns (e.g. a `ZonedDateTime` timezone string).
/// Registered as the `TemporalCleanup` finalize hook in `gc/types.rs`.
///
/// # Safety
/// `cell` must point at a live, fully-initialized `TemporalCell` that the GC is
/// about to reclaim; it is not read again afterwards.
#[cfg(feature = "temporal")]
pub unsafe fn finalize_temporal_cell_for_gc(cell: *mut TemporalCell) {
    if cell.is_null() {
        return;
    }
    std::ptr::drop_in_place(cell);
}

/// Temporal gated off: no Temporal cell is ever allocated, so the GC never
/// reaches this finalize hook. Kept as a no-op so `gc/types.rs`'s registration
/// resolves without the engine.
#[cfg(not(feature = "temporal"))]
pub unsafe fn finalize_temporal_cell_for_gc(_cell: *mut TemporalCell) {}

/// Convert a Temporal value to epoch milliseconds for Intl.DateTimeFormat.
///
/// Each Temporal type maps its fields to a Unix timestamp (UTC):
/// - `Instant`/`ZonedDateTime`: direct epoch_milliseconds()
/// - `PlainDate`/`PlainDateTime`: treat as UTC midnight / UTC wall-clock time
/// - `PlainTime`: use epoch base date 1970-01-01 with the time fields
/// - `PlainYearMonth`: use day=1 for the epoch base
/// - `PlainMonthDay`: use year=1970 for the epoch base
/// - `Duration`: no epoch representation → `None`
#[cfg(feature = "temporal")]
pub fn temporal_to_epoch_ms(tv: &TemporalValue) -> Option<f64> {
    let secs: i64 = match tv {
        TemporalValue::Instant(i) => return Some(i.epoch_milliseconds() as f64),
        TemporalValue::ZonedDateTime(z) => return Some(z.epoch_milliseconds() as f64),
        TemporalValue::PlainDate(d) => crate::date::components_to_timestamp(
            d.year(),
            d.month() as u32,
            d.day() as u32,
            0,
            0,
            0,
        ),
        TemporalValue::PlainDateTime(dt) => crate::date::components_to_timestamp(
            dt.year(),
            dt.month() as u32,
            dt.day() as u32,
            dt.hour() as u32,
            dt.minute() as u32,
            dt.second() as u32,
        ),
        TemporalValue::PlainTime(t) => crate::date::components_to_timestamp(
            1970,
            1,
            1,
            t.hour() as u32,
            t.minute() as u32,
            t.second() as u32,
        ),
        TemporalValue::PlainYearMonth(ym) => {
            crate::date::components_to_timestamp(ym.year(), ym.month() as u32, 1, 0, 0, 0)
        }
        TemporalValue::PlainMonthDay(md) => crate::date::components_to_timestamp(
            1970,
            md.month_code().to_month_integer() as u32,
            md.day() as u32,
            0,
            0,
            0,
        ),
        TemporalValue::Duration(_) => return None,
    };
    Some(secs as f64 * 1000.0)
}

#[cfg(not(feature = "temporal"))]
pub fn temporal_to_epoch_ms(_tv: &TemporalValue) -> Option<f64> {
    match *_tv {}
}

/// Render a Temporal value as its canonical ISO-8601 / IXDTF string — the form
/// `toString` and `toJSON` use. Returns `None` only if `value` is not a
/// Temporal cell.
#[cfg(feature = "temporal")]
pub fn temporal_iso_string(value: f64) -> Option<String> {
    temporal_value_ref(value).map(temporal_value_iso_string)
}

/// `console.log` / `util.inspect` form: `Temporal.Duration <P1Y…>` — the brand
/// tag followed by the canonical string in angle brackets, matching V8's custom
/// Temporal inspect output. Returns `None` if `value` is not a Temporal cell.
#[cfg(feature = "temporal")]
pub fn temporal_inspect_string(value: f64) -> Option<String> {
    temporal_value_ref(value)
        .map(|v| format!("{} <{}>", v.type_name(), temporal_value_iso_string(v)))
}

/// ISO/IXDTF string for an already-borrowed [`TemporalValue`]. `temporal_rs`
/// implements `Display` for each type as its canonical string form, so we defer
/// to that (no formatting options = spec-default precision).
#[cfg(feature = "temporal")]
pub fn temporal_value_iso_string(v: &TemporalValue) -> String {
    match v {
        TemporalValue::Duration(d) => d.to_string(),
        // Instant / PlainTime don't impl Display; their canonical string comes
        // from `to_ixdtf_string` with spec-default rounding (auto precision).
        TemporalValue::Instant(i) => i
            .to_ixdtf_string(
                None,
                temporal_rs::options::ToStringRoundingOptions::default(),
            )
            .unwrap_or_default(),
        TemporalValue::PlainDate(d) => d.to_string(),
        TemporalValue::PlainTime(t) => t
            .to_ixdtf_string(temporal_rs::options::ToStringRoundingOptions::default())
            .unwrap_or_default(),
        TemporalValue::PlainDateTime(dt) => dt.to_string(),
        TemporalValue::PlainYearMonth(ym) => ym.to_string(),
        TemporalValue::PlainMonthDay(md) => md.to_string(),
        TemporalValue::ZonedDateTime(zdt) => zdt.to_string(),
    }
}
