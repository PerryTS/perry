//! The Intl operations the always-live runtime reaches.
//!
//! `instanceof`, class construction (`class X extends Intl.<Ctor>`) and
//! `Number`/`BigInt.prototype.toLocaleString(locales, options)` have an Intl
//! arm. Those callers are live in every program, so they must not name the
//! ECMA-402 machinery directly, or the prebuilt full-feature runtime keeps it
//! in every binary. They call these forwarders, which reach it only through
//! slots the `intl-namespace` install fills (see `crate::feature_hooks`). An
//! empty slot answers what a build without the feature answers: no Intl
//! constructor value can exist, so the probes never match, and the locale
//! formatting falls back to the plain ECMA-262 rendering.

use crate::feature_hooks::Hook;
use crate::string::StringHeader;

static INSTANCEOF: Hook<fn(f64, f64) -> Option<bool>> = Hook::empty();
static IS_CONSTRUCTOR_VALUE: Hook<fn(f64) -> bool> = Hook::empty();
static SUBCLASS_SUPER: Hook<unsafe fn(f64, f64, *const f64, usize) -> bool> = Hook::empty();
static NUMBER_TO_LOCALE_STRING: Hook<fn(f64, f64, f64) -> *mut StringHeader> = Hook::empty();
static BIGINT_TO_LOCALE_STRING: Hook<fn(f64, f64, f64) -> *mut StringHeader> = Hook::empty();

pub(crate) fn intl_instanceof(value: f64, type_ref: f64) -> Option<bool> {
    INSTANCEOF.get().and_then(|f| f(value, type_ref))
}

pub(crate) fn is_intl_constructor_value(value: f64) -> bool {
    IS_CONSTRUCTOR_VALUE.get().is_some_and(|f| f(value))
}

/// # Safety
/// As `crate::intl::intl_subclass_super`.
pub(crate) unsafe fn intl_subclass_super(
    parent: f64,
    this_box: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> bool {
    SUBCLASS_SUPER
        .get()
        .is_some_and(|f| f(parent, this_box, args_ptr, args_len))
}

/// `None` without the ECMA-402 formatter.
pub(crate) fn number_to_locale_string(
    value: f64,
    locales: f64,
    options: f64,
) -> Option<*mut StringHeader> {
    NUMBER_TO_LOCALE_STRING
        .get()
        .map(|f| f(value, locales, options))
}

/// `None` without the ECMA-402 formatter.
pub(crate) fn bigint_to_locale_string(
    value: f64,
    locales: f64,
    options: f64,
) -> Option<*mut StringHeader> {
    BIGINT_TO_LOCALE_STRING
        .get()
        .map(|f| f(value, locales, options))
}

/// The hub half of the `intl-namespace` install.
#[cfg(feature = "intl-namespace")]
pub(crate) fn install() {
    INSTANCEOF.set(super::intl_instanceof);
    IS_CONSTRUCTOR_VALUE.set(super::is_intl_constructor_value);
    SUBCLASS_SUPER.set(super::intl_subclass_super);
    NUMBER_TO_LOCALE_STRING.set(super::number_to_locale_string);
    BIGINT_TO_LOCALE_STRING.set(super::bigint_to_locale_string);
    crate::object::date_proto_thunks::install_date_to_locale_opts();
}
