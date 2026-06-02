//! ImageGallery widget — paged image carousel.
//!
//! Win32 implementation: a single `PerryImage` HWND (registered via
//! `widgets::image`) displays the current page; `set_index` swaps the
//! URL via `image::set_url` which kicks off a fresh WinHTTP fetch on
//! a background thread and repaints once decoded. Images are stored
//! as `(url, alt)` pairs — `on_index_change` fires whenever the
//! visible page changes.
//!
//! Note: this is a single-image-at-a-time carousel; the
//! UIScrollView-style horizontal swipe + paged-scroll affordance
//! (e.g. on iOS) lands in a follow-up if needed. The user-driving
//! API (`set_index`) + the visual feedback (real image render) match
//! the macOS shape one-for-one.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::SS_CENTER;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

#[derive(Clone)]
struct ImageEntry {
    url: String,
    alt: String,
}

struct GalleryEntry {
    handle: i64,
    images: Vec<ImageEntry>,
    index: i64,
    on_index_change: f64,
}

thread_local! {
    static GALLERIES: RefCell<HashMap<i64, GalleryEntry>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn create(on_index_change: f64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        // Reuse the same PerryImage HWND class as `Image(url)` — the
        // gallery is conceptually one Image widget whose URL changes
        // on `set_index`. `super::image::create_url` with an empty
        // URL leaves it blank until `add_image` adds the first page.
        let empty: [u8; 0] = [];
        let handle = super::image::create_url(empty.as_ptr(), empty.as_ptr());
        GALLERIES.with(|g| {
            g.borrow_mut().insert(
                handle,
                GalleryEntry {
                    handle,
                    images: Vec::new(),
                    index: 0,
                    on_index_change,
                },
            );
        });
        handle
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = on_index_change;
        let control_id = alloc_control_id();
        let handle = register_widget(0, WidgetKind::Image, control_id);
        GALLERIES.with(|g| {
            g.borrow_mut().insert(
                handle,
                GalleryEntry {
                    handle,
                    images: Vec::new(),
                    index: 0,
                    on_index_change,
                },
            );
        });
        handle
    }
}

/// Update the visible image to the gallery's current `index`. Routes
/// through `image::set_url` which clears the old bytes and kicks off
/// a fresh WinHTTP fetch on a background thread.
fn refresh_image(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let url_opt = GALLERIES.with(|g| {
            g.borrow().get(&handle).and_then(|gal| {
                if gal.images.is_empty() {
                    None
                } else {
                    let idx = gal.index.clamp(0, (gal.images.len() as i64) - 1) as usize;
                    Some(gal.images[idx].url.clone())
                }
            })
        });
        if let Some(url) = url_opt {
            // Stuff the URL into a heap-allocated StringHeader so we
            // can pass it through the existing `set_url` ptr-based
            // signature. Length-prefixed UTF-8 matches the on-the-wire
            // format the runtime uses.
            let bytes = url.as_bytes();
            let str_ptr =
                perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            super::image::set_url(handle, str_ptr as *const u8);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn add_image(handle: i64, url_ptr: *const u8, alt_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    let alt = str_from_header(alt_ptr);
    GALLERIES.with(|g| {
        if let Some(gal) = g.borrow_mut().get_mut(&handle) {
            gal.images.push(ImageEntry { url, alt });
        }
    });
    refresh_image(handle);
}

pub fn set_index(handle: i64, index: i64) {
    let on_change = GALLERIES.with(|g| {
        let mut galleries = g.borrow_mut();
        if let Some(gal) = galleries.get_mut(&handle) {
            let max = gal.images.len() as i64;
            let new_idx = if max == 0 { 0 } else { index.clamp(0, max - 1) };
            if gal.index == new_idx {
                return None;
            }
            gal.index = new_idx;
            Some((gal.on_index_change, new_idx))
        } else {
            None
        }
    });
    refresh_image(handle);
    if let Some((closure, idx)) = on_change {
        if closure != 0.0 {
            let closure_ptr = unsafe { js_nanbox_get_pointer(closure) } as *const u8;
            if !closure_ptr.is_null() {
                unsafe {
                    js_closure_call1(closure_ptr, idx as f64);
                }
            }
        }
    }
}
