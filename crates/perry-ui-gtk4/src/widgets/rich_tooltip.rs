//! GTK4 Rich tooltip — wraps `GtkPopover` anchored to the host widget,
//! shown after a hover delay (issue #479 / Linux parity work).
//!
//! `gtk4::Tooltip` only accepts plain text or a single image; the
//! `tooltip` property hooks already cover that path. For arbitrary
//! widget-tree tooltip bodies, `GtkPopover` is the natural primitive
//! (modal-less, anchored-to-widget overlay). Hover detection uses
//! `GtkEventControllerMotion` (enter/leave) plus a `glib::timeout`
//! for the delay before pop-up.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

struct RichTooltipBinding {
    content_handle: i64,
    hover_delay_ms: u32,
    /// Active popover (if any) — kept so `mouseExited` can close it.
    popover: Rc<RefCell<Option<gtk4::Popover>>>,
    /// Pending show timer; cancelled on leave so brief hovers don't pop.
    show_token: Rc<Cell<u64>>,
}

thread_local! {
    static BINDINGS: RefCell<HashMap<i64, RichTooltipBinding>> = RefCell::new(HashMap::new());
}

pub fn set_rich_tooltip(widget_handle: i64, content_handle: i64, hover_delay_ms: u32) {
    let delay = if hover_delay_ms == 0 { 500 } else { hover_delay_ms };
    let popover_slot: Rc<RefCell<Option<gtk4::Popover>>> = Rc::new(RefCell::new(None));
    let show_token: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    BINDINGS.with(|b| {
        b.borrow_mut().insert(
            widget_handle,
            RichTooltipBinding {
                content_handle,
                hover_delay_ms: delay,
                popover: popover_slot.clone(),
                show_token: show_token.clone(),
            },
        );
    });

    let Some(widget) = super::get_widget(widget_handle) else { return };
    let motion = gtk4::EventControllerMotion::new();

    let widget_for_enter = widget.clone();
    let popover_slot_enter = popover_slot.clone();
    let show_token_enter = show_token.clone();
    motion.connect_enter(move |_ctrl, _x, _y| {
        let token = show_token_enter.get().wrapping_add(1);
        show_token_enter.set(token);
        let widget_inner = widget_for_enter.clone();
        let popover_slot_inner = popover_slot_enter.clone();
        let show_token_inner = show_token_enter.clone();
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(delay as u64),
            move || {
                if show_token_inner.get() != token {
                    return;
                }
                let content = super::get_widget(content_handle);
                let Some(content) = content else { return };
                // Detach from any previous parent so we can hand it to
                // the popover; safe because the WIDGETS table keeps
                // the strong reference alive.
                if let Some(parent) = content.parent() {
                    if let Some(b) = parent.downcast_ref::<gtk4::Box>() {
                        b.remove(&content);
                    }
                }
                let popover = gtk4::Popover::new();
                popover.set_parent(&widget_inner);
                popover.set_autohide(false);
                popover.set_has_arrow(true);
                popover.set_position(gtk4::PositionType::Bottom);
                popover.set_child(Some(&content));
                popover.popup();
                *popover_slot_inner.borrow_mut() = Some(popover);
            },
        );
    });

    let popover_slot_leave = popover_slot.clone();
    let show_token_leave = show_token.clone();
    motion.connect_leave(move |_ctrl| {
        // Bump token so any pending show callback bails out.
        show_token_leave.set(show_token_leave.get().wrapping_add(1));
        if let Some(p) = popover_slot_leave.borrow_mut().take() {
            p.popdown();
            // Detach from parent + child so subsequent shows can re-host.
            p.unparent();
        }
    });

    widget.add_controller(motion);
}
