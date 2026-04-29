use objc2::msg_send;
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static ZSTACK_HANDLES: RefCell<HashSet<i64>> = RefCell::new(HashSet::new());
}

/// Check if a widget handle is a ZStack.
pub fn is_zstack(handle: i64) -> bool {
    ZSTACK_HANDLES.with(|h| h.borrow().contains(&handle))
}

/// Create an overlay (ZStack) container — a plain NSView where children are stacked on top.
/// Returns widget handle.
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    let handle = unsafe {
        let view: Retained<NSView> = msg_send![
            NSView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(300.0, 300.0),
            )
        ];

        super::register_widget(view)
    };
    ZSTACK_HANDLES.with(|h| {
        h.borrow_mut().insert(handle);
    });
    handle
}

/// Add a child to the ZStack pinned to fill the parent bounds using Auto Layout constraints.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (
        super::get_widget(parent_handle),
        super::get_widget(child_handle),
    ) {
        unsafe {
            parent.addSubview(&child);
            let _: () = msg_send![&*child, setTranslatesAutoresizingMaskIntoConstraints: false];

            // Pin child to fill parent using layout anchors
            let arr_cls = objc2::runtime::AnyClass::get(c"NSMutableArray").unwrap();
            let arr: *mut objc2::runtime::AnyObject = msg_send![arr_cls, arrayWithCapacity: 4usize];

            // leading
            let child_anchor: *mut objc2::runtime::AnyObject = msg_send![&*child, leadingAnchor];
            let parent_anchor: *mut objc2::runtime::AnyObject = msg_send![&*parent, leadingAnchor];
            let constraint: *mut objc2::runtime::AnyObject =
                msg_send![child_anchor, constraintEqualToAnchor: parent_anchor];
            let _: () = msg_send![arr, addObject: constraint];

            // trailing
            let child_anchor: *mut objc2::runtime::AnyObject = msg_send![&*child, trailingAnchor];
            let parent_anchor: *mut objc2::runtime::AnyObject = msg_send![&*parent, trailingAnchor];
            let constraint: *mut objc2::runtime::AnyObject =
                msg_send![child_anchor, constraintEqualToAnchor: parent_anchor];
            let _: () = msg_send![arr, addObject: constraint];

            // top
            let child_anchor: *mut objc2::runtime::AnyObject = msg_send![&*child, topAnchor];
            let parent_anchor: *mut objc2::runtime::AnyObject = msg_send![&*parent, topAnchor];
            let constraint: *mut objc2::runtime::AnyObject =
                msg_send![child_anchor, constraintEqualToAnchor: parent_anchor];
            let _: () = msg_send![arr, addObject: constraint];

            // bottom
            let child_anchor: *mut objc2::runtime::AnyObject = msg_send![&*child, bottomAnchor];
            let parent_anchor: *mut objc2::runtime::AnyObject = msg_send![&*parent, bottomAnchor];
            let constraint: *mut objc2::runtime::AnyObject =
                msg_send![child_anchor, constraintEqualToAnchor: parent_anchor];
            let _: () = msg_send![arr, addObject: constraint];

            // Activate all constraints
            let constraints: Retained<objc2::runtime::AnyObject> = Retained::retain(arr).unwrap();
            let _: () = msg_send![
                objc2::runtime::AnyClass::get(c"NSLayoutConstraint").unwrap(),
                activateConstraints: &*constraints
            ];
        }
    }
}
