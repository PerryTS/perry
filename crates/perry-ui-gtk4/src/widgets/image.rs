use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Store image widgets for set_size/set_tint operations
    static IMAGE_WIDGETS: RefCell<HashMap<i64, ImageKind>> = RefCell::new(HashMap::new());
}

enum ImageKind {
    File(gtk4::Picture, Option<String>),
    Symbol(gtk4::Image),
}

pub(crate) fn str_from_header(ptr: *const u8) -> &'static str {
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

/// Create an image from a file path.
pub fn create_file(path_ptr: *const u8) -> i64 {
    crate::app::ensure_gtk_init();
    let raw = str_from_header(path_ptr);
    let path = raw.split('\0').next().unwrap_or(raw);
    // Resolve path relative to executable directory (handles bundled assets)
    let resolved = crate::resolve_asset_path(path);
    let picture = gtk4::Picture::for_filename(&resolved);
    picture.set_can_shrink(true);
    let handle = super::register_widget(picture.clone().upcast());
    IMAGE_WIDGETS.with(|w| {
        w.borrow_mut().insert(
            handle,
            ImageKind::File(picture, Some(resolved.to_string_lossy().to_string())),
        )
    });
    handle
}

/// Create an image from a named icon (freedesktop icon theme).
pub fn create_symbol(name_ptr: *const u8) -> i64 {
    crate::app::ensure_gtk_init();
    let name = str_from_header(name_ptr);
    let image = gtk4::Image::from_icon_name(name);
    let handle = super::register_widget(image.clone().upcast());
    IMAGE_WIDGETS.with(|w| w.borrow_mut().insert(handle, ImageKind::Symbol(image)));
    handle
}

/// Set the size of an image widget.
pub fn set_size(handle: i64, width: f64, height: f64) {
    IMAGE_WIDGETS.with(|w| {
        if let Some(kind) = w.borrow().get(&handle) {
            match kind {
                ImageKind::File(picture, path) => {
                    let w = width as i32;
                    let h = height as i32;
                    // Load a scaled Pixbuf so the Picture renders at the exact size
                    if let Some(path) = path {
                        if let Ok(pixbuf) =
                            gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(path, w, h, true)
                        {
                            let texture = gtk4::gdk::Texture::for_pixbuf(&pixbuf);
                            picture.set_paintable(Some(&texture));
                        }
                    }
                    picture.set_size_request(w, h);
                    picture.set_halign(gtk4::Align::Start);
                    picture.set_valign(gtk4::Align::Center);
                }
                ImageKind::Symbol(image) => {
                    image.set_pixel_size(width.max(height) as i32);
                }
            }
        }
    });
}

/// Set the tint color of an image (applies CSS color filter).
pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    if let Some(widget) = super::get_widget(handle) {
        let css = format!(
            "image {{ color: rgb({},{},{}); }}",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8
        );
        crate::widgets::apply_css(&widget, &css);
    }
}
