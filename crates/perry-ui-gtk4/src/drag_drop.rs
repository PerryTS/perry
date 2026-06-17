//! GTK4 drag & drop FFI (issue #4773).
//!
//! Widget-level drag/drop setters exported by every `perry-ui-*` backend so
//! that `widgetOnDrop` / `widgetSetDrag*` compile and link on every
//! `--target`: codegen emits a single symbol name regardless of platform (see
//! `crates/perry-dispatch/src/ui_table.rs`), so the symbol must exist in each
//! platform's static library or the link fails.
//!
//! This is the GTK4 implementation, mirroring the AppKit backend's JS-side
//! marshalling (`crates/perry-ui-macos/src/drag_drop.rs`):
//!
//! * Drop destination: a `GtkDropTarget` accepting `String` / `GdkFileList` /
//!   `GFile` / uri-list types is attached to the widget. The `"drop"` signal
//!   reads the dropped `GValue`, builds a `{ text?, files?, urls? }` object,
//!   and invokes the registered callback. A plain string that parses as a
//!   `file://` / `http(s)://` URI is routed to `files` / `urls` respectively,
//!   matching the AppKit behavior where the pasteboard carries typed reps.
//!
//! * Drag source: a `GtkDragSource` is attached and its `"prepare"` signal
//!   calls whichever `widgetSetDrag*` provider closures were registered at
//!   drag-start time, building a `GdkContentProvider` from the returned
//!   strings (text -> `String`, file -> `GFile`, url -> uri string). Multiple
//!   `widgetSetDrag*` calls on one widget are unioned via a single
//!   `ContentProvider::new_union`.
//!
//! State lives in thread-local side tables keyed by widget handle (the GTK
//! objects are not `Send`/`Sync` and run on the GTK main loop, exactly like
//! the per-widget callback tables in `widgets/button.rs` / `widgets/canvas.rs`).
//!
//! NOTE: this module is **compile-unverified in this environment and in PR
//! CI** — the cross-host UI crates (including `perry-ui-gtk4`) are not built by
//! `cargo test` on macOS and are excluded from PR CI; there are no GTK4 system
//! libraries / pkg-config here. It needs a Linux/GTK build and manual testing
//! to confirm the `GdkContentProvider` / `GdkFileList` / `GValue` round-trips.

use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{DragSource, DropTarget};

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut c_void;
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(obj: *mut c_void, key: *const c_void, value: f64);
    fn js_array_alloc(capacity: u32) -> *mut c_void;
    fn js_array_push_f64(arr: *mut c_void, value: f64) -> *mut c_void;
    fn js_jsvalue_to_string(value: f64) -> *const u8;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

thread_local! {
    /// Drop callback (NaN-boxed closure) per droppable widget handle.
    static DROP_CB: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    /// Drag-source providers (NaN-boxed closures) per source widget handle.
    static DRAG_TEXT: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static DRAG_FILE: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static DRAG_URL: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    /// Widgets that already have a `GtkDragSource` attached, so repeated
    /// `widgetSetDrag*` calls add to the side tables instead of stacking
    /// controllers.
    static DRAG_SOURCE_ATTACHED: RefCell<HashMap<i64, ()>> = RefCell::new(HashMap::new());
}

/// Extract a `&str` from a runtime `StringHeader` pointer (same layout as
/// `clipboard.rs` / `widgets/button.rs`).
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

/// NaN-box a Rust string into a JS string value.
unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

/// Allocate a JS string key for `js_object_set_field_by_name`.
fn js_key(name: &[u8]) -> *const c_void {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as u32) as *const c_void }
}

/// Call a provider closure (0 args) and convert its return value to a Rust
/// string. Returns `None` for undefined / non-string returns.
unsafe fn call_provider(cb: f64) -> Option<String> {
    let p = js_nanbox_get_pointer(cb) as *const u8;
    if p.is_null() {
        return None;
    }
    let ret = js_closure_call0(p);
    let sh = js_jsvalue_to_string(ret);
    if sh.is_null() {
        None
    } else {
        Some(str_from_header(sh))
    }
}

// --- drop destination --------------------------------------------------------

/// Classify a dropped plain string. GTK delivers `file://` and `http(s)://`
/// drags either as typed `GFile` / `GdkFileList` values or, from some sources,
/// as a bare uri string — mirror the AppKit typed-rep split so the JS object
/// shape is identical across platforms.
enum DroppedText {
    Files(Vec<String>),
    Urls(Vec<String>),
    Text(String),
}

fn classify_text(s: &str) -> DroppedText {
    // A uri-list (text/uri-list) drop arrives newline-separated.
    let lines: Vec<&str> = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if !lines.is_empty() && lines.iter().all(|l| l.starts_with("file://")) {
        let files = lines
            .iter()
            .filter_map(|l| gio::File::for_uri(l).path())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        return DroppedText::Files(files);
    }
    if !lines.is_empty()
        && lines
            .iter()
            .all(|l| l.starts_with("http://") || l.starts_with("https://"))
    {
        return DroppedText::Urls(lines.iter().map(|l| l.to_string()).collect());
    }
    DroppedText::Text(s.to_string())
}

/// Build a `{ text?, files?, urls? }` object from a dropped `GValue` and invoke
/// the registered drop callback. Runs on the GTK main loop inside the `"drop"`
/// signal handler.
fn deliver_drop(handle: i64, value: &glib::Value) {
    let cb = match DROP_CB.with(|m| m.borrow().get(&handle).copied()) {
        Some(cb) => cb,
        None => return,
    };

    let mut text: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut urls: Vec<String> = Vec::new();

    // `GdkFileList` (multi-file drag) — preferred file representation.
    if let Ok(file_list) = value.get::<gdk::FileList>() {
        for file in file_list.files() {
            if let Some(path) = file.path() {
                files.push(path.to_string_lossy().into_owned());
            } else {
                // A non-local file (e.g. a remote `GFile`) has no local path;
                // surface its uri instead.
                urls.push(file.uri().to_string());
            }
        }
    } else if let Ok(file) = value.get::<gio::File>() {
        // Single `GFile`.
        if let Some(path) = file.path() {
            files.push(path.to_string_lossy().into_owned());
        } else {
            urls.push(file.uri().to_string());
        }
    } else if let Ok(s) = value.get::<String>() {
        // Plain string — split into text / files / urls by content.
        match classify_text(&s) {
            DroppedText::Files(f) => files.extend(f),
            DroppedText::Urls(u) => urls.extend(u),
            DroppedText::Text(t) => text = Some(t),
        }
    }

    unsafe {
        let obj = js_object_alloc(0, 3);
        if obj.is_null() {
            return;
        }
        if let Some(text) = text {
            js_object_set_field_by_name(obj, js_key(b"text"), nanbox_str(&text));
        }
        if !files.is_empty() {
            let mut arr = js_array_alloc(files.len() as u32);
            for f in &files {
                arr = js_array_push_f64(arr, nanbox_str(f));
            }
            js_object_set_field_by_name(obj, js_key(b"files"), js_nanbox_pointer(arr as i64));
        }
        if !urls.is_empty() {
            let mut arr = js_array_alloc(urls.len() as u32);
            for u in &urls {
                arr = js_array_push_f64(arr, nanbox_str(u));
            }
            js_object_set_field_by_name(obj, js_key(b"urls"), js_nanbox_pointer(arr as i64));
        }

        let payload = js_nanbox_pointer(obj as i64);
        let cb_ptr = js_nanbox_get_pointer(cb) as *const u8;
        js_closure_call1(cb_ptr, payload);
    }
}

// --- drag source -------------------------------------------------------------

/// Build a `GdkContentProvider` from whichever providers are registered for
/// `handle`, calling each provider closure at drag-start time. Returns `None`
/// when nothing was produced (the drag is then cancelled by GTK).
fn build_content_provider(handle: i64) -> Option<gdk::ContentProvider> {
    let mut providers: Vec<gdk::ContentProvider> = Vec::new();

    unsafe {
        if let Some(cb) = DRAG_TEXT.with(|m| m.borrow().get(&handle).copied()) {
            if let Some(s) = call_provider(cb) {
                let v = s.to_value();
                providers.push(gdk::ContentProvider::for_value(&v));
            }
        }
        if let Some(cb) = DRAG_FILE.with(|m| m.borrow().get(&handle).copied()) {
            if let Some(path) = call_provider(cb) {
                let file = gio::File::for_path(&path);
                let v = file.to_value();
                providers.push(gdk::ContentProvider::for_value(&v));
            }
        }
        if let Some(cb) = DRAG_URL.with(|m| m.borrow().get(&handle).copied()) {
            if let Some(s) = call_provider(cb) {
                // A web URL is carried as a plain string so text-accepting
                // targets (browsers, editors, address bars) take it; GTK has
                // no distinct "url" content type the way AppKit's pasteboard
                // does, so the string representation is the portable choice.
                let v = s.to_value();
                providers.push(gdk::ContentProvider::for_value(&v));
            }
        }
    }

    match providers.len() {
        0 => None,
        1 => providers.into_iter().next(),
        _ => Some(gdk::ContentProvider::new_union(&providers)),
    }
}

/// Attach a single `GtkDragSource` to `widget` whose `"prepare"` handler builds
/// content from the registered providers. Idempotent per handle.
fn ensure_drag_source(handle: i64, widget: &gtk4::Widget) {
    let already = DRAG_SOURCE_ATTACHED.with(|m| m.borrow().contains_key(&handle));
    if already {
        return;
    }
    DRAG_SOURCE_ATTACHED.with(|m| {
        m.borrow_mut().insert(handle, ());
    });

    let source = DragSource::new();
    source.set_actions(gdk::DragAction::COPY);
    source.connect_prepare(move |_src, _x, _y| build_content_provider(handle));
    widget.add_controller(source);
}

// --- FFI ---------------------------------------------------------------------

/// Register `widget` as a drop destination. `callback` (a NaN-boxed closure)
/// is invoked with a `{ text?, files?, urls? }` object describing the payload
/// when text, files, or URLs are dropped onto the widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_on_drop(widget: i64, callback: f64) {
    if callback.to_bits() == TAG_UNDEFINED {
        return;
    }
    let Some(w) = crate::widgets::get_widget(widget) else {
        return;
    };
    DROP_CB.with(|m| {
        m.borrow_mut().insert(widget, callback);
    });

    // Accept files (GdkFileList / GFile), plain text, and uri-list strings.
    let drop_target = DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
    drop_target.set_types(&[
        gdk::FileList::static_type(),
        gio::File::static_type(),
        String::static_type(),
    ]);
    drop_target.connect_drop(move |_target, value, _x, _y| {
        deliver_drop(widget, value);
        true
    });
    w.add_controller(drop_target);
}

/// Register `widget` as a drag source offering plain text. `provider` (a
/// NaN-boxed closure) returns the text payload when a drag begins.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_text(widget: i64, provider: f64) {
    let Some(w) = crate::widgets::get_widget(widget) else {
        return;
    };
    DRAG_TEXT.with(|m| {
        m.borrow_mut().insert(widget, provider);
    });
    ensure_drag_source(widget, &w);
}

/// Register `widget` as a drag source offering a file. `provider` returns the
/// absolute path of the file to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_file(widget: i64, provider: f64) {
    let Some(w) = crate::widgets::get_widget(widget) else {
        return;
    };
    DRAG_FILE.with(|m| {
        m.borrow_mut().insert(widget, provider);
    });
    ensure_drag_source(widget, &w);
}

/// Register `widget` as a drag source offering a web URL. `provider` returns
/// the URL string to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_url(widget: i64, provider: f64) {
    let Some(w) = crate::widgets::get_widget(widget) else {
        return;
    };
    DRAG_URL.with(|m| {
        m.borrow_mut().insert(widget, provider);
    });
    ensure_drag_source(widget, &w);
}
