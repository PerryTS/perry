//! #9364: heritage resolution when one class template is evaluated more than
//! once — the self-edge the VALUE stash must reject, and the per-evaluation
//! override that keeps a `super()` chain walking instead of looping.

use super::super::{js_get_dynamic_parent_value, js_register_class_parent_dynamic};
use super::{active_class_evaluation_parent, push_active_class_evaluation};

const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// The NaN-boxed ClassRef a shared-template `class X {}` evaluates to.
fn class_ref(class_id: u32) -> f64 {
    f64::from_bits(INT32_TAG | class_id as u64)
}

fn register(class_id: u32) {
    unsafe { crate::object::class_registry::js_register_class_id(class_id) };
}

#[test]
fn a_self_heritage_class_ref_does_not_displace_an_earlier_evaluations_parent() {
    let _lock = crate::gc::global_side_table_test_lock();
    const PARENT: u32 = 0x0936_4001;
    const CHILD: u32 = 0x0936_4002;
    register(PARENT);
    register(CHILD);

    // First evaluation of the shared template records a real heritage.
    js_register_class_parent_dynamic(CHILD, class_ref(PARENT));
    assert_eq!(
        js_get_dynamic_parent_value(CHILD).to_bits(),
        class_ref(PARENT).to_bits(),
        "fixture must start with a stashed heritage, or the verdict below is vacuous",
    );

    // A later evaluation extends the earlier one — which, sharing the template
    // id, IS `ClassRef(CHILD)`.
    js_register_class_parent_dynamic(CHILD, class_ref(CHILD));
    assert_eq!(
        js_get_dynamic_parent_value(CHILD).to_bits(),
        class_ref(PARENT).to_bits(),
        "a self-heritage ClassRef must not displace the recorded parent",
    );
}

#[test]
fn a_self_heritage_class_ref_is_not_recorded_at_all() {
    let _lock = crate::gc::global_side_table_test_lock();
    const SOLO: u32 = 0x0936_4003;
    register(SOLO);

    js_register_class_parent_dynamic(SOLO, class_ref(SOLO));
    assert_eq!(
        js_get_dynamic_parent_value(SOLO).to_bits(),
        TAG_UNDEFINED,
        "a class is never its own superclass, so nothing may be stashed",
    );
}

/// The guard reads the ClassRef PAYLOAD, not merely the INT32 tag.
#[test]
fn a_class_ref_to_a_different_class_is_still_recorded() {
    let _lock = crate::gc::global_side_table_test_lock();
    const PARENT: u32 = 0x0936_4004;
    const CHILD: u32 = 0x0936_4005;
    register(PARENT);
    register(CHILD);

    js_register_class_parent_dynamic(CHILD, class_ref(PARENT));
    assert_eq!(
        js_get_dynamic_parent_value(CHILD).to_bits(),
        class_ref(PARENT).to_bits(),
    );
    assert_eq!(
        crate::object::class_registry::get_parent_class_id(CHILD),
        Some(PARENT),
        "the registry edge is unaffected by the stash guard",
    );
}

/// A per-evaluation class object under replay answers with ITS OWN pinned
/// heritage, not the template's last-wins stash. This is what stops a chained
/// factory's `super()` from resolving to the same parent at every level.
#[test]
fn an_active_replay_resolves_this_evaluations_pinned_heritage() {
    let _lock = crate::gc::global_side_table_test_lock();
    const TEMPLATE: u32 = 0x0936_4006;
    const FIRST_PARENT: u32 = 0x0936_4007;
    const LAST_PARENT: u32 = 0x0936_4008;
    register(TEMPLATE);
    register(FIRST_PARENT);
    register(LAST_PARENT);

    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let class_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(TEMPLATE, 0));
        class_handle.with_mut_ptr::<crate::ObjectHeader, _>(|class| {
            crate::object::class_registry::js_object_mark_class(class as i64)
        });
        let class_value = class_handle.with_mut_ptr::<crate::ObjectHeader, _>(|class| {
            crate::value::js_nanbox_pointer(class as i64)
        });
        assert!(
            crate::object::class_registry::is_class_object_value(class_value),
            "fixture must be a per-evaluation class object, or the override cannot apply",
        );

        // This evaluation's heritage, pinned onto its own class object exactly
        // as codegen does right after `RegisterClassParentDynamic`.
        js_register_class_parent_dynamic(TEMPLATE, class_ref(FIRST_PARENT));
        class_handle.with_mut_ptr::<crate::ObjectHeader, _>(|class| {
            super::super::parent_static::js_class_object_pin_parent(class as i64, TEMPLATE)
        });

        // A LATER evaluation of the same template overwrites the shared stash.
        js_register_class_parent_dynamic(TEMPLATE, class_ref(LAST_PARENT));
        assert_eq!(
            js_get_dynamic_parent_value(TEMPLATE).to_bits(),
            class_ref(LAST_PARENT).to_bits(),
            "without an active replay the template stash is still last-wins",
        );

        {
            let _active =
                push_active_class_evaluation(class_handle.with_mut_ptr::<crate::ObjectHeader, _>(
                    |class| crate::value::js_nanbox_pointer(class as i64),
                ));
            assert_eq!(
                js_get_dynamic_parent_value(TEMPLATE).to_bits(),
                class_ref(FIRST_PARENT).to_bits(),
                "a replay must resolve the replaying evaluation's own heritage",
            );
        }

        assert_eq!(
            js_get_dynamic_parent_value(TEMPLATE).to_bits(),
            class_ref(LAST_PARENT).to_bits(),
            "the frame must pop, restoring the template stash for other callers",
        );
        assert!(
            active_class_evaluation_parent(TEMPLATE).is_none(),
            "no frame is active once the guard has dropped",
        );
    }
}

/// The override is scoped to the replaying class's OWN template id: an
/// unrelated class asking for its heritage during someone else's replay still
/// reads the stash.
#[test]
fn an_active_replay_does_not_answer_for_another_class_id() {
    let _lock = crate::gc::global_side_table_test_lock();
    const TEMPLATE: u32 = 0x0936_4009;
    const OTHER: u32 = 0x0936_400A;
    const OTHER_PARENT: u32 = 0x0936_400B;
    register(TEMPLATE);
    register(OTHER);
    register(OTHER_PARENT);

    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let class_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(TEMPLATE, 0));
        class_handle.with_mut_ptr::<crate::ObjectHeader, _>(|class| {
            crate::object::class_registry::js_object_mark_class(class as i64)
        });
        js_register_class_parent_dynamic(OTHER, class_ref(OTHER_PARENT));

        let _active =
            push_active_class_evaluation(class_handle.with_mut_ptr::<crate::ObjectHeader, _>(
                |class| crate::value::js_nanbox_pointer(class as i64),
            ));
        assert!(active_class_evaluation_parent(OTHER).is_none());
        assert_eq!(
            js_get_dynamic_parent_value(OTHER).to_bits(),
            class_ref(OTHER_PARENT).to_bits(),
        );
    }
}
