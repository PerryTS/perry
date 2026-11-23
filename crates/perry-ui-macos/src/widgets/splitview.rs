use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSLayoutConstraint, NSSplitView, NSSplitViewDividerStyle, NSView};
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;

/// Initial width of the first (sidebar) pane, in points.
const FIRST_PANE_WIDTH: f64 = 260.0;

pub struct PerrySplitViewIvars {
    /// Starting width of the first pane. Active only until the user first
    /// presses the mouse in the split view (i.e. starts dragging the divider);
    /// left active it would snap the divider back when the drag ends.
    initial_width: RefCell<Option<Retained<NSLayoutConstraint>>>,
}

define_class!(
    #[unsafe(super(NSSplitView, NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "PerrySplitView"]
    #[ivars = PerrySplitViewIvars]
    pub struct PerrySplitView;

    impl PerrySplitView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(c) = self.ivars().initial_width.borrow_mut().take() {
                c.setActive(false);
            }
            unsafe {
                let _: () = msg_send![super(self), mouseDown: event];
            }
        }
    }
);

/// Create a side-by-side split view (vertical divider, thin style).
///
/// Children are added with [`add_child`] and become the split view's arranged
/// subviews, left to right. The first pane starts at `FIRST_PANE_WIDTH` and
/// keeps its width when the window resizes (the last pane absorbs the change);
/// the divider can be dragged.
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let this = PerrySplitView::alloc(mtm).set_ivars(PerrySplitViewIvars {
        initial_width: RefCell::new(None),
    });
    let split: Retained<PerrySplitView> = unsafe { msg_send![super(this), init] };
    split.setVertical(true);
    split.setDividerStyle(NSSplitViewDividerStyle::Thin);
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(split) };
    super::register_widget(view)
}

/// Append `child` as the next pane of the split view `parent`.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    let (Some(parent), Some(child)) = (
        super::get_layout_widget(parent_handle),
        super::get_layout_widget(child_handle),
    ) else {
        return;
    };
    let Some(split) = parent.downcast_ref::<NSSplitView>() else {
        return;
    };
    let index = split.arrangedSubviews().count();
    split.addArrangedSubview(&child);
    if index == 0 {
        // Starting width for the first pane. Priority 490 beats the split
        // view's own even split but stays below
        // NSLayoutPriorityDragThatCanResizeSplitView (510); the constraint is
        // dropped on the first mouse-down so a dragged divider stays put.
        let width = child
            .widthAnchor()
            .constraintEqualToConstant(FIRST_PANE_WIDTH);
        width.setPriority(490.0);
        width.setActive(true);
        let obj: &AnyObject = split.as_ref();
        if let Some(ours) = obj.downcast_ref::<PerrySplitView>() {
            *ours.ivars().initial_width.borrow_mut() = Some(width);
        }
        // The sidebar holds its width on window resize; later panes give way.
        split.setHoldingPriority_forSubviewAtIndex(260.0, 0);
    } else {
        split.setHoldingPriority_forSubviewAtIndex(250.0, index as isize);
    }
}
