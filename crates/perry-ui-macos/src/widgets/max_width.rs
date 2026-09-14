//! A full-width layout slot lets a capped child center without fighting its
//! NSStackView's leading/fill alignment. Widget operations still target the
//! original content view; only insertion/removal uses the slot.

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSLayoutConstraint, NSStackView, NSStackViewGravity, NSView};
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;

pub struct MaxWidthIvars {
    content: Retained<NSView>,
    cap: Retained<NSLayoutConstraint>,
    parent_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "PerryMaxWidthView"]
    #[ivars = MaxWidthIvars]
    pub struct MaxWidthView;

    impl MaxWidthView {
        #[unsafe(method(viewDidMoveToSuperview))]
        fn did_move_to_superview(&self) {
            unsafe { let _: () = msg_send![super(self), viewDidMoveToSuperview]; }
            self.refresh_parent();
        }
    }
);

impl MaxWidthView {
    fn refresh_parent(&self) {
        let mut constraints = self.ivars().parent_constraints.borrow_mut();
        for constraint in constraints.drain(..) {
            constraint.setActive(false);
        }
        let Some(parent) = (unsafe { self.superview() }) else {
            return;
        };
        // A stack's edge insets are inside its bounds. Do not fill through them.
        let inset = parent
            .downcast_ref::<NSStackView>()
            .map(|stack| {
                let insets = stack.edgeInsets();
                insets.left + insets.right
            })
            .unwrap_or(0.0);
        let fits = self
            .widthAnchor()
            .constraintLessThanOrEqualToAnchor_constant(&parent.widthAnchor(), -inset);
        let fills = self
            .widthAnchor()
            .constraintEqualToAnchor_constant(&parent.widthAnchor(), -inset);
        fills.setPriority(999.0);
        fits.setActive(true);
        fills.setActive(true);
        constraints.extend([fits, fills]);
    }
}

pub(super) fn content(view: Retained<NSView>) -> Retained<NSView> {
    view.downcast_ref::<MaxWidthView>()
        .map(|slot| slot.ivars().content.clone())
        .unwrap_or(view)
}

pub(super) fn clear_cap(handle: i64) {
    if let Some(view) = super::get_layout_widget(handle) {
        if let Some(slot) = view.downcast_ref::<MaxWidthView>() {
            slot.ivars().cap.setActive(false);
        }
    }
}

pub(super) fn refresh_children(view: &NSView) {
    for child in view.subviews() {
        if let Some(slot) = child.downcast_ref::<MaxWidthView>() {
            slot.refresh_parent();
        }
    }
}

/// Fill the available width up to `max_width`, keeping the content centered.
/// Invalid values are ignored; zero is a valid cap. A new cap replaces an old
/// fixed width, and repeated calls update the existing constraint in place.
pub fn set_max_width(handle: i64, max_width: f64) {
    if !max_width.is_finite() || max_width < 0.0 {
        return;
    }
    let Some(view) = super::get_layout_widget(handle) else {
        return;
    };
    super::WIDTH_CONSTRAINTS.with(|constraints| {
        if let Some(old) = constraints.borrow_mut().remove(&handle) {
            unsafe {
                let _: () = msg_send![&*old, setActive: false];
            }
        }
    });
    if let Some(slot) = view.downcast_ref::<MaxWidthView>() {
        slot.ivars().cap.setConstant(max_width);
        slot.ivars().cap.setActive(true);
        return;
    }

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let parent = unsafe { view.superview() };
    let window = view
        .window()
        .filter(|window| window.contentView().as_deref() == Some(&view));
    let stack_position = parent.as_ref().and_then(|parent| {
        parent.downcast_ref::<NSStackView>().and_then(|stack| {
            stack
                .viewsInGravity(NSStackViewGravity::Top)
                .iter()
                .position(|child| Retained::as_ptr(&child) == Retained::as_ptr(&view))
        })
    });

    // Existing parent pins (matchParentWidth, ZStack, ScrollView, app body)
    // describe the layout slot. Transfer them so the cap can win inside it.
    let mut external = Vec::new();
    let mut ancestor = parent.clone();
    while let Some(owner) = ancestor {
        for constraint in owner.constraints() {
            unsafe {
                let first = constraint.firstItem();
                let second = constraint.secondItem();
                let is_content = |item: &Option<Retained<objc2::runtime::AnyObject>>| {
                    item.as_ref().is_some_and(|item| {
                        Retained::as_ptr(item).cast::<NSView>() == Retained::as_ptr(&view)
                    })
                };
                if is_content(&first) || is_content(&second) {
                    constraint.setActive(false);
                    external.push((constraint, first, second));
                }
            }
        }
        ancestor = unsafe { owner.superview() };
    }
    if let Some(stack) = parent
        .as_ref()
        .and_then(|parent| parent.downcast_ref::<NSStackView>())
    {
        stack.removeView(&view);
    }
    view.removeFromSuperview();
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    let cap = view
        .widthAnchor()
        .constraintLessThanOrEqualToConstant(max_width);
    let slot = MaxWidthView::alloc(mtm).set_ivars(MaxWidthIvars {
        content: view.clone(),
        cap: cap.clone(),
        parent_constraints: RefCell::new(Vec::new()),
    });
    let slot: Retained<MaxWidthView> = unsafe { msg_send![super(slot), init] };
    slot.setTranslatesAutoresizingMaskIntoConstraints(false);
    slot.setHidden(view.isHidden());
    slot.addSubview(&view);
    let fills = view
        .widthAnchor()
        .constraintEqualToAnchor(&slot.widthAnchor());
    fills.setPriority(998.0);
    let fits = view
        .widthAnchor()
        .constraintLessThanOrEqualToAnchor(&slot.widthAnchor());
    // An explicit setWidth applied later can still overflow a narrow parent.
    fits.setPriority(999.0);
    for constraint in [
        cap,
        fills,
        fits,
        view.centerXAnchor()
            .constraintEqualToAnchor(&slot.centerXAnchor()),
        view.topAnchor().constraintEqualToAnchor(&slot.topAnchor()),
        view.bottomAnchor()
            .constraintEqualToAnchor(&slot.bottomAnchor()),
    ] {
        constraint.setActive(true);
    }
    let slot: Retained<NSView> = slot.into_super();
    super::WIDGETS.with(|widgets| widgets.borrow_mut()[handle as usize - 1] = slot.clone());
    if let Some(window) = window {
        window.setContentView(Some(&slot));
    } else if let Some(parent) = parent {
        if let (Some(stack), Some(index)) = (parent.downcast_ref::<NSStackView>(), stack_position) {
            stack.insertView_atIndex_inGravity(&slot, index, NSStackViewGravity::Top);
        } else {
            parent.addSubview(&slot);
        }
    }
    for (old, first, second) in external {
        unsafe {
            let replace = |item: Option<Retained<objc2::runtime::AnyObject>>| {
                item.map(|item| {
                    if Retained::as_ptr(&item).cast::<NSView>() == Retained::as_ptr(&view) {
                        Retained::cast_unchecked(slot.clone())
                    } else {
                        item
                    }
                })
            };
            let first = replace(first);
            let second = replace(second);
            let constraint = NSLayoutConstraint::constraintWithItem_attribute_relatedBy_toItem_attribute_multiplier_constant(
                first.as_deref().unwrap(), old.firstAttribute(), old.relation(),
                second.as_deref(), old.secondAttribute(), old.multiplier(), old.constant(),
            );
            constraint.setPriority(old.priority());
            constraint.setActive(true);
        }
    }
}
