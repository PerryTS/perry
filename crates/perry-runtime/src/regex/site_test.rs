//! Allocation-free headers for regex literals whose only observable use at a
//! source site is the receiver of one `.test` call.
//!
//! A cached header is a strong mutable GC root.  It is never returned from a
//! transformed expression: direct literals feed it only to the paired test
//! dispatch, and a factory participates only while its structurally proven
//! body (`return /literal/`) is executing under a recorded caller site.  The
//! caller still invokes the factory on every evaluation, so reassignment and
//! member lookup retain their ordinary effects.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{is_valid_regex_ptr, js_regexp_new_impl, RegExpHeader};

struct Entry {
    header: *mut RegExpHeader,
    /// Zero for a direct literal; otherwise the compiler-emitted public
    /// function entry for the exact-return factory body.
    factory_identity: usize,
    /// Set by the construction/factory entry after this evaluation's
    /// canonicality check, consumed by the immediately following property Get.
    approved: bool,
}

#[derive(Clone, Copy)]
struct ActiveFactorySite {
    site_key: usize,
    /// Native entry resolved from the actual callee value before invocation.
    /// A literal in a nested helper must not consume its caller's site.
    expected_identity: usize,
    handled_by_literal: bool,
}

crate::perry_thread_local! {
    static SITE_TEST_HEADERS: RefCell<HashMap<usize, Entry>> = RefCell::new(HashMap::new());
    static ACTIVE_FACTORY_SITES: RefCell<Vec<ActiveFactorySite>> = RefCell::new(Vec::new());
}

const BUILTIN_TEST_MARKER: u64 = crate::value::TAG_MARKER;

#[inline]
fn note_no_alloc() {
    #[cfg(test)]
    TEST_NO_ALLOC.with(|counter| counter.set(counter.get() + 1));
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_counters(|d| d.site_test_no_alloc += 1);
    }
}

#[inline]
fn note_declined(reason: DeclineReason) {
    #[cfg(test)]
    {
        TEST_DECLINED.with(|counter| counter.set(counter.get() + 1));
        let bucket = match reason {
            DeclineReason::PatchedPrototype => &TEST_DECLINED_PATCHED,
            DeclineReason::CalleeMismatch => &TEST_DECLINED_CALLEE,
            DeclineReason::NonLiteral => &TEST_DECLINED_NON_LITERAL,
        };
        bucket.with(|counter| counter.set(counter.get() + 1));
    }
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_counters(|d| {
            d.site_test_declined += 1;
            match reason {
                DeclineReason::PatchedPrototype => d.site_test_declined_patched_prototype += 1,
                DeclineReason::CalleeMismatch => d.site_test_declined_callee_mismatch += 1,
                DeclineReason::NonLiteral => d.site_test_declined_non_literal += 1,
            }
        });
    }
}

#[cfg(test)]
thread_local! {
    static TEST_NO_ALLOC: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TEST_DECLINED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TEST_DECLINED_PATCHED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TEST_DECLINED_CALLEE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TEST_DECLINED_NON_LITERAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TEST_ALLOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy)]
enum DeclineReason {
    PatchedPrototype,
    CalleeMismatch,
    NonLiteral,
}

fn lookup(site_key: usize) -> Option<(*mut RegExpHeader, usize)> {
    SITE_TEST_HEADERS.with(|table| {
        table
            .borrow()
            .get(&site_key)
            .map(|entry| (entry.header, entry.factory_identity))
    })
}

fn install(site_key: usize, header: *mut RegExpHeader, factory_identity: usize) {
    if site_key == 0 || header.is_null() {
        return;
    }
    let mut entry = Entry {
        header: std::ptr::null_mut(),
        factory_identity,
        approved: true,
    };
    // GC_STORE_AUDIT(ROOT): `entry.header` becomes a mutable raw root when the
    // entry is inserted into SITE_TEST_HEADERS; `scan_roots_mut` visits it.
    // SAFETY: both pointers are either null or allocator-returned RegExp
    // addresses; the destination is the root slot that will own `header`.
    unsafe {
        crate::gc::runtime_store_root_raw_mut_ptr_slot(&mut entry.header, header);
    }
    SITE_TEST_HEADERS.with(|table| {
        table.borrow_mut().insert(site_key, entry);
    });
}

#[inline]
fn allocate(
    pattern: *const crate::StringHeader,
    flags: *const crate::StringHeader,
    site_key: usize,
) -> *mut RegExpHeader {
    #[cfg(test)]
    TEST_ALLOCATIONS.with(|counter| counter.set(counter.get() + 1));
    js_regexp_new_impl(pattern, flags, site_key)
}

fn approve(site_key: usize, header: *mut RegExpHeader) {
    SITE_TEST_HEADERS.with(|table| {
        if let Some(entry) = table.borrow_mut().get_mut(&site_key) {
            if entry.header == header {
                entry.approved = true;
            }
        }
    });
}

fn take_approval(site_key: usize, header: *mut RegExpHeader) -> bool {
    SITE_TEST_HEADERS.with(|table| {
        let mut table = table.borrow_mut();
        let Some(entry) = table.get_mut(&site_key) else {
            return false;
        };
        if entry.header != header || !entry.approved {
            return false;
        }
        entry.approved = false;
        true
    })
}

fn canonical_rooted_header(header: *mut RegExpHeader) -> Option<*mut RegExpHeader> {
    if !is_valid_regex_ptr(header) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let rooted = scope.root_raw_mut_ptr(header);
    let value = f64::from_bits(crate::value::JSValue::pointer(header.cast::<u8>()).bits());
    if !crate::object::regex_proto_thunks::regexp_prototype_test_is_canonical(value) {
        return None;
    }
    Some(rooted.get_raw_mut_ptr())
}

/// Direct literal receiver for `/literal/flags.test(arg)`.
#[no_mangle]
pub extern "C" fn js_regexp_site_test_new(
    pattern: *const crate::StringHeader,
    flags: *const crate::StringHeader,
    site_key: i64,
) -> *mut RegExpHeader {
    let site_key = site_key as usize;
    if let Some((header, factory_identity)) = lookup(site_key) {
        if factory_identity == 0 {
            if let Some(header) = canonical_rooted_header(header) {
                approve(site_key, header);
                // Price the construction this function avoided.  The cold
                // install is deliberately excluded.
                note_no_alloc();
                return header;
            }
            note_declined(DeclineReason::PatchedPrototype);
            return allocate(pattern, flags, site_key);
        }
    }

    let header = allocate(pattern, flags, site_key);
    if let Some(header) = canonical_rooted_header(header) {
        install(site_key, header, 0);
        header
    } else {
        note_declined(DeclineReason::PatchedPrototype);
        header
    }
}

fn mark_active_literal(factory_identity: usize) -> Option<usize> {
    ACTIVE_FACTORY_SITES.with(|stack| {
        let mut stack = stack.borrow_mut();
        let frame = stack.last_mut()?;
        if frame.expected_identity != factory_identity {
            return None;
        }
        frame.handled_by_literal = true;
        Some(frame.site_key)
    })
}

/// Literal construction inside a HIR-proven exact regex factory.  Outside a
/// transformed caller it is exactly `js_regexp_new_site`; inside one it uses
/// the caller site's `(site, function-entry)` record.
#[no_mangle]
pub extern "C" fn js_regexp_new_factory_site(
    pattern: *const crate::StringHeader,
    flags: *const crate::StringHeader,
    literal_site_key: i64,
    factory_identity: i64,
) -> *mut RegExpHeader {
    let factory_identity = factory_identity as usize;
    let Some(call_site_key) = mark_active_literal(factory_identity) else {
        return allocate(pattern, flags, literal_site_key as usize);
    };
    if let Some((header, recorded_identity)) = lookup(call_site_key) {
        if recorded_identity != factory_identity {
            note_declined(DeclineReason::CalleeMismatch);
            return allocate(pattern, flags, literal_site_key as usize);
        }
        if let Some(header) = canonical_rooted_header(header) {
            approve(call_site_key, header);
            // Price the construction this function avoided.  The cold
            // install is deliberately excluded.
            note_no_alloc();
            return header;
        }
        note_declined(DeclineReason::PatchedPrototype);
        return allocate(pattern, flags, literal_site_key as usize);
    }

    let header = allocate(pattern, flags, literal_site_key as usize);
    if let Some(header) = canonical_rooted_header(header) {
        install(call_site_key, header, factory_identity);
        header
    } else {
        note_declined(DeclineReason::PatchedPrototype);
        header
    }
}

struct ActiveFactoryGuard {
    depth_before: usize,
}

impl ActiveFactoryGuard {
    fn push(site_key: usize, expected_identity: usize) -> Self {
        let depth_before = ACTIVE_FACTORY_SITES.with(|stack| {
            let depth_before = stack.borrow().len();
            stack.borrow_mut().push(ActiveFactorySite {
                site_key,
                expected_identity,
                handled_by_literal: false,
            });
            depth_before
        });
        Self { depth_before }
    }

    fn handled(&self) -> bool {
        ACTIVE_FACTORY_SITES.with(|stack| {
            stack
                .borrow()
                .get(self.depth_before)
                .is_some_and(|frame| frame.handled_by_literal)
        })
    }
}

impl Drop for ActiveFactoryGuard {
    fn drop(&mut self) {
        ACTIVE_FACTORY_SITES.with(|stack| {
            let mut stack = stack.borrow_mut();
            // `js_throw` may already have restored the stack before a system
            // unwind runs this Drop.  Never pop a still-live outer frame.
            if stack.len() > self.depth_before {
                stack.truncate(self.depth_before);
            }
        });
    }
}

pub(crate) fn active_factory_stack_savepoint() -> usize {
    ACTIVE_FACTORY_SITES.with(|stack| stack.borrow().len())
}

pub(crate) fn active_factory_stack_restore(depth: usize) {
    ACTIVE_FACTORY_SITES.with(|stack| stack.borrow_mut().truncate(depth));
}

struct ImplicitThisGuard<'scope> {
    previous: crate::gc::RuntimeHandle<'scope>,
}

impl<'scope> ImplicitThisGuard<'scope> {
    fn bind(scope: &'scope crate::gc::RuntimeHandleScope, receiver: f64) -> Self {
        Self {
            previous: scope.root_nanbox_f64(crate::object::js_implicit_this_set(receiver)),
        }
    }
}

impl Drop for ImplicitThisGuard<'_> {
    fn drop(&mut self) {
        crate::object::js_implicit_this_set(self.previous.get_nanbox_f64());
    }
}

fn call_value_at_site(site_key: usize, callee: f64, this_value: Option<f64>) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callee = scope.root_nanbox_f64(callee);
    let callee_value = crate::value::JSValue::from_bits(callee.get_nanbox_f64().to_bits());
    let expected_identity = if callee_value.is_pointer() {
        let closure = callee_value.as_pointer::<crate::closure::ClosureHeader>();
        crate::closure::get_valid_func_ptr(closure) as usize
    } else {
        0
    };
    let this_value = this_value.map(|value| scope.root_nanbox_f64(value));
    let this_guard = this_value
        .as_ref()
        .map(|value| ImplicitThisGuard::bind(&scope, value.get_nanbox_f64()));
    let active = ActiveFactoryGuard::push(site_key, expected_identity);
    let result = unsafe {
        crate::closure::js_native_call_value(callee.get_nanbox_f64(), std::ptr::null(), 0)
    };
    if !active.handled() {
        let reason = if lookup(site_key).is_some() {
            DeclineReason::CalleeMismatch
        } else {
            DeclineReason::NonLiteral
        };
        note_declined(reason);
    }
    drop(active);
    drop(this_guard);
    result
}

#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_regexp_site_factory_call_value(site_key: i64, callee: f64) -> f64 {
    call_value_at_site(site_key as usize, callee, None)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_regexp_site_factory_call_value(site_key: i64, callee: f64) -> f64 {
    call_value_at_site(site_key as usize, callee, None)
}

#[cfg(panic = "abort")]
#[no_mangle]
pub unsafe extern "C" fn js_regexp_site_factory_call_method(
    site_key: i64,
    receiver: f64,
    method_key: f64,
) -> f64 {
    unsafe { site_factory_call_method_impl(site_key, receiver, method_key) }
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub unsafe extern "C-unwind" fn js_regexp_site_factory_call_method(
    site_key: i64,
    receiver: f64,
    method_key: f64,
) -> f64 {
    unsafe { site_factory_call_method_impl(site_key, receiver, method_key) }
}

#[inline(always)]
unsafe fn site_factory_call_method_impl(site_key: i64, receiver: f64, method_key: f64) -> f64 {
    // Resolve the member before activating the factory site.  An accessor is
    // arbitrary user code and must not let an incidental regex construction
    // masquerade as the function the call actually selected.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let method_key = scope.root_nanbox_f64(method_key);
    let method =
        crate::value::js_dyn_index_get(receiver.get_nanbox_f64(), method_key.get_nanbox_f64());
    call_value_at_site(site_key as usize, method, Some(receiver.get_nanbox_f64()))
}

/// Resolve `.test` at the spec-mandated point (before argument evaluation).
/// The internal marker means the builtin was validated for the cached header;
/// every generic decline returns the actual property value instead.
#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_regexp_site_test_get_method(site_key: i64, receiver: f64) -> f64 {
    site_test_get_method_impl(site_key, receiver)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_regexp_site_test_get_method(site_key: i64, receiver: f64) -> f64 {
    site_test_get_method_impl(site_key, receiver)
}

#[inline(always)]
fn site_test_get_method_impl(site_key: i64, receiver: f64) -> f64 {
    let site_key = site_key as usize;
    let receiver_value = crate::value::JSValue::from_bits(receiver.to_bits());
    let receiver_ptr = receiver_value
        .is_pointer()
        .then(|| receiver_value.as_pointer::<RegExpHeader>() as *mut RegExpHeader);
    if let (Some(receiver_ptr), Some((header, _))) = (receiver_ptr, lookup(site_key)) {
        if receiver_ptr == header {
            if take_approval(site_key, header) {
                return f64::from_bits(BUILTIN_TEST_MARKER);
            }
            note_declined(DeclineReason::PatchedPrototype);
        }
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = crate::string::intern_ascii_literal(b"test");
    crate::value::js_dyn_index_get(
        receiver.get_nanbox_f64(),
        crate::js_nanbox_string(key as i64),
    )
}

/// Consume the method captured above and the now-evaluated argument.
#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_regexp_site_test_dispatch(
    _site_key: i64,
    receiver: f64,
    method: f64,
    argument: f64,
) -> f64 {
    site_test_dispatch_impl(receiver, method, argument)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_regexp_site_test_dispatch(
    _site_key: i64,
    receiver: f64,
    method: f64,
    argument: f64,
) -> f64 {
    site_test_dispatch_impl(receiver, method, argument)
}

#[inline(always)]
fn site_test_dispatch_impl(receiver: f64, method: f64, argument: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let argument = scope.root_nanbox_f64(argument);
    if method.to_bits() == BUILTIN_TEST_MARKER {
        let string = crate::value::js_jsvalue_to_string_coerce(argument.get_nanbox_f64());
        let receiver = receiver.get_nanbox_f64();
        let value = crate::value::JSValue::from_bits(receiver.to_bits());
        if !value.is_pointer() {
            return f64::from_bits(crate::value::TAG_FALSE);
        }
        let header = value.as_pointer::<RegExpHeader>() as *mut RegExpHeader;
        if !is_valid_regex_ptr(header) {
            return f64::from_bits(crate::value::TAG_FALSE);
        }
        // A fresh literal begins every evaluation at zero.  `test` may write
        // the cached header's lastIndex, but no reference to this header leaves
        // the transformed expression and the next call resets it again.
        unsafe {
            (*header).last_index = crate::value::JSValue::number(0.0).bits();
        }
        return f64::from_bits(
            crate::value::JSValue::bool(super::js_regexp_test(header, string) != 0).bits(),
        );
    }

    let method = scope.root_nanbox_f64(method);
    let _this_guard = ImplicitThisGuard::bind(&scope, receiver.get_nanbox_f64());
    let args = [argument.get_nanbox_f64()];
    unsafe { crate::closure::js_native_call_value(method.get_nanbox_f64(), args.as_ptr(), 1) }
}

/// Strong root for every cached header.  The visitor rewrites entries in
/// place during evacuation, so later probes never retain a from-space address.
pub(crate) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SITE_TEST_HEADERS.with(|table| {
        for entry in table.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut entry.header);
        }
    });
}

pub(super) fn census() -> crate::gc::census::SideTableRow {
    SITE_TEST_HEADERS.with(|table| {
        let table = table.borrow();
        (
            "regex.site_test_headers",
            table.len(),
            crate::gc::census::map_bytes(&*table),
        )
    })
}

pub(crate) fn side_table_census() -> Vec<crate::gc::census::SideTableRow> {
    vec![
        super::site_cache::census(),
        super::site_key::census(),
        census(),
    ]
}

#[cfg(test)]
pub(super) fn test_reset() {
    SITE_TEST_HEADERS.with(|table| table.borrow_mut().clear());
    ACTIVE_FACTORY_SITES.with(|stack| stack.borrow_mut().clear());
    TEST_NO_ALLOC.with(|counter| counter.set(0));
    TEST_DECLINED.with(|counter| counter.set(0));
    TEST_DECLINED_PATCHED.with(|counter| counter.set(0));
    TEST_DECLINED_CALLEE.with(|counter| counter.set(0));
    TEST_DECLINED_NON_LITERAL.with(|counter| counter.set(0));
    TEST_ALLOCATIONS.with(|counter| counter.set(0));
}

#[cfg(test)]
pub(super) fn test_header(site_key: usize) -> Option<usize> {
    lookup(site_key).map(|(header, _)| header as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    static DIRECT_G: u64 = 1;
    static DIRECT_Y: u64 = 2;
    static FACTORY_CALL: u64 = 3;
    static MEMBER_CALL: u64 = 4;
    static NESTED_FACTORY_CALL: u64 = 5;
    static NESTED_WRAPPER_CALLS: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    fn string(text: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
    }

    fn key(slot: &'static u64) -> i64 {
        slot as *const u64 as i64
    }

    fn run_direct(site: i64, flags: &str, input: &str) -> (usize, bool) {
        let receiver = js_regexp_site_test_new(string("x"), string(flags), site);
        let receiver_value = crate::value::js_nanbox_pointer(receiver as i64);
        let method = js_regexp_site_test_get_method(site, receiver_value);
        let result = js_regexp_site_test_dispatch(
            site,
            receiver_value,
            method,
            crate::js_nanbox_string(string(input) as i64),
        );
        (receiver as usize, crate::value::js_is_truthy(result) != 0)
    }

    fn run_fresh_generic(flags: &str, input: &str) -> bool {
        let receiver = super::super::js_regexp_new(string("x"), string(flags));
        super::super::js_regexp_test(receiver, string(input)) != 0
    }

    fn ensure_regexp_builtins() {
        let value = crate::object::builtin_prototype_value("RegExp");
        assert!(
            crate::value::JSValue::from_bits(value.to_bits()).is_pointer(),
            "the RegExp intrinsic must be installed before testing its recorded test site"
        );
    }

    #[test]
    fn direct_global_site_allocates_one_header_and_resets_last_index() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset();
        ensure_regexp_builtins();
        let site = key(&DIRECT_G);
        let inputs = ["x", "x", "a", "xx"];
        let mut header = None;
        for input in inputs {
            let want = run_fresh_generic("g", input);
            let (current, got) = run_direct(site, "g", input);
            assert_eq!(got, want, "site result must equal a fresh /x/g.test(input)");
            assert_eq!(*header.get_or_insert(current), current);
        }
        assert_eq!(test_header(site as usize), header);
        assert_eq!(
            TEST_NO_ALLOC.with(std::cell::Cell::get),
            inputs.len() as u64 - 1
        );
        assert_eq!(
            TEST_ALLOCATIONS.with(std::cell::Cell::get),
            1,
            "the site must allocate exactly its one rooted header"
        );
    }

    #[test]
    fn direct_sticky_site_starts_each_evaluation_at_zero() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset();
        ensure_regexp_builtins();
        let site = key(&DIRECT_Y);
        let first = run_direct(site, "y", "x");
        let second = run_direct(site, "y", "x");
        let offset_only = run_direct(site, "y", "ax");
        assert_eq!(first.0, second.0, "one rooted header serves the site");
        assert!(
            first.1 && second.1,
            "lastIndex must reset before the second test"
        );
        assert!(!offset_only.1, "fresh /x/y is anchored at index zero");
        assert_eq!(offset_only.1, run_fresh_generic("y", "ax"));
        assert_eq!(TEST_ALLOCATIONS.with(std::cell::Cell::get), 1);
    }

    #[test]
    fn escaping_generic_global_header_carries_last_index_between_tests() {
        let _lock = crate::gc::global_side_table_test_lock();
        let re = super::super::js_regexp_new(string("x"), string("g"));
        let subject = string("xx");
        assert_ne!(super::super::js_regexp_test(re, subject), 0);
        assert_eq!(
            unsafe { (*re).last_index },
            crate::value::JSValue::number(1.0).bits()
        );
        assert_ne!(
            super::super::js_regexp_test(re, subject),
            0,
            "the second test must start at the first test's lastIndex, not at zero"
        );
        assert_eq!(
            unsafe { (*re).last_index },
            crate::value::JSValue::number(2.0).bits()
        );
    }

    extern "C" fn exact_factory(_closure: *const crate::closure::ClosureHeader) -> f64 {
        let re = js_regexp_new_factory_site(
            string("x"),
            string("g"),
            key(&DIRECT_G),
            exact_factory as *const u8 as i64,
        );
        crate::value::js_nanbox_pointer(re as i64)
    }

    extern "C" fn replacement_factory(_closure: *const crate::closure::ClosureHeader) -> f64 {
        let object = crate::object::js_object_alloc(0, 0);
        crate::object::js_object_set_field_by_name(
            object,
            string("test"),
            closure_with_arity(patched_test as *const u8, 1),
        );
        crate::value::js_nanbox_pointer(object as i64)
    }

    #[inline(never)]
    extern "C" fn nested_factory_wrapper(_closure: *const crate::closure::ClosureHeader) -> f64 {
        // Keep this observably distinct from `exact_factory` under release
        // function merging while modeling a non-literal wrapper with effects.
        NESTED_WRAPPER_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        exact_factory(std::ptr::null())
    }

    fn closure(function: *const u8) -> f64 {
        closure_with_arity(function, 0)
    }

    fn closure_with_arity(function: *const u8, arity: usize) -> f64 {
        crate::closure::js_register_closure_arity(function, arity as u32);
        crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(function, 0) as i64)
    }

    fn dispatch_test(site: i64, receiver: f64, input: &str) -> f64 {
        let method = js_regexp_site_test_get_method(site, receiver);
        js_regexp_site_test_dispatch(
            site,
            receiver,
            method,
            crate::js_nanbox_string(string(input) as i64),
        )
    }

    #[test]
    fn direct_factory_site_reuses_only_the_recorded_callee() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset();
        ensure_regexp_builtins();
        let site = key(&FACTORY_CALL);
        let callee = closure(exact_factory as *const u8);
        let first = js_regexp_site_factory_call_value(site, callee);
        let second = js_regexp_site_factory_call_value(site, callee);
        assert_eq!(
            first.to_bits(),
            second.to_bits(),
            "the exact factory reuses its header"
        );
        assert_eq!(TEST_NO_ALLOC.with(std::cell::Cell::get), 1);

        let replacement = closure(replacement_factory as *const u8);
        let receiver = js_regexp_site_factory_call_value(site, replacement);
        let result = dispatch_test(site, receiver, "does not contain the pattern");
        assert_eq!(result.to_bits(), crate::value::TAG_TRUE);
        assert_eq!(
            TEST_DECLINED.with(std::cell::Cell::get),
            1,
            "the very next call must record the callee mismatch"
        );
        assert_eq!(TEST_DECLINED_CALLEE.with(std::cell::Cell::get), 1);
    }

    #[test]
    fn nested_exact_factory_cannot_claim_a_different_callees_site() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset();
        ensure_regexp_builtins();
        NESTED_WRAPPER_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let site = key(&NESTED_FACTORY_CALL);
        let wrapper = closure(nested_factory_wrapper as *const u8);
        let scope = crate::gc::RuntimeHandleScope::new();
        let first = scope.root_nanbox_f64(js_regexp_site_factory_call_value(site, wrapper));
        let second = js_regexp_site_factory_call_value(site, wrapper);
        assert_ne!(
            first.get_nanbox_f64().to_bits(),
            second.to_bits(),
            "a nested literal must keep fresh-object semantics"
        );
        assert_eq!(test_header(site as usize), None);
        assert_eq!(TEST_ALLOCATIONS.with(std::cell::Cell::get), 2);
        assert_eq!(TEST_DECLINED.with(std::cell::Cell::get), 2);
        assert_eq!(TEST_DECLINED_NON_LITERAL.with(std::cell::Cell::get), 2);
        assert_eq!(
            NESTED_WRAPPER_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
    }

    #[test]
    fn caught_throw_restores_an_orphaned_factory_site_frame() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset();
        let base = active_factory_stack_savepoint();
        let _jump_buffer = crate::exception::js_try_push();
        let active = ActiveFactoryGuard::push(key(&FACTORY_CALL) as usize, usize::MAX);
        assert_eq!(active_factory_stack_savepoint(), base + 1);

        // A raw throw skips this Drop. Model that transport, then replay the
        // exception path's recorded savepoint restoration.
        std::mem::forget(active);
        crate::exception::test_unwind_innermost_shadow_restore();
        crate::exception::js_try_end();
        assert_eq!(active_factory_stack_savepoint(), base);
    }

    #[test]
    fn namespace_member_factory_site_is_covered() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset();
        ensure_regexp_builtins();
        let site = key(&MEMBER_CALL);
        let namespace = crate::object::js_object_alloc(0, 0);
        let name = string("default");
        crate::object::js_object_set_field_by_name(
            namespace,
            name,
            closure(exact_factory as *const u8),
        );
        let namespace_object = namespace;
        let namespace = crate::value::js_nanbox_pointer(namespace_object as i64);
        let member = crate::js_nanbox_string(name as i64);
        let first = unsafe { js_regexp_site_factory_call_method(site, namespace, member) };
        let second = unsafe { js_regexp_site_factory_call_method(site, namespace, member) };
        assert_eq!(first.to_bits(), second.to_bits());
        assert_eq!(TEST_NO_ALLOC.with(std::cell::Cell::get), 1);
        assert_eq!(
            test_header(site as usize),
            Some(crate::value::js_nanbox_get_pointer(first) as usize)
        );

        crate::object::js_object_set_field_by_name(
            namespace_object,
            name,
            closure(replacement_factory as *const u8),
        );
        let replacement = unsafe { js_regexp_site_factory_call_method(site, namespace, member) };
        assert_eq!(
            dispatch_test(site, replacement, "no match").to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(
            TEST_DECLINED.with(std::cell::Cell::get),
            1,
            "rebinding the namespace member must decline on the next call"
        );
        assert_eq!(TEST_DECLINED_CALLEE.with(std::cell::Cell::get), 1);
    }

    extern "C" fn patched_test(_closure: *const crate::closure::ClosureHeader, _arg: f64) -> f64 {
        f64::from_bits(crate::value::TAG_TRUE)
    }

    #[test]
    fn patched_regexp_prototype_test_declines_on_the_next_call() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset();
        ensure_regexp_builtins();
        let site = key(&DIRECT_G);
        let (warm_header, _) = run_direct(site, "g", "x");

        let proto_value = crate::object::builtin_prototype_value("RegExp");
        let proto = crate::value::JSValue::from_bits(proto_value.to_bits())
            .as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader;
        let test_key = string("test");
        let original = crate::object::js_object_get_field_by_name(proto, test_key);
        crate::object::js_object_set_field_by_name(
            proto,
            test_key,
            closure_with_arity(patched_test as *const u8, 1),
        );

        let receiver = js_regexp_site_test_new(string("x"), string("g"), site);
        assert_ne!(
            receiver as usize, warm_header,
            "patched prototype forces a fresh header"
        );
        let receiver_value = crate::value::js_nanbox_pointer(receiver as i64);
        let method = js_regexp_site_test_get_method(site, receiver_value);
        let result = js_regexp_site_test_dispatch(
            site,
            receiver_value,
            method,
            crate::js_nanbox_string(string("no match") as i64),
        );
        assert_eq!(result.to_bits(), crate::value::TAG_TRUE);
        assert_eq!(TEST_DECLINED.with(std::cell::Cell::get), 1);
        assert_eq!(TEST_DECLINED_PATCHED.with(std::cell::Cell::get), 1);

        crate::object::js_object_set_field_by_name(
            proto,
            test_key,
            f64::from_bits(original.bits()),
        );
    }

    #[test]
    fn site_header_root_is_rewritten_by_a_copying_minor() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        test_reset();
        crate::gc::gc_register_mutable_root_scanner(scan_roots_mut);

        let site = key(&DIRECT_G) as usize;
        let header = super::super::test_alloc_nursery_regexp_for_move("site-root", "g");
        let old = header as usize;
        assert!(crate::arena::pointer_in_nursery(old));
        install(site, header, 0);

        let _ = crate::gc::gc_collect_minor();
        let moved = test_header(site).expect("the site header remains rooted");
        assert_ne!(moved, old, "the scanner must rewrite the cached address");
        assert!(super::super::regex_header_has_magic(
            moved as *const RegExpHeader
        ));
        test_reset();
    }
}
