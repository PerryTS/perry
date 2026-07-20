//! macOS AppKit drag & drop (issue #4773).
//!
//! Widget-level drag/drop setters that attach behavior to an existing widget
//! handle. AppKit dragging-destination/source messages must be implemented by
//! the view's class, so we add them to the *specific* view instance via
//! KVO-style isa-swizzling: the first time `widgetOnDrop` / `widgetSetDrag*` is
//! called on a view, we look up (or create) a dynamic subclass of the view's
//! current class — `PerryDragDrop_<ClassName>` — that implements the dragging
//! methods, then `object_setClass` the instance to it. The subclass inherits
//! all original behavior (so an `NSButton` stays a button) and the added
//! methods read per-view state from thread-local side tables keyed by the view
//! pointer (the same pattern as `widgets/canvas.rs`). This is order-independent
//! and works regardless of where the widget sits in the view hierarchy.
//!
//! Drop destination: `draggingEntered:`/`draggingUpdated:` advertise a copy
//! operation, `performDragOperation:` reads `NSPasteboard` for
//! `public.utf8-plain-text` / `public.file-url` / `public.url`, builds a
//! `{ text?, files?, urls? }` object, and invokes the callback.
//!
//! Drag source: `mouseDown:` starts an `NSDraggingSession` whose
//! `NSPasteboardItem` is populated from whichever `widgetSetDrag*` providers
//! were registered; `draggingSession:sourceOperationMaskForDraggingContext:`
//! advertises copy. When no provider is set, `mouseDown:` forwards to the
//! original class's implementation so interactive controls keep working.

use crate::ffi::js_array_push_f64;
use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Imp, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::NSView;
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSString};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_void, CString};

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(obj: *mut c_void, key: *const c_void, value: f64);
    fn js_array_alloc(capacity: u32) -> *mut c_void;
    fn js_jsvalue_to_string(value: f64) -> *const u8;
}

// NSDragOperation bits (NSDragOperationCopy = 1).
const NS_DRAG_OPERATION_NONE: usize = 0;
const NS_DRAG_OPERATION_COPY: usize = 1;

const UTI_TEXT: &str = "public.utf8-plain-text";
const UTI_FILE_URL: &str = "public.file-url";
const UTI_URL: &str = "public.url";

thread_local! {
    /// Drop callback (NaN-boxed closure) per droppable view pointer.
    static DROP_CB: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// Drag-source providers (NaN-boxed closures) per source view pointer.
    static DRAG_TEXT: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static DRAG_FILE: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static DRAG_URL: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// Original `mouseDown:` IMP per dynamic subclass pointer, for forwarding
    /// when the view is not (also) a drag source.
    static ORIG_MOUSEDOWN: RefCell<HashMap<usize, Imp>> = RefCell::new(HashMap::new());
}

/// Extract a `&str` from a runtime `StringHeader` pointer (same layout as
/// `clipboard.rs` / `button.rs`).
fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

fn js_key(name: &[u8]) -> *const c_void {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as u32) as *const c_void }
}

/// Call a provider closure and convert its return value to a Rust string.
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

// --- dynamic subclass management ---------------------------------------------

/// Look up or build the `PerryDragDrop_<orig>` subclass and isa-swizzle `view`
/// to it (idempotent).
unsafe fn ensure_swizzled(view: *mut AnyObject) {
    let cls = (*view).class();
    if cls.name().to_bytes().starts_with(b"PerryDragDrop_") {
        return; // already swizzled
    }
    let sub = get_or_create_subclass(cls);
    objc2::ffi::object_setClass(view, sub as *const AnyClass as *mut AnyClass);
}

unsafe fn get_or_create_subclass(orig: &AnyClass) -> &'static AnyClass {
    let sub_name = format!("PerryDragDrop_{}", orig.name().to_str().unwrap_or("View"));
    let c_name = CString::new(sub_name).unwrap();
    if let Some(existing) = AnyClass::get(&c_name) {
        return existing;
    }
    let mut builder =
        ClassBuilder::new(&c_name, orig).expect("PerryDragDrop subclass name unexpectedly taken");

    // Destination protocol.
    builder.add_method(
        sel!(draggingEntered:),
        dragging_op as extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
    );
    builder.add_method(
        sel!(draggingUpdated:),
        dragging_op as extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
    );
    builder.add_method(
        sel!(prepareForDragOperation:),
        prepare_for_drag as extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool,
    );
    builder.add_method(
        sel!(performDragOperation:),
        perform_drag as extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool,
    );

    // Source protocol.
    builder.add_method(
        sel!(draggingSession:sourceOperationMaskForDraggingContext:),
        source_op_mask as extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, usize) -> usize,
    );
    builder.add_method(
        sel!(mouseDown:),
        mouse_down as extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject),
    );

    let sub = builder.register();

    // Remember the original mouseDown IMP so non-source views can forward.
    if let Some(method) = orig.instance_method(sel!(mouseDown:)) {
        let imp = method.implementation();
        ORIG_MOUSEDOWN.with(|m| {
            m.borrow_mut().insert(sub as *const AnyClass as usize, imp);
        });
    }
    sub
}

// --- destination methods -----------------------------------------------------

extern "C-unwind" fn dragging_op(
    this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> usize {
    let has = DROP_CB.with(|m| m.borrow().contains_key(&(this as usize)));
    if has {
        NS_DRAG_OPERATION_COPY
    } else {
        NS_DRAG_OPERATION_NONE
    }
}

extern "C-unwind" fn prepare_for_drag(
    this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> Bool {
    let has = DROP_CB.with(|m| m.borrow().contains_key(&(this as usize)));
    Bool::new(has)
}

extern "C-unwind" fn perform_drag(this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) -> Bool {
    let cb = DROP_CB.with(|m| m.borrow().get(&(this as usize)).copied());
    let Some(cb) = cb else {
        return Bool::NO;
    };
    unsafe {
        let pasteboard: *mut AnyObject = msg_send![sender, draggingPasteboard];
        if pasteboard.is_null() {
            return Bool::NO;
        }

        let obj = js_object_alloc(0, 3);
        if obj.is_null() {
            return Bool::NO;
        }

        // text
        if let Some(text) = read_string_for_type(pasteboard, UTI_TEXT) {
            js_object_set_field_by_name(obj, js_key(b"text"), nanbox_str(&text));
        }
        // files (absolute paths of dropped file URLs)
        let files = read_file_paths(pasteboard);
        if !files.is_empty() {
            let mut arr = js_array_alloc(files.len() as u32);
            for f in &files {
                arr = js_array_push_f64(arr, nanbox_str(f));
            }
            js_object_set_field_by_name(obj, js_key(b"files"), js_nanbox_pointer(arr as i64));
        }
        // web urls
        if let Some(url) = read_string_for_type(pasteboard, UTI_URL) {
            let mut arr = js_array_alloc(1);
            arr = js_array_push_f64(arr, nanbox_str(&url));
            js_object_set_field_by_name(obj, js_key(b"urls"), js_nanbox_pointer(arr as i64));
        }

        let payload = js_nanbox_pointer(obj as i64);
        let cb_ptr = js_nanbox_get_pointer(cb) as *const u8;
        js_closure_call1(cb_ptr, payload);
    }
    Bool::YES
}

unsafe fn read_string_for_type(pasteboard: *mut AnyObject, uti: &str) -> Option<String> {
    let ns_type = NSString::from_str(uti);
    let s: *mut NSString = msg_send![pasteboard, stringForType: &*ns_type];
    if s.is_null() {
        None
    } else {
        Some((*s).to_string())
    }
}

/// Read dropped file URLs from the pasteboard as absolute paths.
unsafe fn read_file_paths(pasteboard: *mut AnyObject) -> Vec<String> {
    let mut out = Vec::new();
    // `readObjectsForClasses:[NSURL] options:nil` returns NSArray<NSURL>.
    let url_class: &AnyClass = AnyClass::get(c"NSURL").unwrap();
    let classes = NSArray::from_slice(&[url_class]);
    let nil: *const AnyObject = std::ptr::null();
    let urls: *mut AnyObject =
        msg_send![pasteboard, readObjectsForClasses: &*classes, options: nil];
    if urls.is_null() {
        return out;
    }
    let count: usize = msg_send![urls, count];
    for i in 0..count {
        let url: *mut AnyObject = msg_send![urls, objectAtIndex: i];
        if url.is_null() {
            continue;
        }
        let is_file: Bool = msg_send![url, isFileURL];
        if !is_file.as_bool() {
            continue;
        }
        let path: *mut NSString = msg_send![url, path];
        if !path.is_null() {
            out.push((*path).to_string());
        }
    }
    out
}

// --- source methods ----------------------------------------------------------

extern "C-unwind" fn source_op_mask(
    _this: *mut AnyObject,
    _cmd: Sel,
    _session: *mut AnyObject,
    _ctx: usize,
) -> usize {
    NS_DRAG_OPERATION_COPY
}

extern "C-unwind" fn mouse_down(this: *mut AnyObject, cmd: Sel, event: *mut AnyObject) {
    let key = this as usize;
    let has_source = DRAG_TEXT.with(|m| m.borrow().contains_key(&key))
        || DRAG_FILE.with(|m| m.borrow().contains_key(&key))
        || DRAG_URL.with(|m| m.borrow().contains_key(&key));

    if has_source {
        unsafe { start_drag(this, event) };
        return;
    }

    // Not a drag source — forward to the original class's mouseDown: so the
    // underlying control (button, text field, …) keeps behaving normally.
    unsafe {
        let sub = (*this).class();
        let imp =
            ORIG_MOUSEDOWN.with(|m| m.borrow().get(&(sub as *const AnyClass as usize)).copied());
        if let Some(imp) = imp {
            let f: extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) =
                std::mem::transmute(imp);
            f(this, cmd, event);
        }
    }
}

unsafe fn start_drag(view: *mut AnyObject, event: *mut AnyObject) {
    let key = view as usize;

    // Build an NSPasteboardItem with whatever representations are registered.
    let item: *mut AnyObject = msg_send![AnyClass::get(c"NSPasteboardItem").unwrap(), new];
    let mut wrote_any = false;

    if let Some(cb) = DRAG_TEXT.with(|m| m.borrow().get(&key).copied()) {
        if let Some(s) = call_provider(cb) {
            let v = NSString::from_str(&s);
            let t = NSString::from_str(UTI_TEXT);
            let _: Bool = msg_send![item, setString: &*v, forType: &*t];
            wrote_any = true;
        }
    }
    if let Some(cb) = DRAG_FILE.with(|m| m.borrow().get(&key).copied()) {
        if let Some(path) = call_provider(cb) {
            // public.file-url wants a file URL string.
            let url: *mut AnyObject = msg_send![AnyClass::get(c"NSURL").unwrap(), fileURLWithPath: &*NSString::from_str(&path)];
            if !url.is_null() {
                let abs: *mut NSString = msg_send![url, absoluteString];
                if !abs.is_null() {
                    let t = NSString::from_str(UTI_FILE_URL);
                    let _: Bool = msg_send![item, setString: &*abs, forType: &*t];
                    wrote_any = true;
                }
            }
        }
    }
    if let Some(cb) = DRAG_URL.with(|m| m.borrow().get(&key).copied()) {
        if let Some(s) = call_provider(cb) {
            let v = NSString::from_str(&s);
            let t = NSString::from_str(UTI_URL);
            let _: Bool = msg_send![item, setString: &*v, forType: &*t];
            wrote_any = true;
        }
    }

    if !wrote_any {
        return;
    }

    // NSDraggingItem from the pasteboard writer, with a snapshot of the view.
    let drag_item: *mut AnyObject = msg_send![AnyClass::get(c"NSDraggingItem").unwrap(), alloc];
    let drag_item: *mut AnyObject = msg_send![drag_item, initWithPasteboardWriter: item];
    if drag_item.is_null() {
        return;
    }
    let bounds: CGRect = msg_send![view, bounds];
    let image = snapshot_view(view, bounds);
    let _: () = msg_send![drag_item, setDraggingFrame: bounds, contents: image];

    let items = NSArray::from_slice(&[&*drag_item]);
    let _session: *mut AnyObject = msg_send![
        view,
        beginDraggingSessionWithItems: &*items,
        event: event,
        source: view
    ];
}

/// Render the view into an NSImage for the drag cursor. Returns nil on failure
/// (AppKit then drags without a preview, which is acceptable).
unsafe fn snapshot_view(view: *mut AnyObject, bounds: CGRect) -> *mut AnyObject {
    let rep: *mut AnyObject = msg_send![view, bitmapImageRepForCachingDisplayInRect: bounds];
    if rep.is_null() {
        return std::ptr::null_mut();
    }
    let _: () = msg_send![view, cacheDisplayInRect: bounds, toBitmapImageRep: rep];
    let img: *mut AnyObject = msg_send![AnyClass::get(c"NSImage").unwrap(), alloc];
    let img: *mut AnyObject = msg_send![img, initWithSize: bounds.size];
    if img.is_null() {
        return std::ptr::null_mut();
    }
    let _: () = msg_send![img, addRepresentation: rep];
    img
}

// --- FFI ---------------------------------------------------------------------

fn view_ptr(handle: i64) -> Option<*mut AnyObject> {
    let view = crate::widgets::get_widget(handle)?;
    Some(Retained::as_ptr(&view) as *mut AnyObject)
}

/// Register `widget` as a drop destination.
#[no_mangle]
pub extern "C" fn perry_ui_widget_on_drop(widget: i64, callback: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DROP_CB.with(|m| {
        m.borrow_mut().insert(ptr as usize, callback);
    });
    unsafe {
        ensure_swizzled(ptr);
        let view: &NSView = &*(ptr as *const NSView);
        let types = NSArray::from_retained_slice(&[
            NSString::from_str(UTI_TEXT),
            NSString::from_str(UTI_FILE_URL),
            NSString::from_str(UTI_URL),
        ]);
        let _: () = msg_send![view, registerForDraggedTypes: &*types];
    }
}

/// Register `widget` as a drag source offering plain text.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_text(widget: i64, provider: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DRAG_TEXT.with(|m| {
        m.borrow_mut().insert(ptr as usize, provider);
    });
    unsafe { ensure_swizzled(ptr) };
}

/// Register `widget` as a drag source offering a file path.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_file(widget: i64, provider: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DRAG_FILE.with(|m| {
        m.borrow_mut().insert(ptr as usize, provider);
    });
    unsafe { ensure_swizzled(ptr) };
}

/// Register `widget` as a drag source offering a web URL.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_url(widget: i64, provider: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DRAG_URL.with(|m| {
        m.borrow_mut().insert(ptr as usize, provider);
    });
    unsafe { ensure_swizzled(ptr) };
}
