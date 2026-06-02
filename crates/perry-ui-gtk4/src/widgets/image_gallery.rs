//! GTK4 ImageGallery — `GtkScrolledWindow` containing a horizontal `GtkBox`
//! of fixed-size `GtkPicture` widgets. Issue #553. Page-snap is approximated
//! via `set_index` calling `hadjustment.set_value(index * page_width)`; the
//! user's free swipe scrolls smoothly without snap.

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

const PAGE_PX: i32 = 320;

struct GalleryState {
    scrolled: gtk4::ScrolledWindow,
    inner: gtk4::Box,
    pages: Vec<gtk4::Picture>,
    on_index_change: f64,
    current_index: i64,
}

thread_local! {
    static STATES: RefCell<HashMap<i64, GalleryState>> = RefCell::new(HashMap::new());
}

pub fn create(on_index_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_hscrollbar_policy(gtk4::PolicyType::Automatic);
    scrolled.set_vscrollbar_policy(gtk4::PolicyType::Never);
    scrolled.set_size_request(PAGE_PX, PAGE_PX);

    let inner = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    scrolled.set_child(Some(&inner));

    let handle = super::register_widget(scrolled.clone().upcast());

    // Wire scroll-end-style index update — when the user finishes scrolling,
    // recompute which page is closest to centered and fire on_index_change
    // if it changed.
    {
        let h_handle = handle;
        let hadj = scrolled.hadjustment();
        hadj.connect_value_changed(move |adj| {
            let value = adj.value();
            STATES.with(|s| {
                let mut states = s.borrow_mut();
                let Some(state) = states.get_mut(&h_handle) else {
                    return;
                };
                let new_index = ((value + PAGE_PX as f64 / 2.0) / PAGE_PX as f64).floor() as i64;
                let count = state.pages.len() as i64;
                let new_index = new_index.max(0).min(count.saturating_sub(1));
                if new_index != state.current_index {
                    state.current_index = new_index;
                    let cb = state.on_index_change;
                    if cb != 0.0 {
                        unsafe {
                            let ptr = js_nanbox_get_pointer(cb) as *const u8;
                            js_closure_call1(ptr, new_index as f64);
                        }
                    }
                }
            });
        });
    }

    STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            GalleryState {
                scrolled,
                inner,
                pages: Vec::new(),
                on_index_change,
                current_index: 0,
            },
        );
    });
    handle
}

pub fn add_image(handle: i64, url_ptr: *const u8, alt_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    let alt = str_from_header(alt_ptr);
    STATES.with(|s| {
        let mut states = s.borrow_mut();
        let Some(state) = states.get_mut(&handle) else {
            return;
        };

        let pic = gtk4::Picture::new();
        pic.set_size_request(PAGE_PX, PAGE_PX);
        pic.set_can_shrink(true);
        // Picture's default keep-aspect-ratio + can_shrink gives us
        // ContentFit::Contain semantics on GTK 4.6 (jammy / Ubuntu 22.04).
        // The explicit set_content_fit call requires v4_8 feature, but the
        // crate's gtk4-rs feature is pinned to v4_6 to keep CI's Ubuntu 22.04
        // glibc build working — release-packages.yml uses libgtk-4-dev 4.6.9.

        if !alt.is_empty() {
            pic.set_tooltip_text(Some(alt));
        }

        if !url.is_empty() {
            if url.starts_with("http://") || url.starts_with("https://") {
                // Remote loading via libsoup3 would require an extra dep;
                // for the GTK4 path we skip remote URLs and document the
                // gap. Local paths cover the production case.
            } else {
                let path = std::path::Path::new(url);
                if path.exists() {
                    pic.set_filename(Some(url));
                }
            }
        }

        state.inner.append(&pic);
        state.pages.push(pic);
    });
}

pub fn set_index(handle: i64, index: i64) {
    STATES.with(|s| {
        let mut states = s.borrow_mut();
        let Some(state) = states.get_mut(&handle) else {
            return;
        };
        if (index as usize) >= state.pages.len() {
            return;
        }
        state.current_index = index;
        let target = (index as f64) * (PAGE_PX as f64);
        state.scrolled.hadjustment().set_value(target);
    });
}
