//! The Temporal operations the always-live runtime reaches.
//!
//! Generic operators (`==` / ToPrimitive / ToString / `JSON.stringify` /
//! `valueOf`), property lookup, `instanceof`, subclass construction, the GC
//! finalizer and `globalThis` population all have a Temporal arm. Those
//! callers are live in every program, so they must not name the
//! `temporal_rs`-backed implementation directly, or the prebuilt full-feature
//! runtime keeps Temporal (and the ICU calendar/time-zone data behind it) in
//! every binary. They keep their cheap `is_temporal_value` /
//! `is_temporal_cell_addr` guards and call these forwarders for the rest; the
//! forwarders reach the implementation only through slots the `temporal`
//! install fills (see `crate::feature_hooks`).
//!
//! A Temporal cell exists only after Temporal code ran, so an empty slot is
//! never reached behind a guard in practice; each forwarder still answers what
//! a build without the feature answers.

use super::{TemporalCell, TemporalKind, TemporalValue};
use crate::feature_hooks::Hook;
use crate::object::ObjectHeader;

static CALL_METHOD: Hook<fn(f64, &str, &[f64]) -> f64> = Hook::empty();
static ISO_STRING: Hook<fn(f64) -> Option<String>> = Hook::empty();
static DURATION_UNIT_VALUES: Hook<fn(f64) -> Option<[f64; 10]>> = Hook::empty();
static GET_PROPERTY: Hook<fn(f64, &str) -> Option<f64>> = Hook::empty();
static HAS_METHOD: Hook<fn(f64, &str) -> bool> = Hook::empty();
static SUBCLASS_CELL: Hook<unsafe fn(usize) -> Option<f64>> = Hook::empty();
static SUBCLASS_SUPER: Hook<unsafe fn(f64, f64, *const f64, usize) -> bool> = Hook::empty();
static KIND_PROTOTYPE: Hook<fn(TemporalKind) -> f64> = Hook::empty();
static INSTALL_NAMESPACE: Hook<fn(*mut ObjectHeader)> = Hook::empty();
static FINALIZE_CELL: Hook<unsafe fn(*mut TemporalCell)> = Hook::empty();
static TO_EPOCH_MS: Hook<fn(&TemporalValue) -> Option<f64>> = Hook::empty();
static CALENDAR_ID: Hook<fn(f64) -> Option<&'static str>> = Hook::empty();
static INSPECT_STRING: Hook<fn(f64) -> Option<String>> = Hook::empty();
static CTOR_KIND: Hook<fn(f64) -> Option<TemporalKind>> = Hook::empty();

pub fn call_method(recv: f64, name: &str, args: &[f64]) -> f64 {
    CALL_METHOD
        .get()
        .map_or(f64::from_bits(crate::value::TAG_UNDEFINED), |f| {
            f(recv, name, args)
        })
}

pub fn iso_string(value: f64) -> Option<String> {
    ISO_STRING.get().and_then(|f| f(value))
}

pub fn duration_unit_values(value: f64) -> Option<[f64; 10]> {
    DURATION_UNIT_VALUES.get().and_then(|f| f(value))
}

pub fn get_property(recv: f64, name: &str) -> Option<f64> {
    GET_PROPERTY.get().and_then(|f| f(recv, name))
}

pub fn has_method(recv: f64, name: &str) -> bool {
    HAS_METHOD.get().is_some_and(|f| f(recv, name))
}

/// # Safety
/// As `crate::object::temporal_subclass_cell`.
pub unsafe fn subclass_cell(obj: usize) -> Option<f64> {
    SUBCLASS_CELL.get().and_then(|f| f(obj))
}

/// # Safety
/// As `crate::object::global_this::temporal_subclass_super`.
pub unsafe fn subclass_super(
    parent: f64,
    this_box: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> bool {
    SUBCLASS_SUPER
        .get()
        .is_some_and(|f| f(parent, this_box, args_ptr, args_len))
}

pub fn kind_prototype(kind: TemporalKind) -> f64 {
    KIND_PROTOTYPE
        .get()
        .map_or(f64::from_bits(crate::value::TAG_UNDEFINED), |f| f(kind))
}

pub fn install_namespace(ns_obj: *mut ObjectHeader) -> bool {
    match INSTALL_NAMESPACE.get() {
        Some(install) => {
            install(ns_obj);
            true
        }
        None => false,
    }
}

/// # Safety
/// As `super::finalize_temporal_cell_for_gc`.
pub unsafe fn finalize_cell(cell: *mut TemporalCell) {
    if let Some(f) = FINALIZE_CELL.get() {
        f(cell);
    }
}

pub fn to_epoch_ms(tv: &TemporalValue) -> Option<f64> {
    TO_EPOCH_MS.get().and_then(|f| f(tv))
}

pub fn calendar_id(value: f64) -> Option<&'static str> {
    CALENDAR_ID.get().and_then(|f| f(value))
}

pub fn inspect_string(value: f64) -> Option<String> {
    INSPECT_STRING.get().and_then(|f| f(value))
}

pub fn ctor_kind(type_ref: f64) -> Option<TemporalKind> {
    CTOR_KIND.get().and_then(|f| f(type_ref))
}

/// The `temporal` install.
#[cfg(feature = "temporal")]
pub(crate) fn install() {
    CALL_METHOD.set(super::dispatch::call_method);
    ISO_STRING.set(super::temporal_iso_string);
    DURATION_UNIT_VALUES.set(super::duration_unit_values);
    GET_PROPERTY.set(super::dispatch::get_property);
    HAS_METHOD.set(super::dispatch::has_method);
    SUBCLASS_CELL.set(crate::object::temporal_subclass_cell);
    SUBCLASS_SUPER.set(crate::object::global_this_temporal_subclass_super);
    KIND_PROTOTYPE.set(crate::object::global_this_temporal_kind_prototype);
    INSTALL_NAMESPACE.set(crate::object::global_this_install_temporal_namespace);
    FINALIZE_CELL.set(super::finalize_temporal_cell_impl);
    TO_EPOCH_MS.set(super::temporal_to_epoch_ms_impl);
    CALENDAR_ID.set(super::temporal_calendar_id_impl);
    INSPECT_STRING.set(super::temporal_inspect_string);
    CTOR_KIND.set(crate::object::global_this_temporal_ctor_kind);
}
