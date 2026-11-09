use super::*;
use objc2_app_kit::NSApplication;
use objc2_foundation::MainThreadMarker;
use perry_runtime as _;
use std::cell::Cell;

thread_local! {
    pub(super) static REGISTRY_LOOKUPS: Cell<usize> = const { Cell::new(0) };
}

#[test]
fn stack_registry_lookup_growth_is_bounded() {
    let mtm = MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    for size in [16, 32, 64] {
        let parent = vstack::create(0.0);
        let children: Vec<_> = (0..size).map(|_| spacer::create()).collect();
        let registered = WIDGETS.with(|widgets| widgets.borrow().len());
        REGISTRY_LOOKUPS.with(|count| count.set(0));
        for (index, &child) in children.iter().enumerate() {
            // The next render callback sees every previously published child.
            PARENT_MAP.with(|parents| {
                let parents = parents.borrow();
                for (position, previous) in children[..index].iter().enumerate() {
                    assert_eq!(parents.get(previous), Some(&(parent, position)));
                }
            });
            add_child(parent, child);
            PARENT_MAP.with(|parents| {
                assert_eq!(parents.borrow().get(&child), Some(&(parent, index)));
            });
        }
        let lookups = REGISTRY_LOOKUPS.with(Cell::get);
        println!("siblings={size} registered={registered} registry_visits={lookups}");
        assert!(
            lookups <= size * registered,
            "a refresh must inspect each registered widget at most once, not once per sibling"
        );
        clear_children(parent);
    }
}

fn stack(handle: i64) -> Retained<NSStackView> {
    let view = get_widget(handle).unwrap();
    assert!(view.isKindOfClass(AnyClass::get(c"NSStackView").unwrap()));
    unsafe { Retained::cast_unchecked(view) }
}

fn assert_children(parent: i64, expected: &[i64]) {
    let actual: Vec<_> = stack(parent)
        .viewsInGravity(NSStackViewGravity::Top)
        .iter()
        .map(|view| Retained::as_ptr(&view))
        .collect();
    let expected_pointers: Vec<_> = expected
        .iter()
        .map(|&handle| Retained::as_ptr(&get_layout_widget(handle).unwrap()))
        .collect();
    assert_eq!(actual, expected_pointers);
    PARENT_MAP.with(|parents| {
        let parents = parents.borrow();
        for (index, handle) in expected.iter().enumerate() {
            assert_eq!(parents.get(handle), Some(&(parent, index)));
        }
    });
}

#[test]
fn stack_registry_preserves_moves_hidden_positions_and_cleanup() {
    let mtm = MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let parent = vstack::create(0.0);
    let other = hstack::create(0.0);
    let a = spacer::create();
    let b = spacer::create();
    let c = spacer::create();
    add_child(parent, a);
    add_child(parent, b);
    add_child_at(parent, c, 1);
    assert_children(parent, &[a, c, b]);
    add_child_at(parent, a, 2);
    assert_children(parent, &[c, b, a]);
    set_width(b, 80.0);
    add_child_at(other, b, 0);
    assert_children(parent, &[c, a]);
    assert_children(other, &[b]);
    WIDTH_CONSTRAINTS.with(|constraints| {
        let constraints = constraints.borrow();
        let constraint = constraints.get(&b).unwrap();
        let active: bool = unsafe { msg_send![&**constraint, isActive] };
        assert!(
            active,
            "moving a child must not dispose its layout metadata"
        );
    });
    add_child_at(parent, b, -1);
    assert_children(parent, &[b, c, a]);
    assert_children(other, &[]);
    reorder_child(parent, 0, 2);
    assert_children(parent, &[c, a, b]);

    // AppKit can detach a hidden view while Perry retains its insertion position.
    let a_view = get_layout_widget(a).unwrap();
    set_hidden(a, true);
    stack(parent).removeArrangedSubview(&a_view);
    a_view.removeFromSuperview();
    set_hidden(a, false);
    assert_children(parent, &[c, a, b]);
    remove_child(parent, c);
    assert_children(parent, &[a, b]);
    stack(parent).removeArrangedSubview(&a_view);
    a_view.removeFromSuperview();
    set_hidden(a, false);
    assert_children(parent, &[a, b]);
    PARENT_MAP.with(|parents| assert!(!parents.borrow().contains_key(&c)));

    let nested = vstack::create(0.0);
    let descendant = spacer::create();
    add_child(nested, descendant);
    add_child(parent, nested);
    set_height(descendant, 25.0);
    BG_COLOR_MAP.with(|colors| colors.borrow_mut().insert(descendant, (0.1, 0.2, 0.3, 1.0)));
    clear_children(parent);
    assert_children(parent, &[]);
    PARENT_MAP.with(|parents| {
        for handle in [a, b, nested, descendant] {
            assert!(!parents.borrow().contains_key(&handle));
        }
    });
    WIDTH_CONSTRAINTS.with(|constraints| assert!(!constraints.borrow().contains_key(&b)));
    HEIGHT_CONSTRAINTS.with(|constraints| assert!(!constraints.borrow().contains_key(&descendant)));
    BG_COLOR_MAP.with(|colors| assert!(!colors.borrow().contains_key(&descendant)));
}

#[test]
fn stack_registry_observes_layout_replacement_and_first_handle() {
    let mtm = MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let parent = vstack::create(0.0);
    let child = spacer::create();
    add_child(parent, child);
    let original = get_layout_widget(child).unwrap();
    set_max_width(child, 100.0);
    let wrapped = get_layout_widget(child).unwrap();
    assert_ne!(Retained::as_ptr(&original), Retained::as_ptr(&wrapped));
    let sibling = spacer::create();
    add_child_at(parent, sibling, 0);
    assert_children(parent, &[sibling, child]);
    let duplicate = register_widget(wrapped);
    refresh_stack_parent_map(parent, &stack(parent));
    assert_children(parent, &[sibling, child]);
    PARENT_MAP.with(|parents| assert!(!parents.borrow().contains_key(&duplicate)));
}

#[test]
fn stack_registry_preserves_wrapped_alias_reparent_roundtrip() {
    let mtm = MainThreadMarker::new().expect("panic-abort tests run on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let left = vstack::create(0.0);
    let right = hstack::create(0.0);
    let a = spacer::create();
    let b = spacer::create();
    let c = spacer::create();
    add_child(left, a);
    add_child(left, b);
    set_max_width(a, 73.0);
    let alias = register_widget(get_layout_widget(a).unwrap());
    add_child_at(left, c, 0);
    assert_children(left, &[c, a, b]);
    add_child_at(right, a, 0);
    assert_children(left, &[c, b]);
    assert_children(right, &[a]);
    set_hidden(a, true);
    let slot = get_layout_widget(a).unwrap();
    stack(right).removeArrangedSubview(&slot);
    slot.removeFromSuperview();
    set_hidden(a, false);
    assert_children(right, &[a]);
    add_child_at(left, a, 1);
    assert_children(left, &[c, a, b]);
    assert_children(right, &[]);
    PARENT_MAP.with(|p| assert!(!p.borrow().contains_key(&alias)));
    clear_children(left);
    PARENT_MAP.with(|p| {
        for h in [a, b, c, alias] {
            assert!(!p.borrow().contains_key(&h));
        }
    });
    println!("wrapped/alias/reparent/hidden/cleanup roundtrip PASS");
}
