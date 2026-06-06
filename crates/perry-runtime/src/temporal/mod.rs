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

pub mod dispatch;
pub mod duration;
pub mod instant;
pub mod now;
pub mod plain_date;
pub mod plain_date_time;
pub mod plain_month_day;
pub mod plain_time;
pub mod plain_year_month;
pub mod zoned_date_time;

const NANBOX_PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// The concrete `temporal_rs` value carried by a [`TemporalCell`]. The active
/// variant is the type's brand sub-kind; [`TemporalValue::kind`] exposes it
/// cheaply for dispatch without re-matching.
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
        std::ptr::write(ptr, TemporalCell { value: boxed });
        f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
    }
}

/// True if `addr` (a cleaned heap address, NOT NaN-boxed bits) points at a
/// `TemporalCell`. Mirrors [`crate::date::is_date_cell_addr`]: reject the
/// small-handle band first (registry ids are pointer-tagged but not real heap),
/// then read the `GcHeader.obj_type`.
#[inline]
pub fn is_temporal_cell_addr(addr: usize) -> bool {
    if addr < 0x100000 || !crate::object::is_valid_obj_ptr(addr as *const u8) {
        return false;
    }
    unsafe {
        let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*header).obj_type == crate::gc::GC_TYPE_TEMPORAL
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
pub unsafe fn finalize_temporal_cell_for_gc(cell: *mut TemporalCell) {
    if cell.is_null() {
        return;
    }
    std::ptr::drop_in_place(cell);
}

/// Render a Temporal value as its canonical ISO-8601 / IXDTF string — the form
/// `toString` and `toJSON` use. Returns `None` only if `value` is not a
/// Temporal cell.
pub fn temporal_iso_string(value: f64) -> Option<String> {
    temporal_value_ref(value).map(temporal_value_iso_string)
}

/// `console.log` / `util.inspect` form: `Temporal.Duration <P1Y…>` — the brand
/// tag followed by the canonical string in angle brackets, matching V8's custom
/// Temporal inspect output. Returns `None` if `value` is not a Temporal cell.
pub fn temporal_inspect_string(value: f64) -> Option<String> {
    temporal_value_ref(value)
        .map(|v| format!("{} <{}>", v.type_name(), temporal_value_iso_string(v)))
}

/// ISO/IXDTF string for an already-borrowed [`TemporalValue`]. `temporal_rs`
/// implements `Display` for each type as its canonical string form, so we defer
/// to that (no formatting options = spec-default precision).
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
