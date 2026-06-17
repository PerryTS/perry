//! UIKit drag & drop (issue #4773) — iOS / iPadOS backend.
//!
//! Widget-level drag/drop setters that attach behavior to an existing widget
//! handle. Unlike AppKit, UIKit lets any `UIView` host drag/drop via
//! `addInteraction:` — no subclassing/swizzling. We create a shared
//! `UIDragInteraction` / `UIDropInteraction` delegate (one singleton each,
//! mirroring `widgets/button.rs`'s `PerryButtonTarget` pattern), add the
//! matching interaction to the target view, and key per-view state in
//! thread-local side tables by the view pointer.
//!
//! Drop destination: the drop delegate advertises a copy operation and, on
//! `performDrop`, loads `NSString` (→ `text`) and `NSURL` (file URLs → `files`,
//! web URLs → `urls`) from the session via `loadObjectsOfClass:` and invokes
//! the callback with a `{ text?, files?, urls? }` object.
//!
//! Drag source: the drag delegate returns `UIDragItem`s built from
//! `NSItemProvider`s wrapping whichever `widgetSetDrag*` provider strings are
//! registered (text → `NSString`, file → file `NSURL`, url → web `NSURL`).
//!
//! NOTE: compile-checked for `aarch64-apple-ios` but not behaviorally verified
//! (drag gestures require a device/simulator session).

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSArray, NSString};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut c_void;
    fn js_object_set_field_by_name(obj: *mut c_void, key: *const c_void, value: f64);
    fn js_array_alloc(capacity: u32) -> *mut c_void;
    fn js_array_push_f64(arr: *mut c_void, value: f64) -> *mut c_void;
    fn js_jsvalue_to_string(value: f64) -> *const u8;
}

// UIDropOperationCopy = 2.
const UI_DROP_OPERATION_COPY: isize = 2;

thread_local! {
    static DROP_CB: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static DRAG_TEXT: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static DRAG_FILE: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static DRAG_URL: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
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

unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
    js_nanbox_string(ptr as i64)
}

fn js_key(name: &[u8]) -> *const c_void {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as i64) as *const c_void }
}

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

/// The `interaction.view` pointer is the key into the side tables.
unsafe fn interaction_view_key(interaction: *mut AnyObject) -> usize {
    let view: *mut AnyObject = msg_send![interaction, view];
    view as usize
}

// --- drop delegate -----------------------------------------------------------

/// Zero-sized ivars — the delegate carries no per-instance state (all state
/// lives in the thread-local side tables, keyed by view pointer).
pub struct DragDropIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryDropDelegate"]
    #[ivars = DragDropIvars]
    pub struct PerryDropDelegate;

    impl PerryDropDelegate {
        #[unsafe(method(dropInteraction:canHandleSession:))]
        fn can_handle(&self, interaction: *mut AnyObject, _session: *mut AnyObject) -> bool {
            let key = unsafe { interaction_view_key(interaction) };
            DROP_CB.with(|m| m.borrow().contains_key(&key))
        }

        #[unsafe(method(dropInteraction:sessionDidUpdate:))]
        fn session_did_update(
            &self,
            _interaction: *mut AnyObject,
            _session: *mut AnyObject,
        ) -> *mut AnyObject {
            // UIDropProposal(dropOperation: .copy), returned +0 (autoreleased).
            unsafe {
                let alloc: *mut AnyObject = msg_send![class!(UIDropProposal), alloc];
                let proposal: *mut AnyObject =
                    msg_send![alloc, initWithDropOperation: UI_DROP_OPERATION_COPY];
                match Retained::from_raw(proposal) {
                    Some(p) => Retained::autorelease_return(p),
                    None => std::ptr::null_mut(),
                }
            }
        }

        #[unsafe(method(dropInteraction:performDrop:))]
        fn perform_drop(&self, interaction: *mut AnyObject, session: *mut AnyObject) {
            unsafe { perform_drop_impl(interaction, session) };
        }
    }
);

impl PerryDropDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(DragDropIvars);
        unsafe { msg_send![super(this), init] }
    }
}

unsafe fn perform_drop_impl(interaction: *mut AnyObject, session: *mut AnyObject) {
    let key = interaction_view_key(interaction);
    let Some(cb) = DROP_CB.with(|m| m.borrow().get(&key).copied()) else {
        return;
    };

    // Accumulate the two async loads (strings, urls); fire the JS callback when
    // both have completed.
    struct Accum {
        text: Option<String>,
        files: Vec<String>,
        urls: Vec<String>,
        pending: u8,
        cb: f64,
    }
    let accum = Rc::new(RefCell::new(Accum {
        text: None,
        files: Vec::new(),
        urls: Vec::new(),
        pending: 2,
        cb,
    }));

    unsafe fn finish(accum: &Rc<RefCell<Accum>>) {
        let mut a = accum.borrow_mut();
        a.pending -= 1;
        if a.pending != 0 {
            return;
        }
        let obj = js_object_alloc(0, 3);
        if obj.is_null() {
            return;
        }
        if let Some(t) = &a.text {
            js_object_set_field_by_name(obj, js_key(b"text"), nanbox_str(t));
        }
        if !a.files.is_empty() {
            let mut arr = js_array_alloc(a.files.len() as u32);
            for f in &a.files {
                arr = js_array_push_f64(arr, nanbox_str(f));
            }
            js_object_set_field_by_name(obj, js_key(b"files"), js_nanbox_pointer(arr as i64));
        }
        if !a.urls.is_empty() {
            let mut arr = js_array_alloc(a.urls.len() as u32);
            for u in &a.urls {
                arr = js_array_push_f64(arr, nanbox_str(u));
            }
            js_object_set_field_by_name(obj, js_key(b"urls"), js_nanbox_pointer(arr as i64));
        }
        let payload = js_nanbox_pointer(obj as i64);
        let cb_ptr = js_nanbox_get_pointer(a.cb) as *const u8;
        js_closure_call1(cb_ptr, payload);
    }

    // text
    let a_text = accum.clone();
    let block_text = block2::RcBlock::new(move |objs: *mut AnyObject| {
        if !objs.is_null() {
            let count: usize = unsafe { msg_send![objs, count] };
            if count > 0 {
                let s: *mut NSString = unsafe { msg_send![objs, objectAtIndex: 0usize] };
                if !s.is_null() {
                    a_text.borrow_mut().text = Some(unsafe { (*s).to_string() });
                }
            }
        }
        unsafe { finish(&a_text) };
    });
    let _: *mut AnyObject =
        msg_send![session, loadObjectsOfClass: class!(NSString), completionHandler: &*block_text];

    // urls (classified into file paths vs web urls)
    let a_url = accum.clone();
    let block_url = block2::RcBlock::new(move |objs: *mut AnyObject| {
        if !objs.is_null() {
            let count: usize = unsafe { msg_send![objs, count] };
            for i in 0..count {
                let url: *mut AnyObject = unsafe { msg_send![objs, objectAtIndex: i] };
                if url.is_null() {
                    continue;
                }
                let is_file: bool = unsafe { msg_send![url, isFileURL] };
                let s: *mut NSString = if is_file {
                    unsafe { msg_send![url, path] }
                } else {
                    unsafe { msg_send![url, absoluteString] }
                };
                if !s.is_null() {
                    let rs = unsafe { (*s).to_string() };
                    if is_file {
                        a_url.borrow_mut().files.push(rs);
                    } else {
                        a_url.borrow_mut().urls.push(rs);
                    }
                }
            }
        }
        unsafe { finish(&a_url) };
    });
    let _: *mut AnyObject =
        msg_send![session, loadObjectsOfClass: class!(NSURL), completionHandler: &*block_url];
}

// --- drag delegate -----------------------------------------------------------

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryDragDelegate"]
    #[ivars = DragDropIvars]
    pub struct PerryDragDelegate;

    impl PerryDragDelegate {
        #[unsafe(method(dragInteraction:itemsForBeginningSession:))]
        fn items_for_session(
            &self,
            interaction: *mut AnyObject,
            _session: *mut AnyObject,
        ) -> *mut NSArray {
            // Returned +0 (autoreleased).
            unsafe { Retained::autorelease_return(drag_items_impl(interaction)) }
        }
    }
);

impl PerryDragDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(DragDropIvars);
        unsafe { msg_send![super(this), init] }
    }
}

unsafe fn drag_items_impl(interaction: *mut AnyObject) -> Retained<NSArray> {
    let key = interaction_view_key(interaction);
    let mut items: Vec<Retained<AnyObject>> = Vec::new();

    let mut add_item = |obj: *mut AnyObject| {
        if obj.is_null() {
            return;
        }
        let provider: *mut AnyObject = msg_send![class!(NSItemProvider), alloc];
        let provider: *mut AnyObject = msg_send![provider, initWithObject: obj];
        // Adopt the +1 from `initWithObject:`; UIDragItem retains it, so the
        // provider is released back to that retain when `_provider` drops.
        let _provider: Option<Retained<AnyObject>> = Retained::from_raw(provider);
        let item: *mut AnyObject = msg_send![class!(UIDragItem), alloc];
        let item: *mut AnyObject = msg_send![item, initWithItemProvider: provider];
        // Adopt the +1 from `initWithItemProvider:`.
        if let Some(retained) = Retained::from_raw(item) {
            items.push(retained);
        }
    };

    if let Some(cb) = DRAG_TEXT.with(|m| m.borrow().get(&key).copied()) {
        if let Some(s) = call_provider(cb) {
            let ns = NSString::from_str(&s);
            add_item(Retained::as_ptr(&ns) as *mut AnyObject);
        }
    }
    if let Some(cb) = DRAG_FILE.with(|m| m.borrow().get(&key).copied()) {
        if let Some(path) = call_provider(cb) {
            let url: *mut AnyObject =
                msg_send![class!(NSURL), fileURLWithPath: &*NSString::from_str(&path)];
            add_item(url);
        }
    }
    if let Some(cb) = DRAG_URL.with(|m| m.borrow().get(&key).copied()) {
        if let Some(s) = call_provider(cb) {
            let url: *mut AnyObject =
                msg_send![class!(NSURL), URLWithString: &*NSString::from_str(&s)];
            add_item(url);
        }
    }

    let refs: Vec<&AnyObject> = items.iter().map(|r| &**r).collect();
    NSArray::from_slice(&refs)
}

// --- shared singletons + view helpers ----------------------------------------

thread_local! {
    static DROP_DELEGATE: RefCell<Option<Retained<PerryDropDelegate>>> = RefCell::new(None);
    static DRAG_DELEGATE: RefCell<Option<Retained<PerryDragDelegate>>> = RefCell::new(None);
    /// Views that already have a drop / drag interaction installed.
    static DROP_INSTALLED: RefCell<std::collections::HashSet<usize>> =
        RefCell::new(std::collections::HashSet::new());
    static DRAG_INSTALLED: RefCell<std::collections::HashSet<usize>> =
        RefCell::new(std::collections::HashSet::new());
}

fn view_ptr(handle: i64) -> Option<*mut AnyObject> {
    let view = crate::widgets::get_widget(handle)?;
    Some(Retained::as_ptr(&view) as *mut AnyObject)
}

unsafe fn ensure_drop_interaction(view: *mut AnyObject) {
    if DROP_INSTALLED.with(|s| s.borrow().contains(&(view as usize))) {
        return;
    }
    let delegate = DROP_DELEGATE.with(|d| {
        d.borrow_mut()
            .get_or_insert_with(PerryDropDelegate::new)
            .clone()
    });
    let interaction: *mut AnyObject = msg_send![class!(UIDropInteraction), alloc];
    let interaction: *mut AnyObject = msg_send![interaction, initWithDelegate: &*delegate];
    let _: () = msg_send![view, addInteraction: interaction];
    DROP_INSTALLED.with(|s| {
        s.borrow_mut().insert(view as usize);
    });
}

unsafe fn ensure_drag_interaction(view: *mut AnyObject) {
    if DRAG_INSTALLED.with(|s| s.borrow().contains(&(view as usize))) {
        return;
    }
    let delegate = DRAG_DELEGATE.with(|d| {
        d.borrow_mut()
            .get_or_insert_with(PerryDragDelegate::new)
            .clone()
    });
    let interaction: *mut AnyObject = msg_send![class!(UIDragInteraction), alloc];
    let interaction: *mut AnyObject = msg_send![interaction, initWithDelegate: &*delegate];
    let _: () = msg_send![view, addInteraction: interaction];
    // Drag interactions require user interaction enabled on the view.
    let _: () = msg_send![view, setUserInteractionEnabled: true];
    DRAG_INSTALLED.with(|s| {
        s.borrow_mut().insert(view as usize);
    });
}

// --- FFI ---------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn perry_ui_widget_on_drop(widget: i64, callback: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DROP_CB.with(|m| {
        m.borrow_mut().insert(ptr as usize, callback);
    });
    unsafe { ensure_drop_interaction(ptr) };
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_text(widget: i64, provider: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DRAG_TEXT.with(|m| {
        m.borrow_mut().insert(ptr as usize, provider);
    });
    unsafe { ensure_drag_interaction(ptr) };
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_file(widget: i64, provider: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DRAG_FILE.with(|m| {
        m.borrow_mut().insert(ptr as usize, provider);
    });
    unsafe { ensure_drag_interaction(ptr) };
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_url(widget: i64, provider: f64) {
    let Some(ptr) = view_ptr(widget) else { return };
    DRAG_URL.with(|m| {
        m.borrow_mut().insert(ptr as usize, provider);
    });
    unsafe { ensure_drag_interaction(ptr) };
}
