#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSStackView, NSView};
    use objc2_foundation::MainThreadMarker;
    use perry_runtime as _;
    use perry_ui_macos::widgets;

    fn children(handle: i64) -> Vec<usize> {
        let view = widgets::get_widget(handle).unwrap();
        let stack = unsafe { &*(Retained::as_ptr(&view) as *const NSStackView) };
        stack
            .arrangedSubviews()
            .iter()
            .map(|v| Retained::as_ptr(&v) as usize)
            .collect()
    }
    fn ptr(handle: i64) -> usize {
        Retained::as_ptr(&widgets::get_widget(handle).unwrap()) as usize
    }

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_widget_order: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("native widget test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let parent = widgets::vstack::create(0.0);
    let other = widgets::hstack::create(0.0);
    let a = widgets::spacer::create();
    let b = widgets::spacer::create();
    let c = widgets::spacer::create();
    widgets::add_child(parent, a);
    widgets::add_child(parent, b);
    widgets::add_child_at(parent, c, 1);
    assert_eq!(
        children(parent),
        vec![ptr(a), ptr(c), ptr(b)],
        "indexed insertion must affect native order"
    );

    widgets::add_child_at(parent, a, 2);
    assert_eq!(children(parent), vec![ptr(c), ptr(b), ptr(a)]);
    widgets::set_width(b, 80.0);
    widgets::add_child_at(other, b, 0);
    assert_eq!(children(parent), vec![ptr(c), ptr(a)]);
    assert_eq!(children(other), vec![ptr(b)]);
    let b_view = widgets::get_widget(b).unwrap();
    assert!(
        b_view
            .constraints()
            .iter()
            .any(|constraint| constraint.constant() == 80.0 && constraint.isActive()),
        "moving a widget preserves its width constraint"
    );

    widgets::add_child_at(parent, b, -1);
    assert_eq!(children(parent), vec![ptr(b), ptr(c), ptr(a)]);
    assert!(children(other).is_empty());
    widgets::reorder_child(parent, 0, 2);
    assert_eq!(children(parent), vec![ptr(c), ptr(a), ptr(b)]);

    // Simulate a stack-detached hidden child, then exercise the cached position
    // used by set_hidden. Reordering must update that position for every child.
    let parent_view = widgets::get_widget(parent).unwrap();
    let stack = unsafe { &*(Retained::as_ptr(&parent_view) as *const NSStackView) };
    let a_view: Retained<NSView> = widgets::get_widget(a).unwrap();
    stack.removeArrangedSubview(&a_view);
    a_view.removeFromSuperview();
    widgets::set_hidden(a, false);
    assert_eq!(children(parent), vec![ptr(c), ptr(a), ptr(b)]);
    widgets::remove_child(parent, c);
    stack.removeArrangedSubview(&a_view);
    a_view.removeFromSuperview();
    widgets::set_hidden(a, false);
    assert_eq!(
        children(parent),
        vec![ptr(a), ptr(b)],
        "removal refreshes surviving cached positions"
    );
    widgets::add_child_at(parent, c, i64::MAX);
    assert_eq!(children(parent), vec![ptr(a), ptr(b), ptr(c)]);
    println!(
        "PASS native widget ordering, reparenting, retained constraints, and hidden reattachment"
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {}
