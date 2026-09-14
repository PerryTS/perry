//! One declaration per catch savepoint: storage, capture, restore and a real
//! throw witness are generated together. The order here is also restore order.
//!
//! Capture providers are inlined so their HotKey accesses share the existing
//! hot TLS cache. No live subsystem state is mirrored or journaled, and the
//! cold restore path continues to use each subsystem's own cleanup rules.

use crate::gc::ShadowSavepoint;

macro_rules! catch_savepoints {
    ($($(#[$attr:meta])* $name:ident: $ty:ty,
        capture: $capture:path, restore: $restore:path;)*) => {
        #[derive(Clone, Copy)]
        pub(super) struct CatchSavepoint {
            $($(#[$attr])* $name: $ty,)*
        }

        impl CatchSavepoint {
            #[inline(always)]
            pub(super) fn capture() -> Self {
                Self { $($(#[$attr])* $name: $capture(),)* }
            }

            pub(super) fn restore(self) {
                $($(#[$attr])* $restore(self.$name);)*
            }
        }

        #[cfg(test)]
        mod restore_tests {
            use super::tests;
            $($(#[$attr])*
                #[test]
                fn $name() {
                    tests::assert_nested_restore($capture, $restore, tests::$name);
                }
            )*
        }
    };
}

catch_savepoints! {
    // #1830, #6951: both shadow frames and expression temp roots.
    shadow: ShadowSavepoint,
    capture: crate::gc::shadow_stack_savepoint,
    restore: crate::gc::shadow_stack_restore;
    // Longjmp skips RuntimeHandleScope drops.
    runtime_handles: usize,
    capture: crate::gc::runtime_handle_stack_savepoint,
    restore: crate::gc::runtime_handle_stack_restore;
    // #5591: skipped method guards must not wedge the recursion counter.
    call_method: u32,
    capture: crate::object::call_method_depth_savepoint,
    restore: crate::object::call_method_depth_restore;
    // Includes removal of the process-wide outer-pump contribution.
    pump: u32,
    capture: crate::stdlib_pump::pump_depth_savepoint,
    restore: crate::stdlib_pump::pump_depth_restore;
    // #9082: abandoned walks must no longer inhibit compaction.
    set_foreach: usize,
    capture: crate::set::set_foreach_stack_savepoint,
    restore: crate::set::set_foreach_stack_restore;
    map_foreach: usize,
    capture: crate::map::map_foreach_stack_savepoint,
    restore: crate::map::map_foreach_stack_restore;
    prototype_resolution: usize,
    capture: crate::object::prototype_chain::resolution_stack_savepoint,
    restore: crate::object::prototype_chain::resolution_stack_restore;
    static_private_owner: usize,
    capture: crate::object::static_private_owner_stack_savepoint,
    restore: crate::object::static_private_owner_stack_restore;
    private_lexical_brand: usize,
    capture: crate::object::private_lexical_brand_stack_savepoint,
    restore: crate::object::private_lexical_brand_stack_restore;
    derived_super_binding: usize,
    capture: crate::object::derived_super_binding_stack_savepoint,
    restore: crate::object::derived_super_binding_stack_restore;
    private_member_access_hints: usize,
    capture: crate::object::private_member_access_hints_savepoint,
    restore: crate::object::private_member_access_hints_restore;
    #[cfg(feature = "regex-engine")]
    regex_factory: usize,
    capture: crate::regex::site_test::active_factory_stack_savepoint,
    restore: crate::regex::site_test::active_factory_stack_restore;
    // #6559: rooted interpreter values AND the packed call depth.
    #[cfg(feature = "dyn-eval")]
    dyn_eval: u64,
    capture: crate::dyn_eval::interp_savepoint,
    restore: crate::dyn_eval::interp_restore;
}

#[cfg(test)]
mod tests;
