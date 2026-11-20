//! Wasm-native transport and frame-owned managed-state checkpoints.
use perry_abi::WasmEhCheckpoint;

/// Save into the caller's activation. Depths contain no managed pointers.
#[no_mangle]
pub unsafe extern "C" fn js_wasm_eh_checkpoint_save(out: *mut WasmEhCheckpoint) {
    let shadow = crate::gc::frame_root_savepoint();
    let stacks = super::marks::catch_stack_depths();
    // GC_STORE_AUDIT(STACK): caller-owned checkpoint of stack depths, not managed pointers.
    out.write(WasmEhCheckpoint {
        handles: crate::gc::runtime_handle_stack_len() as u64,
        shadow_frame_top: shadow.frame_top as u64,
        shadow_len: shadow.len as u64,
        async_context: stacks.context_guards as u64,
        temp_roots: crate::gc::temp_root_len() as u64,
        call_method: stacks.call_method as u64,
        pump: stacks.pump as u64,
        set_foreach: stacks.set_foreach as u64,
        map_foreach: stacks.map_foreach as u64,
        prototype_resolution: stacks.prototype_resolution as u64,
        static_private_owner: stacks.static_private_owner as u64,
        private_lexical_brand: stacks.private_lexical_brand as u64,
        derived_super_binding: stacks.derived_super_binding as u64,
        private_member_access_hints: stacks.private_member_access_hints as u64,
        namespace_override: stacks.namespace_override as u64,
    });
}

fn depth(value: u64) -> usize {
    usize::try_from(value).expect("invalid wasm EH checkpoint depth")
}

/// Restore all managed stacks before any catch binding, allocation, or JS.
/// Each subsystem performs the same effects as its native unwind operation.
#[no_mangle]
pub unsafe extern "C" fn js_wasm_eh_pad_enter(saved: *const WasmEhCheckpoint) {
    let s = &*saved;
    crate::closure::reset_throw_not_callable_counter();
    crate::gc::runtime_handle_stack_restore_to(depth(s.handles));
    #[cfg(test)]
    if !crate::native_payload::callback_sabotage("wasm_checkpoint_async_context") {
        crate::async_context::restore_context_guard_depth(depth(s.async_context));
    }
    #[cfg(not(test))]
    crate::async_context::restore_context_guard_depth(depth(s.async_context));
    crate::gc::temp_roots_truncate_to(depth(s.temp_roots));
    crate::object::call_method_depth_restore_depth(depth(s.call_method));
    crate::stdlib_pump::pump_depth_restore_depth(depth(s.pump));
    crate::set::set_foreach_restore_depth(depth(s.set_foreach));
    crate::map::map_foreach_restore_depth(depth(s.map_foreach));
    crate::object::prototype_chain::resolution_stack_restore_depth(depth(s.prototype_resolution));
    crate::object::static_private_owner_restore_depth(depth(s.static_private_owner));
    crate::object::private_lexical_brand_restore_depth(depth(s.private_lexical_brand));
    crate::object::derived_super_binding_restore_depth(depth(s.derived_super_binding));
    crate::object::private_member_access_hints_restore_depth(depth(s.private_member_access_hints));
    crate::object::namespace_override_restore_depth(depth(s.namespace_override));
    let now = crate::gc::frame_root_savepoint();
    debug_assert!(
        depth(s.shadow_len) <= now.len,
        "invalid wasm EH shadow prefix"
    );
    crate::gc::frame_root_restore(crate::gc::FrameRootSavepoint {
        frame_top: depth(s.shadow_frame_top),
        len: depth(s.shadow_len),
    });
}

/// Tag zero is shared with the standard C++ ABI. Only our header is accepted;
/// a foreign header is forwarded without touching the pending JS payload.
#[no_mangle]
pub unsafe extern "C" fn js_wasm_eh_is_perry(object: *const crate::eh::UnwindException) -> bool {
    !object.is_null() && (*object).class == crate::eh::PERRY_EXCEPTION_CLASS
}

/// Command entry's lexical outer boundary has already restored its checkpoint.
#[no_mangle]
pub extern "C" fn js_wasm_eh_exit_uncaught() -> ! {
    super::exit_uncaught(super::js_get_exception())
}
