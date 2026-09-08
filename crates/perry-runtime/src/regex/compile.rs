//! `RegExp.prototype.compile(pattern, flags)` (Annex B §B.2.4.1).
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use std::sync::Arc;

use super::class_range_validate::has_out_of_order_double_dash_class_range;
use super::grammar::{
    has_invalid_repeated_quantifier, has_unicode_forbidden_legacy_escape,
    has_unicode_forbidden_pattern, js_regex_to_rust,
};
use super::{
    build_fancy_regex, build_std_regex, get_or_compile_regex, is_regex_pointer, is_valid_ptr,
    is_valid_regex_ptr, js_regexp_get_flags, js_regexp_get_source, js_string_from_str,
    string_as_str, throw_regexp_syntax_error, validate_and_canonicalize_flags, RegExpHeader,
};

/// `RegExp.prototype.compile(pattern, flags)`. Re-initializes the receiver
/// RegExp *in place*: re-validates and recompiles the pattern, updates
/// `.source`/`.flags`, and resets `lastIndex` to 0. Returns the receiver
/// (NaN-boxed). When `pattern` is itself a RegExp its source+flags are adopted
/// and a non-`undefined` `flags` argument is a TypeError.
#[no_mangle]
pub extern "C" fn js_regexp_compile_value(
    re: *mut RegExpHeader,
    pattern_val: f64,
    flags_val: f64,
) -> f64 {
    if !is_valid_regex_ptr(re) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let re_handle = scope.root_raw_mut_ptr(re);
    let pattern_handle = scope.root_nanbox_f64(pattern_val);
    let flags_value_handle = scope.root_nanbox_f64(flags_val);
    let pj = crate::value::JSValue::from_bits(pattern_val.to_bits());
    let fj = crate::value::JSValue::from_bits(flags_val.to_bits());

    let (pattern_owned, flags_owned) = if pj.is_pointer() && is_regex_pointer(pj.as_pointer::<u8>())
    {
        // RegExp source: adopt source+flags; supplying flags is a TypeError.
        if !fj.is_undefined() {
            crate::collection_iter::throw_type_error(
                "Cannot supply flags when constructing one RegExp from another",
            );
        }
        let src_re = pj.as_pointer::<RegExpHeader>();
        let ((src, pattern_val), _) = re_handle.across_mut::<RegExpHeader, _>(|| {
            pattern_handle.across_nanbox(|| js_regexp_get_source(src_re))
        });
        let src_s = if is_valid_ptr(src) {
            string_as_str(src).to_string()
        } else {
            String::new()
        };
        let src_re =
            crate::value::JSValue::from_bits(pattern_val.to_bits()).as_pointer::<RegExpHeader>();
        let ((flg, _), _) = re_handle.across_mut::<RegExpHeader, _>(|| {
            pattern_handle.across_nanbox(|| js_regexp_get_flags(src_re))
        });
        let flg_s = if is_valid_ptr(flg) {
            string_as_str(flg).to_string()
        } else {
            String::new()
        };
        (src_s, flg_s)
    } else {
        // ToString(pattern), with `undefined` -> "" (spec); same for flags.
        // Abstract ToString (§7.1.17) rejects a Symbol with a `TypeError` — the
        // lenient `js_string_coerce` would otherwise stringify it to
        // "Symbol(desc)" (annexB `.../compile/{pattern,flags}-to-string-err`).
        let (pat, flags_val) = if pj.is_undefined() {
            (String::new(), flags_val)
        } else {
            crate::builtins::reject_symbol_to_string(pattern_val);
            let ((p, flags_val), _) = re_handle.across_mut::<RegExpHeader, _>(|| {
                flags_value_handle.across_nanbox(|| crate::builtins::js_string_coerce(pattern_val))
            });
            let pat = if is_valid_ptr(p) {
                string_as_str(p).to_string()
            } else {
                String::new()
            };
            (pat, flags_val)
        };
        let fj = crate::value::JSValue::from_bits(flags_val.to_bits());
        let flg = if fj.is_undefined() {
            String::new()
        } else {
            crate::builtins::reject_symbol_to_string(flags_val);
            let (f, _) = re_handle
                .across_mut::<RegExpHeader, _>(|| crate::builtins::js_string_coerce(flags_val));
            if is_valid_ptr(f) {
                string_as_str(f).to_string()
            } else {
                String::new()
            }
        };
        (pat, flg)
    };

    let canonical_flags = validate_and_canonicalize_flags(&flags_owned);
    let flags_str = canonical_flags.as_str();
    let pattern_str = pattern_owned.as_str();

    // Same SyntaxError validation as `js_regexp_new`: only reject patterns that
    // neither the `regex` crate nor `fancy-regex` accept.
    if has_invalid_repeated_quantifier(pattern_str) {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }
    // `--` is the real ClassSetExpression subtraction operator under the `v`
    // flag (UTS #51) — see the matching comment in `js_regexp_new`.
    if !flags_str.contains('v') && has_out_of_order_double_dash_class_range(pattern_str) {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }
    // Annex B.1.4 leniencies are hard `SyntaxError`s under `/u` (mirror of
    // `js_regexp_new`): legacy escapes for `u`/`v`, plus the structural
    // restrictions for `u` specifically.
    let unicode = flags_str.contains('u') || flags_str.contains('v');
    if unicode && has_unicode_forbidden_legacy_escape(pattern_str) {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }
    if flags_str.contains('u') && has_unicode_forbidden_pattern(pattern_str) {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }
    let translated = js_regex_to_rust(pattern_str);
    if build_std_regex(&translated).is_err() && build_fancy_regex(&translated).is_err() {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }

    // The header OWNS one raw `Arc` reference to its compiled program set
    // (mirrors `js_regexp_new`), so the capped `REGEX_CACHE`/`FANCY_CACHE`
    // (see `REGEX_CACHE_MAX_ENTRIES`) can evict without invalidating this
    // receiver. Refresh the whole program set so it tracks the NEW pattern,
    // not the one the receiver was constructed with.
    // `RegExp.prototype.compile` re-initialises an existing receiver — once per
    // call from user code, not per object — so materialising the shared key
    // here costs nothing measurable.
    let pattern_key: std::sync::Arc<str> = std::sync::Arc::from(pattern_str);
    let flags_key: std::sync::Arc<str> = std::sync::Arc::from(flags_str);
    let std = get_or_compile_regex(&pattern_key, &flags_key);
    let fancy = super::FANCY_CACHE.with(|fc| {
        fc.borrow()
            .get(&(pattern_key.clone(), flags_key.clone()))
            .cloned()
    });
    let repeat = super::REPEAT_MATCHER_CACHE.with(|cache| {
        cache
            .borrow()
            .get(&(pattern_key.clone(), flags_key.clone()))
            .cloned()
    });
    let programs = Arc::new(super::site_cache::Programs { std, fancy, repeat });
    let matcher_kind = programs.matcher_kind();
    let programs_ptr = Arc::into_raw(programs);
    let (canonical_flags_ptr, _) =
        re_handle.across_mut::<RegExpHeader, _>(|| js_string_from_str(flags_str));
    let canonical_flags_handle = scope.root_string_ptr(canonical_flags_ptr);
    let ((pattern_ptr, canonical_flags_ptr), re) = re_handle.across_mut::<RegExpHeader, _>(|| {
        canonical_flags_handle
            .across_const::<crate::StringHeader, _>(|| js_string_from_str(pattern_str))
    });
    unsafe {
        let old_programs_ptr = (*re).programs_ptr;
        (*re).matcher_kind = matcher_kind;
        (*re).programs_ptr = programs_ptr;
        // Release the receiver's PREVIOUS owned references now that the new
        // ones are installed (recompiling the same pattern is fine: the fresh
        // `into_raw` reference above keeps the shared program alive).
        if !old_programs_ptr.is_null() {
            drop(Arc::from_raw(old_programs_ptr));
        }
        (*re).pattern_ptr = pattern_ptr;
        (*re).flags_ptr = canonical_flags_ptr;
        // These are traced header edges. Unlike construction, `compile` can
        // rewrite a tenured receiver with newly allocated nursery strings, so
        // both stores need the ordinary runtime barrier.
        let parent = re as usize;
        crate::gc::runtime_write_barrier_gc_slot(
            parent,
            std::ptr::addr_of!((*re).pattern_ptr) as usize,
            crate::value::js_nanbox_string(pattern_ptr as i64).to_bits(),
        );
        crate::gc::runtime_write_barrier_gc_slot(
            parent,
            std::ptr::addr_of!((*re).flags_ptr) as usize,
            crate::value::js_nanbox_string(canonical_flags_ptr as i64).to_bits(),
        );
        (*re).case_insensitive = flags_str.contains('i');
        (*re).global = flags_str.contains('g');
        (*re).multiline = flags_str.contains('m');
        (*re).sticky = flags_str.contains('y');
        (*re).dot_all = flags_str.contains('s');
        (*re).unicode = flags_str.contains('u') || flags_str.contains('v');
        (*re).has_indices = flags_str.contains('d');
    }
    // Spec RegExpInitialize step 12: `Set(obj, "lastIndex", 0, true)` runs LAST,
    // with the *Throw* flag. A user-frozen `lastIndex`
    // (`Object.defineProperty(re, "lastIndex", { writable: false })`) makes this
    // a `TypeError` — but only *after* `.source`/`.flags` have already been
    // updated above (annexB `.../compile/pattern-regexp-immutable-lastindex`).
    let ((), re) =
        re_handle.across_mut::<RegExpHeader, _>(|| super::set_last_index_throwing(re, 0));
    f64::from_bits(crate::value::JSValue::pointer(re as *const u8).bits())
}
