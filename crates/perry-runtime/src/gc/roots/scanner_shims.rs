use super::*;

pub fn promise_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::promise::scan_promise_roots(mark);
}

pub fn promise_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::promise::scan_promise_roots_mut(visitor);
}

/// Root scanner for timer callbacks
pub fn timer_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::timer::scan_timer_roots(mark);
}

pub fn timer_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::timer::scan_timer_roots_mut(visitor);
}

/// Root scanner for current exception
pub fn exception_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::exception::scan_exception_roots(mark);
}

pub fn exception_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::exception::scan_exception_roots_mut(visitor);
}

/// Root scanner for active AsyncLocalStorage context.
pub fn async_context_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::async_context::scan_active_context_roots(mark);
    crate::builtins::scan_queued_microtask_roots(mark);
}

pub fn async_context_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::async_context::scan_active_context_roots_mut(visitor);
    crate::builtins::scan_queued_microtask_roots_mut(visitor);
}

/// Root scanner for async_hooks hook callbacks and user resource references.
pub fn async_hooks_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::async_hooks::scan_async_hooks_roots(mark);
}

pub fn async_hooks_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::async_hooks::scan_async_hooks_roots_mut(visitor);
}

/// Root scanner for object shape cache (keys arrays shared across objects with same shape)
pub fn shape_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_shape_cache_roots(mark);
}

pub fn shape_cache_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::object::scan_shape_cache_roots_mut(visitor);
}

/// Root scanner for the shape-transition cache used by the dynamic-key
/// write path (`obj[name] = value`). Same role as `shape_cache_root_scanner`
/// — without it, GC would free cached target keys_arrays that no live
/// object currently references directly.
pub fn transition_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_transition_cache_roots(mark);
}

pub fn transition_cache_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::object::scan_transition_cache_roots_mut(visitor);
}

/// Legacy scanner shim for OVERFLOW_FIELDS. Overflow fields are object-owned
/// external slots traced through GC_TYPE_OBJECT.
pub fn overflow_fields_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::object::scan_overflow_fields_roots(mark);
}

pub fn overflow_fields_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::object::scan_overflow_fields_roots_mut(visitor);
}

/// Root scanner for in-progress JSON.parse frames (issue #46).
/// Without this, GC triggered mid-parse would sweep in-progress arrays/objects
/// and the fresh string/object values about to be pushed into them.
pub fn json_parse_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::json::scan_parse_roots(mark);
}

pub fn json_parse_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::json::scan_parse_roots_mut(visitor);
}

pub fn shadow_stack_root_scanner(mark: &mut dyn FnMut(f64)) {
    visit_shadow_stack_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits != 0 {
            mark(f64::from_bits(bits));
        }
    });
}

/// Initialize GC root scanners. Called once at runtime startup.

pub fn intern_table_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::string::scan_intern_table_roots(mark);
}

pub fn intern_table_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::string::scan_intern_table_roots_mut(visitor);
}

pub fn small_int_cache_root_scanner(mark: &mut dyn FnMut(f64)) {
    crate::string::scan_small_int_cache_roots(mark);
}

pub fn small_int_cache_mutable_root_scanner(visitor: &mut RuntimeRootVisitor<'_>) {
    crate::string::scan_small_int_cache_roots_mut(visitor);
}
