//! GTK4 BottomNavigation — horizontal `GtkBox` of `GtkButton` tabs each
//! containing a vertical (`GtkImage` icon + `GtkLabel` text), with optional
//! badge label drawn as a small overlay on the icon. Issue #553.

use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

struct ItemViews {
    button: gtk4::Button,
    icon: gtk4::Image,
    label: gtk4::Label,
    badge: Option<gtk4::Label>,
    container: gtk4::Box, // vertical inner box (icon+label)
}

struct BottomNavState {
    bar: gtk4::Box,
    items: Vec<ItemViews>,
    on_select: f64,
    selected: i64,
}

thread_local! {
    static STATES: RefCell<HashMap<i64, BottomNavState>> = RefCell::new(HashMap::new());
}

pub fn create(on_select: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    bar.add_css_class("toolbar");
    bar.set_homogeneous(true);
    let handle = super::register_widget(bar.clone().upcast());
    STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            BottomNavState {
                bar,
                items: Vec::new(),
                on_select,
                selected: 0,
            },
        );
    });
    handle
}

pub fn add_item(handle: i64, icon_ptr: *const u8, label_ptr: *const u8) {
    let icon_name = str_from_header(icon_ptr);
    let label_text = str_from_header(label_ptr);

    let bar = STATES.with(|s| s.borrow().get(&handle).map(|st| st.bar.clone()));
    let Some(bar) = bar else { return };

    let inner = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    inner.set_halign(gtk4::Align::Center);

    let icon = if icon_name.is_empty() {
        gtk4::Image::new()
    } else {
        gtk4::Image::from_icon_name(icon_name)
    };
    icon.set_pixel_size(24);
    inner.append(&icon);

    let label = gtk4::Label::new(Some(label_text));
    label.add_css_class("caption");
    inner.append(&label);

    let button = gtk4::Button::new();
    button.set_child(Some(&inner));
    button.set_has_frame(false);

    let item_index = STATES.with(|s| {
        s.borrow()
            .get(&handle)
            .map(|st| st.items.len() as i64)
            .unwrap_or(0)
    });

    {
        let bar_handle = handle;
        button.connect_clicked(move |_btn| {
            select_index(bar_handle, item_index);
            let on_select = STATES.with(|s| {
                s.borrow()
                    .get(&bar_handle)
                    .map(|st| st.on_select)
                    .unwrap_or(0.0)
            });
            if on_select != 0.0 {
                unsafe {
                    let ptr = js_nanbox_get_pointer(on_select) as *const u8;
                    js_closure_call1(ptr, item_index as f64);
                }
            }
        });
    }

    bar.append(&button);

    STATES.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&handle) {
            state.items.push(ItemViews {
                button,
                icon,
                label,
                badge: None,
                container: inner,
            });
        }
    });
    apply_styling(handle);
}

pub fn set_badge(handle: i64, index: i64, badge_ptr: *const u8) {
    let badge_text = str_from_header(badge_ptr);
    STATES.with(|s| {
        let mut nav = s.borrow_mut();
        let Some(state) = nav.get_mut(&handle) else {
            return;
        };
        let Some(item) = state.items.get_mut(index as usize) else {
            return;
        };
        if let Some(old) = item.badge.take() {
            item.container.remove(&old);
        }
        if !badge_text.is_empty() {
            let badge = gtk4::Label::new(Some(badge_text));
            badge.add_css_class("error"); // Adwaita styles "error" badges red.
            badge.add_css_class("caption-heading");
            item.container.prepend(&badge);
            item.badge = Some(badge);
        }
    });
}

pub fn set_selected(handle: i64, index: i64) {
    select_index(handle, index);
}

fn select_index(handle: i64, index: i64) {
    STATES.with(|s| {
        if let Some(state) = s.borrow_mut().get_mut(&handle) {
            if (index as usize) < state.items.len() {
                state.selected = index;
            }
        }
    });
    apply_styling(handle);
}

fn apply_styling(handle: i64) {
    STATES.with(|s| {
        let nav = s.borrow();
        let Some(state) = nav.get(&handle) else {
            return;
        };
        for (i, item) in state.items.iter().enumerate() {
            if i as i64 == state.selected {
                item.button.add_css_class("suggested-action");
                item.label.add_css_class("accent");
            } else {
                item.button.remove_css_class("suggested-action");
                item.label.remove_css_class("accent");
            }
        }
    });
}
