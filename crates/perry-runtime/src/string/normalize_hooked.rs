//! The Unicode normalization tables, reached through hook slots.
//!
//! `String.prototype.normalize`, the canonical-equivalence step of
//! `localeCompare`, `Intl.Collator`'s accent-insensitive keys and the
//! SpecialCasing combining-class checks use `unicode-normalization`
//! (~113 KB of decomposition/composition tables). The callers are live in every
//! program, so they call these forwarders; the implementation is reached only
//! through slots the `string-normalize` install fills (see
//! `crate::feature_hooks`). An empty slot answers what a program that never
//! normalizes answers: text passes through unchanged and canonical equivalents
//! compare by their approximate collation.

use super::wtf8::{self, NormalizeForm, Wtf8Str};
use crate::feature_hooks::Hook;

type CompareFn = fn(Wtf8Str<'_>, Wtf8Str<'_>) -> f64;

pub(crate) use crate::string_combining_class::ccc;
static NORMALIZE_RUN: Hook<fn(&str, NormalizeForm) -> String> = Hook::empty();
static CANONICAL: Hook<for<'a> fn(Wtf8Str<'a>, Wtf8Str<'a>, CompareFn) -> f64> = Hook::empty();
static COLLATION_NORMALIZE: Hook<fn(&str) -> String> = Hook::empty();
static BASE_COLLATION_KEY: Hook<fn(&str, bool) -> String> = Hook::empty();

/// Normalize one lone-surrogate-free run. Unchanged text without the install;
/// form *validity* is still enforced by the caller.
pub(crate) fn normalize_run(run: &str, form: NormalizeForm) -> String {
    match NORMALIZE_RUN.get() {
        Some(f) => f(run, form),
        None => run.to_string(),
    }
}

/// The canonical-equivalence step shared by every locale-aware comparison
/// mode, after the equal-bytes and all-ASCII shortcuts: compare the NFC forms.
pub(crate) fn canonical(a: Wtf8Str<'_>, b: Wtf8Str<'_>, compare: CompareFn) -> f64 {
    match CANONICAL.get() {
        Some(f) => f(a, b, compare),
        None => compare(a, b),
    }
}

/// `Intl.Collator`: NFC, so canonical equivalents are equal.
pub(crate) fn collation_normalize(s: &str) -> String {
    match COLLATION_NORMALIZE.get() {
        Some(f) => f(s),
        None => s.to_string(),
    }
}

/// `Intl.Collator` base-sensitivity key: NFD without combining marks.
pub(crate) fn base_collation_key(s: &str, preserve_case: bool) -> String {
    match BASE_COLLATION_KEY.get() {
        Some(f) => f(s, preserve_case),
        None if preserve_case => s.to_string(),
        None => s.to_lowercase(),
    }
}

fn normalize_run_impl(run: &str, form: NormalizeForm) -> String {
    use unicode_normalization::UnicodeNormalization;
    match form {
        NormalizeForm::Nfc => run.nfc().collect(),
        NormalizeForm::Nfd => run.nfd().collect(),
        NormalizeForm::Nfkc => run.nfkc().collect(),
        NormalizeForm::Nfkd => run.nfkd().collect(),
    }
}

fn canonical_impl(a: Wtf8Str<'_>, b: Wtf8Str<'_>, compare: CompareFn) -> f64 {
    use unicode_normalization::{is_nfc_quick, IsNormalized, UnicodeNormalization};
    // Non-ASCII text is still usually *already* NFC — precomposed letters,
    // CJK and emoji all are; only combining marks and decomposable
    // singletons are not. The quick check is a table lookup per scalar and
    // allocates nothing, so only text that genuinely needs rewriting pays
    // for the two `String`s. (#10094: this ran on every comparison, and a
    // sort pays it O(n log n) times.)
    if let (Some(a_str), Some(b_str)) = (a.as_str(), b.as_str()) {
        if is_nfc_quick(a_str.chars()) == IsNormalized::Yes
            && is_nfc_quick(b_str.chars()) == IsNormalized::Yes
        {
            return compare(a, b);
        }
        let a_nfc: String = a_str.nfc().collect();
        let b_nfc: String = b_str.nfc().collect();
        return compare(Wtf8Str::from_str(&a_nfc), Wtf8Str::from_str(&b_nfc));
    }
    // A WTF-8 payload: `is_nfc_quick` takes `char`s, so there is no quick
    // check to take, and the run-splitting normalizer does the work
    // instead. (#10692)
    let a_nfc = wtf8::normalize(a, NormalizeForm::Nfc);
    let b_nfc = wtf8::normalize(b, NormalizeForm::Nfc);
    compare(
        Wtf8Str::from_wtf8_bytes(&a_nfc),
        Wtf8Str::from_wtf8_bytes(&b_nfc),
    )
}

fn collation_normalize_impl(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    // NFC (composition), not NFD: it makes canonical equivalents equal while
    // keeping precomposed `å/ä/ö` intact for the Swedish fast path.
    s.nfc().collect()
}

fn base_collation_key_impl(s: &str, preserve_case: bool) -> String {
    use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
    s.nfd()
        .filter(|ch| !is_combining_mark(*ch))
        .flat_map(|ch| {
            if preserve_case {
                ch.to_string().chars().collect::<Vec<_>>()
            } else {
                ch.to_lowercase().collect::<Vec<_>>()
            }
        })
        .collect()
}

/// The `string-normalize` install.
pub(crate) fn install() {
    crate::string_combining_class::install();
    NORMALIZE_RUN.set(normalize_run_impl);
    CANONICAL.set(canonical_impl);
    COLLATION_NORMALIZE.set(collation_normalize_impl);
    BASE_COLLATION_KEY.set(base_collation_key_impl);
}
