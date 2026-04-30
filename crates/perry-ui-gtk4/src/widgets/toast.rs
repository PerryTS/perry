use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, CssProvider, Label, Overlay, Revealer, RevealerTransitionType};
use std::cell::RefCell;
use std::collections::VecDeque;

thread_local! {
    static TOAST_OVERLAY: RefCell<Option<Overlay>> = RefCell::new(None);
    static TOAST_QUEUE: RefCell<VecDeque<String>> = RefCell::new(VecDeque::new());
    static TOAST_ACTIVE: RefCell<bool> = RefCell::new(false);
}

/// Register the root overlay so subsequent show_toast calls can inject banners.
/// Called from app.rs immediately after the window overlay is created.
pub fn set_toast_overlay(overlay: &Overlay) {
    TOAST_OVERLAY.with(|o| *o.borrow_mut() = Some(overlay.clone()));
}

/// Queue a toast notification and start draining the queue if idle.
pub fn show_toast(message: &str) {
    TOAST_QUEUE.with(|q| q.borrow_mut().push_back(message.to_string()));
    pump_queue();
}

/// Show the next queued message (no-op if one is already visible).
fn pump_queue() {
    let active = TOAST_ACTIVE.with(|a| *a.borrow());
    if active {
        return;
    }
    let msg = TOAST_QUEUE.with(|q| q.borrow_mut().pop_front());
    let Some(msg) = msg else { return };

    let overlay = TOAST_OVERLAY.with(|o| o.borrow().clone());
    let Some(overlay) = overlay else {
        // No window yet — messages can arrive during startup; re-queue once
        // the overlay is wired up (next show_toast call after app_run fires).
        TOAST_QUEUE.with(|q| q.borrow_mut().push_front(msg));
        return;
    };

    TOAST_ACTIVE.with(|a| *a.borrow_mut() = true);

    // --- Build the toast widget tree: Revealer → Box → Label ---
    let label = Label::new(Some(&msg));
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    label.set_margin_start(16);
    label.set_margin_end(16);
    label.set_wrap(true);
    label.set_max_width_chars(60);

    let toast_box = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    toast_box.add_css_class("perry-toast");
    toast_box.append(&label);

    let css = CssProvider::new();
    css.load_from_data(
        ".perry-toast { \
            background-color: rgba(30,30,30,0.88); \
            color: #ffffff; \
            border-radius: 8px; \
        }",
    );
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let revealer = Revealer::new();
    revealer.set_transition_type(RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(200);
    revealer.set_child(Some(&toast_box));
    revealer.set_halign(gtk4::Align::Center);
    revealer.set_valign(gtk4::Align::Start);
    revealer.set_margin_top(8);
    revealer.set_can_target(false); // pass click/pointer events through

    overlay.add_overlay(&revealer);
    // slide in
    revealer.set_reveal_child(true);

    // After 2500 ms hold, slide out then remove.
    let revealer_hide = revealer.clone();
    let overlay_remove = overlay.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || {
        revealer_hide.set_reveal_child(false);
        // Wait for the hide transition to complete (transition_duration + margin).
        let revealer_rm = revealer_hide.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
            overlay_remove.remove_overlay(&revealer_rm);
            TOAST_ACTIVE.with(|a| *a.borrow_mut() = false);
            pump_queue(); // show next message if any
        });
    });
}
