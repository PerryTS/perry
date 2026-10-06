//! RegExp runtime support for Perry
//!
//! JavaScript-compatible regular expressions compiled and executed by Perex.
//! RegExp objects are heap-allocated and store the compiled pattern and flags.

use std::cell::RefCell;
use std::ptr;

use crate::string::StringHeader;

use crate::object::ObjectHeader;

#[cfg(feature = "regex-engine")]
mod compile;
mod escape;
#[cfg(feature = "regex-engine")]
mod flags;
#[cfg(feature = "regex-engine")]
mod instance;
#[cfg(feature = "regex-engine")]
pub(crate) use instance::intrinsic_prototype;
#[cfg(feature = "regex-engine")]
mod perex_split_compat;
#[cfg(feature = "regex-engine")]
pub use perex_split_compat::{js_string_split_n, js_string_split_regex, js_string_split_regex_n};
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_split;
#[cfg(feature = "regex-engine")]
pub use perex_split::js_string_split_js;
#[cfg(feature = "regex-engine")]
pub(crate) mod match_all;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_api;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_cache;
#[cfg(feature = "regex-engine")]
mod perex_construct;
#[cfg(all(test, feature = "regex-engine"))]
pub(crate) use perex_construct::test_install_program;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_dispatch;
#[cfg(feature = "regex-engine")]
mod perex_display;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_glob;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_literal_bytes;
#[cfg(feature = "regex-engine")]
mod perex_literal_search;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_match_search;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_memory;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_owner;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_position_hint;
#[cfg(feature = "regex-engine")]
mod perex_remove;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_replace;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_replace_direct;
#[cfg(feature = "regex-engine")]
mod perex_replace_storage;

/// Test-only reader for the native-piece counter (#10411): which backing a
/// replacement's pieces took, rather than a timing that only implies it.
#[cfg(test)]
pub(crate) fn test_native_pieces() -> usize {
    perex_replace_storage::NATIVE_PIECES.with(std::cell::Cell::get)
}
#[cfg(feature = "regex-engine")]
mod literal;
#[cfg(feature = "regex-engine")]
mod perex_substitution;
#[cfg(feature = "regex-engine")]
pub use literal::js_regexp_literal;
#[cfg(feature = "regex-engine")]
pub use perex_replace::{js_string_replace_all_js, js_string_replace_js};
#[cfg(feature = "regex-engine")]
mod perex_replace_compat;
#[cfg(feature = "regex-engine")]
mod perex_results;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_runtime;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_strings;
#[cfg(not(feature = "regex-engine"))]
mod replace_fn;
mod utf16;
#[cfg(feature = "regex-engine")]
pub use compile::js_regexp_compile_value;
#[cfg(not(feature = "regex-engine"))]
use escape::escape_regexp_source;
pub use escape::js_regexp_escape;
#[cfg(feature = "regex-engine")]
pub(crate) use flags::validate_and_canonicalize_flags;
#[cfg(feature = "regex-engine")]
pub(crate) use match_all::dispatch_regexp_string_iterator_method_builtin;
#[cfg(all(test, feature = "regex-engine"))]
pub use match_all::js_string_match_all;
#[cfg(feature = "regex-engine")]
pub use match_all::{
    dispatch_regexp_string_iterator_method, js_string_match_all_js, js_string_match_all_value,
};
#[cfg(all(test, feature = "regex-engine"))]
use utf16::{byte_index_to_utf16_index, utf16_index_to_byte};

/// Class id for `RegExp String Iterator` exotic objects. Referenced by the
/// always-linked iterator-prototype dispatch, so it stays ungated even when
/// the regex engine (which produces these iterators) is compiled out.
pub const REGEXP_STRING_ITERATOR_CLASS_ID: u32 = 0xFFFF_000A;

/// `matchAll` iterator methods reached from the always-live generic dispatchers
/// (`js_native_call_method`, the iterator prototypes' `next`). They reach the
/// regex engine only through slots the `regex-engine` install fills (see
/// `crate::feature_hooks`); without it no RegExp string iterator exists and
/// these answer `None`, so the dispatchers take their no-regex path.
static ITERATOR_METHOD: crate::feature_hooks::Hook<
    unsafe fn(*mut crate::ObjectHeader, &str) -> f64,
> = crate::feature_hooks::Hook::empty();
static ITERATOR_METHOD_BUILTIN: crate::feature_hooks::Hook<
    unsafe fn(*mut crate::ObjectHeader, &str) -> f64,
> = crate::feature_hooks::Hook::empty();

/// # Safety
/// `iter` must be a live RegExp string iterator object.
pub(crate) unsafe fn hooked_iterator_method(
    iter: *mut crate::ObjectHeader,
    method: &str,
) -> Option<f64> {
    ITERATOR_METHOD.get().map(|f| f(iter, method))
}

/// # Safety
/// `iter` must be a live RegExp string iterator object.
pub(crate) unsafe fn hooked_iterator_method_builtin(
    iter: *mut crate::ObjectHeader,
    method: &str,
) -> Option<f64> {
    ITERATOR_METHOD_BUILTIN.get().map(|f| f(iter, method))
}

/// The `regex-engine` install's hub half.
#[cfg(feature = "regex-engine")]
pub(crate) fn install_iterator_hooks() {
    ITERATOR_METHOD.set(dispatch_regexp_string_iterator_method);
    ITERATOR_METHOD_BUILTIN.set(dispatch_regexp_string_iterator_method_builtin);
}
#[cfg(feature = "regex-engine")]
pub use perex_replace_compat::*;
#[cfg(not(feature = "regex-engine"))]
pub use replace_fn::{
    js_string_replace_all_js, js_string_replace_all_string, js_string_replace_all_string_fn,
    js_string_replace_js, js_string_replace_string, js_string_replace_string_fn,
};
#[cfg(feature = "regex-engine")]
mod exec;
#[cfg(feature = "regex-engine")]
mod match_string;
#[cfg(feature = "regex-engine")]
pub use exec::js_regexp_exec;
#[cfg(all(test, feature = "regex-engine"))]
pub use match_string::{js_string_match, js_string_search_regex};
#[cfg(feature = "regex-engine")]
pub use match_string::{
    js_string_match_js, js_string_match_value, js_string_search_js, js_string_search_value,
};

crate::perry_thread_local! {
    #[cfg(feature = "regex-engine")]
    static LAST_EXEC_INDEX: RefCell<f64> = const { RefCell::new(0.0) };

    static LAST_EXEC_GROUPS: RefCell<*mut ObjectHeader> = const { RefCell::new(ptr::null_mut()) };
}

/// The intrinsic private entry is the RegExp brand. A property with the same
/// spelling, a prototype, proxy or unrelated ordinary object cannot supply it.
#[inline]
pub fn regexp_data_of(value: f64) -> Option<*const RegExpData> {
    #[cfg(test)]
    REGEX_PTR_VALIDATION_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let slot = MATCHER_READ.with(|site| site.read(value))?;
    let slot = crate::value::JSValue::from_bits(slot.to_bits());
    slot.is_pointer().then(|| slot.as_pointer::<RegExpData>())
}

/// Raw-receiver ABI retained for the typed HIR calls until S5.
pub type RegExpHeader = ObjectHeader;
pub(crate) const REGEXP_MATCHER: &str = "[[RegExpMatcher]]";
crate::perry_thread_local! {
    static MATCHER_READ: crate::object::IntrinsicPrivateReadSite =
        const { crate::object::IntrinsicPrivateReadSite::new(REGEXP_MATCHER) };
}

/// Read an already branded receiver's immutable data before the next
/// collecting action. Handles must be re-read after such an action.
#[inline]
pub(crate) fn regexp_data_ptr(receiver: *const RegExpHeader) -> *const RegExpData {
    regexp_data_of(crate::value::js_nanbox_pointer(receiver as i64))
        .expect("RegExp receiver must have its intrinsic private matcher")
}

/// Test support: construct a RegExp through the PRODUCTION path
/// (`js_regexp_new`), run one `test()` so the compiled programs are installed
/// on the header, and hand the header back unrooted.
#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_construct_regexp_and_exec_once(pattern: &str, flags: &str) -> *mut RegExpHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let p = scope.root_string_ptr(js_string_from_str(pattern));
    let f = scope.root_string_ptr(js_string_from_str(flags));
    let re = p.with_mut_ptr::<StringHeader, _>(|p| {
        f.with_mut_ptr::<StringHeader, _>(|f| js_regexp_new(p, f))
    });
    let re = scope.root_raw_mut_ptr(re);
    let subject = scope.root_string_ptr(js_string_from_str("abc"));
    re.with_const_ptr::<RegExpHeader, _>(|re| {
        subject.with_const_ptr::<StringHeader, _>(|s| {
            let _ = js_regexp_test(re, s);
        })
    });
    re.with_mut_ptr::<RegExpHeader, _>(|re| re)
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_original_strings_and_program(
    re: *const RegExpHeader,
) -> (*const StringHeader, *const StringHeader, bool) {
    unsafe {
        let program = crate::value::addr_class::try_read_gc_header(
            (*crate::regex::regexp_data_ptr(re)).perex_program as usize,
        );
        (
            (*crate::regex::regexp_data_ptr(re)).pattern_ptr,
            (*crate::regex::regexp_data_ptr(re)).flags_ptr,
            program.is_some_and(|gc| gc.obj_type == crate::gc::GC_TYPE_REGEX_PROGRAM),
        )
    }
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_regexp_program_address(re: *const RegExpHeader) -> usize {
    unsafe { (*crate::regex::regexp_data_ptr(re)).perex_program as usize }
}

/// Construct through the production object/slot layout for relocation tests.
#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_alloc_nursery_regexp_for_move(source: &str, flags: &str) -> *mut RegExpHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_string_ptr(js_string_from_str(source));
    let flags = scope.root_string_ptr(js_string_from_str(flags));
    source.with_const_ptr::<StringHeader, _>(|source| {
        flags.with_const_ptr::<StringHeader, _>(|flags| js_regexp_new(source, flags))
    })
}

/// RegExpData's only edges are the original strings and the compiled program.
#[inline]
pub(crate) unsafe fn regex_gc_slot_ptrs(data: *mut RegExpData) -> (*mut u64, usize) {
    let source = std::ptr::addr_of_mut!((*data).pattern_ptr) as *mut u64;
    debug_assert_eq!(
        std::ptr::addr_of_mut!((*data).flags_ptr) as usize - source as usize,
        8
    );
    (source, 2)
}

#[inline]
pub(crate) unsafe fn regex_program_slot(user_ptr: *mut u8) -> Option<*mut u64> {
    Some(std::ptr::addr_of_mut!((*user_ptr.cast::<RegExpData>()).perex_program) as *mut u64)
}

/// The pattern and flags strings a RegExp was made from, for a structured
/// clone (`new RegExp(source, flags)` makes it again). Allocates nothing.
pub(crate) unsafe fn regexp_source_and_flags(
    re: *const RegExpHeader,
) -> (Option<*const StringHeader>, Option<*const StringHeader>) {
    let valid = |s: *const StringHeader| is_valid_ptr(s).then_some(s);
    (
        valid((*crate::regex::regexp_data_ptr(re)).pattern_ptr),
        valid((*crate::regex::regexp_data_ptr(re)).flags_ptr),
    )
}

/// Immutable original source, canonical flags and program of a RegExp.
/// Never exposed as a JS receiver; the ordinary instance owns this cell through
/// its intrinsic private slot. Recompilation publishes a new cell.
#[repr(C)]
pub struct RegExpData {
    pattern_ptr: *const StringHeader,
    flags_ptr: *const StringHeader,
    pub case_insensitive: bool,
    pub global: bool,
    pub multiline: bool,
    pub sticky: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub has_indices: bool,
    pub(crate) perex_program: *const u8,
}

impl RegExpData {
    /// Boolean prototype getters borrow this immutable cell without a safepoint.
    pub(crate) unsafe fn has_flag(&self, flag: char) -> bool {
        match flag {
            'g' => self.global,
            'i' => self.case_insensitive,
            'm' => self.multiline,
            's' => self.dot_all,
            'y' => self.sticky,
            'd' => self.has_indices,
            // The program's unicode bit covers both u and v; the original flags
            // distinguish the two observable getters.
            'u' | 'v' => {
                let flags = &*self.flags_ptr;
                let bytes = std::slice::from_raw_parts(
                    self.flags_ptr
                        .cast::<u8>()
                        .add(std::mem::size_of::<StringHeader>()),
                    flags.byte_len as usize,
                );
                bytes.contains(&(flag as u8))
            }
            _ => false,
        }
    }
}

/// `ToLength(Get(R, "lastIndex"))` → a non-negative integer match offset. The
/// stored value may be any JSValue (e.g. `re.lastIndex = { valueOf() {…} }`), so
/// coerce via `ToNumber` (which invokes `valueOf`/`toString`), then `ToInteger`,
/// clamped to ≥ 0.
#[cfg(feature = "regex-engine")]
pub(crate) fn regex_last_index_offset(re: *const RegExpHeader) -> usize {
    let stored = get_last_index(re);
    if crate::value::JSValue::from_bits(stored.to_bits()).is_number() {
        return stored.max(0.0).floor().min(9_007_199_254_740_991.0) as usize;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stored = scope.root_nanbox_f64(stored);
    perex_api::finish(perex_dispatch::to_length(&stored)) as usize
}

#[cfg(feature = "regex-engine")]
#[inline]
pub(crate) fn store_last_index_number(re: *mut RegExpHeader, n: usize) {
    set_last_index(re, n as f64);
}

/// Spec `Set(R, "lastIndex", n, true)` — the lastIndex updates in
/// RegExpBuiltinExec (steps 14/18) are performed with the *Throw* flag set.
/// A user can make `lastIndex` non-writable
/// (`Object.defineProperty(re, "lastIndex", { writable: false })`); the
/// throwing setter then raises a `TypeError` rather than silently dropping the
/// write (test262 prototype/{exec,test}/y-fail-lastindex-no-write). When
/// `lastIndex` is writable (the default) this just stores the number.
#[cfg(feature = "regex-engine")]
pub(crate) fn set_last_index_throwing(re: *mut RegExpHeader, n: usize) {
    set_last_index(re, n as f64);
}

/// Check if a pointer is valid (not null and not a small invalid value from bad NaN-unboxing)
#[inline]
pub(crate) fn is_valid_ptr<T>(p: *const T) -> bool {
    !p.is_null() && (p as usize) >= 0x1000
}

/// Internal helper: Get string data from StringHeader
pub(crate) fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe { std::str::from_utf8_unchecked(string_as_bytes(s)) }
}

/// Internal helper: get the byte payload without assuming it is Unicode
/// scalar UTF-8. JavaScript strings containing lone surrogates use WTF-8.
pub(crate) fn string_as_bytes<'a>(s: *const StringHeader) -> &'a [u8] {
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len)
    }
}

/// Internal helper: Create a StringHeader from a Rust &str
pub(super) fn js_string_from_str(s: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

#[cfg(feature = "regex-engine")]
/// Throw a `SyntaxError` with the given message and never return.
#[cfg(feature = "regex-engine")]
pub(super) fn throw_regexp_syntax_error(message: &str) -> ! {
    let msg = js_string_from_str(message);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Create a new RegExp from pattern and flags strings
/// Returns a pointer to RegExpHeader
///
/// Compile the original pattern with Perex before publishing a fresh object.
/// Source and flags remain traced string edges; the compiled program is a GC
/// leaf owned through the header. Every evaluation creates a distinct header.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_new(
    pattern: *const StringHeader,
    flags: *const StringHeader,
) -> *mut RegExpHeader {
    perex_api::finish(perex_construct::new(pattern, flags))
}

/// ECMA-262 RegExp constructor (`new RegExp(pattern, flags)`), spec 22.2.4.
/// Handles every argument shape the string/string `js_regexp_new` cannot:
///
///   * `pattern` is a RegExp → reuse its `[[OriginalSource]]`; if `flags` is
///     `undefined`, reuse its `[[OriginalFlags]]`, else `ToString(flags)`.
///   * `pattern` is `undefined` → empty source.
///   * `pattern` is anything else → `ToString(pattern)`.
///   * `flags` is `undefined` → empty (unless inherited from a RegExp pattern);
///     anything else → `ToString(flags)` (so `{}` becomes `"[object Object]"`,
///     which `js_regexp_new` then rejects with a SyntaxError).
///
/// `ToString` runs through the coercing method path so a throwing
/// `toString`/`valueOf` propagates.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_construct(pattern: f64, flags: f64) -> *mut RegExpHeader {
    perex_construct::construct(pattern, flags, false)
}

/// `RegExp(...)` invoked as a *function* (not `new`). ECMA-262 22.2.4.1 step 2:
/// when `NewTarget` is undefined, `pattern` is a RegExp and `flags` is
/// `undefined`, and `pattern.constructor` is the `RegExp` intrinsic, the call
/// returns `pattern` **unchanged** (object identity) instead of constructing a
/// copy. So `var r = /x/i; RegExp(r) === r` is `true`, and a property added to
/// `r` is visible through the returned reference (test262
/// `built-ins/RegExp/S15.10.3.1_A1_T*`, #5586).
///
/// Perry models no user-visible RegExp subclassing, so a registered RegExp's
/// `constructor` resolves through `RegExp.prototype` to the intrinsic `RegExp`
/// and the `SameValue` check holds — *unless* user code has installed an own
/// `constructor` property (e.g. `re.constructor = null`), which makes the
/// `SameValue` check fail and forces a fresh copy
/// (`built-ins/RegExp/call_with_regexp_not_same_constructor.js`). Every other
/// shape (string/object/undefined pattern, or any non-`undefined` flags —
/// which forces a fresh copy with the new flags) likewise falls through to the
/// general [`js_regexp_construct`] path.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_construct_call(pattern: f64, flags: f64) -> *mut RegExpHeader {
    perex_construct::construct(pattern, flags, true)
}

/// Test if a string matches the regex pattern
/// regex.test(string) -> boolean
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_test(re: *const RegExpHeader, s: *const StringHeader) -> i32 {
    // Test is generic: RegExpExec selects the method and requires the
    // private matcher only when it selects builtin exec. Do not read the
    // matcher here just to discard it before that same admission.
    if !is_valid_ptr(re) || !is_valid_ptr(s) {
        return 0;
    }
    if crate::hot_diag::regex_on() {
        diag_note_op(re, crate::hot_diag::RegexOp::Test);
    }
    i32::from(perex_api::finish(perex_dispatch::test_string(
        crate::value::js_nanbox_pointer(re as i64),
        s,
    )))
}

/// `PERRY_REGEX_DIAG`: attribute one exec-family operation to the receiver's
/// pattern. Callers have already validated `re`.
#[cfg(feature = "regex-engine")]
pub(super) fn diag_note_op(re: *const RegExpHeader, op: crate::hot_diag::RegexOp) {
    unsafe {
        let pattern_ptr = (*crate::regex::regexp_data_ptr(re)).pattern_ptr;
        let flags_ptr = (*crate::regex::regexp_data_ptr(re)).flags_ptr;
        let pattern = if is_valid_ptr(pattern_ptr) {
            string_as_bytes(pattern_ptr)
        } else {
            b""
        };
        let flags = if is_valid_ptr(flags_ptr) {
            string_as_str(flags_ptr)
        } else {
            ""
        };
        crate::hot_diag::regex_with(|d| d.note_op(pattern_ptr as usize, pattern, flags, op));
    }
}

/// Get the .index from the last exec() call
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_exec_get_index() -> f64 {
    LAST_EXEC_INDEX.with(|idx| *idx.borrow())
}

/// Get the .groups object from the last exec() call
/// Returns I64 pointer (0 for no groups)
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_exec_get_groups() -> i64 {
    LAST_EXEC_GROUPS.with(|g| {
        let ptr = *g.borrow();
        if ptr.is_null() {
            0
        } else {
            ptr as i64
        }
    })
}

/// GC root scanner for `LAST_EXEC_GROUPS`. The groups object built by
/// `js_regexp_exec` / `js_string_match` is stashed in this thread-local
/// for later `m.groups` reads — without scanning it as a root, a GC
/// firing between the match call and the property read can reclaim the
/// object, and subsequent reads dereference freed memory. Surfaced when
/// the `m.groups` fold was extended to cover `str.match(regex)` results
/// alongside `regex.exec(str)`: a sequence of match calls plus
/// allocations between them was enough to trigger nursery GC mid-test.
pub fn scan_last_exec_groups_root(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_last_exec_groups_root_mut(&mut visitor);
}

pub fn scan_last_exec_groups_root_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    LAST_EXEC_GROUPS.with(|g| {
        visitor.visit_raw_mut_ptr_slot(&mut g.borrow_mut());
    });
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_set_last_exec_groups(ptr: *mut ObjectHeader) {
    LAST_EXEC_GROUPS.with(|g| {
        *g.borrow_mut() = ptr;
    });
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_last_exec_groups() -> usize {
    LAST_EXEC_GROUPS.with(|g| *g.borrow() as usize)
}

/// Get regex.source — returns the pattern string
#[no_mangle]
pub extern "C" fn js_regexp_get_source(re: *const RegExpHeader) -> *mut StringHeader {
    #[cfg(feature = "regex-engine")]
    {
        return perex_api::finish(perex_display::source(re));
    }
    #[cfg(not(feature = "regex-engine"))]
    unsafe {
        let Some(data) = regexp_data_of(crate::value::js_nanbox_pointer(re as i64)) else {
            return js_string_from_str("(?:)");
        };
        if is_valid_ptr((*data).pattern_ptr) {
            // Return a copy of the pattern string
            let pattern_str = string_as_str((*data).pattern_ptr);
            // Escaping only inserts ASCII into text that came from a `&str`, so
            // the result is UTF-8 and the lossy conversion never substitutes.
            let escaped = escape_regexp_source(pattern_str.as_bytes());
            js_string_from_str(&String::from_utf8_lossy(&escaped))
        } else {
            js_string_from_str("(?:)")
        }
    }
}

/// `RegExp.prototype.source` for the prototype object itself (no
/// `[[OriginalSource]]`) returns the canonical empty source `"(?:)"`.
#[no_mangle]
pub extern "C" fn js_regexp_empty_source() -> *mut StringHeader {
    js_string_from_str("(?:)")
}

/// Get regex.flags — returns the flags string
#[no_mangle]
pub extern "C" fn js_regexp_get_flags(re: *const RegExpHeader) -> *mut StringHeader {
    #[cfg(feature = "regex-engine")]
    {
        return perex_api::finish(perex_match_search::flags(crate::value::js_nanbox_pointer(
            re as i64,
        )));
    }
    #[cfg(not(feature = "regex-engine"))]
    original_flags(re)
}

/// Internal serialization reads OriginalFlags, not the observable flags getter.
pub(crate) fn original_flags(re: *const RegExpHeader) -> *mut StringHeader {
    let Some(data) = crate::regex::regexp_data_of(crate::value::js_nanbox_pointer(re as i64))
    else {
        return js_string_from_str("");
    };
    unsafe {
        let flags = (*data).flags_ptr;
        if !is_valid_ptr(flags) {
            return js_string_from_str("");
        }
        crate::string::js_string_addref(flags as *mut StringHeader);
        flags as *mut StringHeader
    }
}

/// `RegExp.prototype.toString()` — `/source/flags`. Used by both the
/// `regex.toString()` method dispatch and ToString coercion (`String(re)`,
/// template literals). Node never produces `"[object Object]"` for a RegExp.
#[no_mangle]
pub extern "C" fn js_regexp_to_string(re: *const RegExpHeader) -> *mut StringHeader {
    #[cfg(feature = "regex-engine")]
    {
        return perex_api::finish(perex_display::to_string(re));
    }
    #[cfg(not(feature = "regex-engine"))]
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let re = scope.root_raw_const_ptr(re);
        let src = scope.root_string_ptr(re.with_const_ptr(|p| js_regexp_get_source(p)));
        let flg = scope.root_string_ptr(re.with_const_ptr(|p| js_regexp_get_flags(p)));
        // Copied into Rust storage before the next GC allocation.
        let out = src.with_const_ptr(|src| {
            flg.with_const_ptr(|flg| format!("/{}/{}", string_as_str(src), string_as_str(flg)))
        });
        js_string_from_str(&out)
    }
}

/// Get regex.lastIndex — returns the stored value (NaN-boxed JSValue bits as
/// f64). Usually a number, but `re.lastIndex = obj` round-trips the object.
#[cfg(feature = "regex-engine")]
pub(crate) fn get_last_index(re: *const RegExpHeader) -> f64 {
    LAST_INDEX_READ.with(|site| unsafe { site.read(re as *mut ObjectHeader, b"lastIndex") })
}

crate::perry_thread_local! {
    #[cfg(feature = "regex-engine")]
    static LAST_INDEX_READ: crate::object::field_get_set::runtime_read_site::RuntimeReadSite =
        const { crate::object::field_get_set::runtime_read_site::RuntimeReadSite::new() };
}

#[cfg(feature = "regex-engine")]
static LAST_INDEX_STORE: crate::object::field_get_set::runtime_store_site::RuntimeStoreSite =
    crate::object::field_get_set::runtime_store_site::RuntimeStoreSite::new();

#[cfg(feature = "regex-engine")]
pub(super) fn set_last_index_caught(receiver: f64, value: f64) -> Result<(), f64> {
    LAST_INDEX_STORE.store_caught(receiver, b"lastIndex", value)
}

/// Builtin spec Set(R, lastIndex, value, true), through the ordinary store site.
#[cfg(feature = "regex-engine")]
pub(crate) fn set_last_index(re: *mut RegExpHeader, value: f64) {
    set_last_index_value(crate::value::js_nanbox_pointer(re as i64), value);
}

#[cfg(feature = "regex-engine")]
pub(super) fn set_last_index_value(receiver: f64, value: f64) {
    LAST_INDEX_STORE.store(receiver, b"lastIndex", value);
}

#[cfg(all(test, feature = "regex-engine"))]
mod tests;
#[cfg(all(test, feature = "regex-engine"))]
mod tests_part2;

#[cfg(test)]
static REGEX_PTR_VALIDATION_CALLS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) fn test_regex_ptr_validation_calls() -> u64 {
    REGEX_PTR_VALIDATION_CALLS.load(std::sync::atomic::Ordering::Relaxed)
}
