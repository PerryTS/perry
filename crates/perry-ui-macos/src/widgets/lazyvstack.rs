use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

struct LazyVStackEntry {
    scroll_view: Retained<NSView>,
    table_view: Retained<NSView>,
    handle: i64,
    row_count: i64,
    row_height: f64,
    render_closure: f64,
}

thread_local! {
    static LAZY_VSTACKS: RefCell<Vec<LazyVStackEntry>> = const { RefCell::new(Vec::new()) };
}

fn find_entry_idx(handle: i64) -> Option<usize> {
    LAZY_VSTACKS.with(|l| l.borrow().iter().position(|e| e.handle == handle))
}

// =============================================================================
// Delegate (NSTableViewDataSource + NSTableViewDelegate, single column)
// =============================================================================

pub struct PerryLazyVStackDelegateIvars {
    pub entry_idx: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryLazyVStackDelegate"]
    #[ivars = PerryLazyVStackDelegateIvars]
    pub struct PerryLazyVStackDelegate;

    impl PerryLazyVStackDelegate {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table_view: &AnyObject) -> i64 {
            let idx = self.ivars().entry_idx.get();
            LAZY_VSTACKS.with(|l| l.borrow().get(idx).map(|e| e.row_count).unwrap_or(0))
        }

        /// Invokes the user's render closure lazily — NSTableView calls this
        /// only for rows currently in (or near) the visible rect, giving true
        /// row-level virtualization.
        #[unsafe(method(tableView:viewForTableColumn:row:))]
        fn view_for_column(
            &self,
            _table_view: &AnyObject,
            _table_column: &AnyObject,
            row: i64,
        ) -> *mut NSView {
            let idx = self.ivars().entry_idx.get();
            let render_closure = LAZY_VSTACKS.with(|l| {
                l.borrow().get(idx).map(|e| e.render_closure).unwrap_or(0.0)
            });
            if render_closure == 0.0 {
                return std::ptr::null_mut();
            }
            let render_ptr = unsafe { js_nanbox_get_pointer(render_closure) } as *const u8;
            let child_f64 = unsafe { js_closure_call1(render_ptr, row as f64) };
            let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
            if let Some(view) = super::get_widget(child_handle) {
                Retained::as_ptr(&view) as *mut NSView
            } else {
                std::ptr::null_mut()
            }
        }

        #[unsafe(method(tableView:heightOfRow:))]
        fn height_of_row(&self, _table_view: &AnyObject, _row: i64) -> f64 {
            let idx = self.ivars().entry_idx.get();
            LAZY_VSTACKS.with(|l| l.borrow().get(idx).map(|e| e.row_height).unwrap_or(44.0))
        }
    }
);

impl PerryLazyVStackDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryLazyVStackDelegateIvars {
            entry_idx: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Create a LazyVStack backed by NSScrollView + NSTableView (single column).
/// NSTableView's row recycling gives true virtualization: the render closure
/// is only invoked for rows currently within (or close to) the visible rect.
/// Default row height is 44pt; override with `set_row_height`.
pub fn create(count: i64, render_closure: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let tv_cls = AnyClass::get(c"NSTableView").unwrap();
        let table_view_obj: Retained<AnyObject> = msg_send![tv_cls, new];

        // Single column — LazyVStack has no concept of columns.
        let tc_cls = AnyClass::get(c"NSTableColumn").unwrap();
        let col_obj: Retained<AnyObject> = msg_send![tc_cls, new];
        let _: () = msg_send![&*table_view_obj, addTableColumn: &*col_obj];

        // Strip chrome so it looks like a vertical list, not a spreadsheet.
        let nil: *const AnyObject = std::ptr::null();
        let _: () = msg_send![&*table_view_obj, setHeaderView: nil];
        // NSTableViewGridNone = 0
        let _: () = msg_send![&*table_view_obj, setGridStyleMask: 0u64];
        // NSTableViewSelectionHighlightStyleNone = -1 (keeps the list look
        // instead of SwiftUI-alien blue row highlights)
        let _: () = msg_send![&*table_view_obj, setSelectionHighlightStyle: -1isize];

        // Wrap in NSScrollView
        let scroll_cls = AnyClass::get(c"NSScrollView").unwrap();
        let scroll_obj: Retained<AnyObject> = msg_send![scroll_cls, new];
        let _: () = msg_send![&*scroll_obj, setHasVerticalScroller: true];
        let _: () = msg_send![&*scroll_obj, setHasHorizontalScroller: false];
        let _: () = msg_send![&*scroll_obj, setDocumentView: &*table_view_obj];

        let table_view: Retained<NSView> = Retained::cast_unchecked(table_view_obj);
        let scroll_view: Retained<NSView> = Retained::cast_unchecked(scroll_obj);

        let handle = super::register_widget(scroll_view.clone());

        let entry_idx = LAZY_VSTACKS.with(|l| l.borrow().len());
        let delegate = PerryLazyVStackDelegate::new();
        delegate.ivars().entry_idx.set(entry_idx);

        let _: () = msg_send![&*table_view, setDataSource: &*delegate];
        let _: () = msg_send![&*table_view, setDelegate: &*delegate];

        // Leak: delegate must outlive the table view.
        std::mem::forget(delegate);

        LAZY_VSTACKS.with(|l| {
            l.borrow_mut().push(LazyVStackEntry {
                scroll_view,
                table_view,
                handle,
                row_count: count,
                row_height: 44.0,
                render_closure,
            });
        });

        handle
    }
}

/// Update the total row count and reload. NSTableView re-fetches only the
/// rows now in view; unchanged off-screen rows are never re-rendered.
pub fn update_count(handle: i64, new_count: i64) {
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = LAZY_VSTACKS.with(|l| {
            let mut stacks = l.borrow_mut();
            if let Some(entry) = stacks.get_mut(idx) {
                entry.row_count = new_count;
                Retained::as_ptr(&entry.table_view) as usize
            } else {
                0
            }
        });
        if tv_ptr != 0 {
            unsafe {
                let _: () = msg_send![tv_ptr as *const AnyObject, reloadData];
            }
        }
    }
}

/// Set a uniform row height. NSTableView requires this to be set before rows
/// are realized, and variable row heights would defeat virtualization anyway.
pub fn set_row_height(handle: i64, height: f64) {
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = LAZY_VSTACKS.with(|l| {
            let mut stacks = l.borrow_mut();
            if let Some(entry) = stacks.get_mut(idx) {
                entry.row_height = if height > 0.0 { height } else { 44.0 };
                Retained::as_ptr(&entry.table_view) as usize
            } else {
                0
            }
        });
        if tv_ptr != 0 {
            unsafe {
                let _: () = msg_send![tv_ptr as *const AnyObject, noteHeightOfRowsWithIndexesChanged: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![tv_ptr as *const AnyObject, reloadData];
            }
        }
    }
}

/// Suppress unused-field warning — the scroll_view Retained keeps the view
/// tree alive; we only access it via the registry handle.
#[allow(dead_code)]
fn _touch(entry: &LazyVStackEntry) -> *const AnyObject {
    Retained::as_ptr(&entry.scroll_view) as *const AnyObject
}

// =============================================================================
// Issue #553 — pull-to-refresh + onScrollEnd hooks
// =============================================================================
//
// macOS has no native pull-to-refresh idiom on NSScrollView, so the pull
// helpers are no-ops here (the FFI exists so cross-platform code can call
// the same symbol on macOS without a fallback branch). The onScrollEnd
// hook IS implementable on macOS: NSScrollView's clip view posts
// `NSViewBoundsDidChangeNotification` whenever the user scrolls; we
// observe that and fire the user's callback when the visible-rect's
// bottom edge gets within `threshold_items * row_height` of the content
// height. Backpressure: only fire once per threshold-cross — the next
// fire is gated until the user scrolls back up past the threshold.

struct ScrollEndState {
    closure: f64,
    threshold_items: i64,
    armed: bool, // true once user has scrolled away from the threshold zone
}

thread_local! {
    static SCROLL_END_STATES: RefCell<HashMap<i64, ScrollEndState>> = RefCell::new(HashMap::new());
    static SCROLL_END_OBSERVER_TO_HANDLE: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
}

pub struct PerryLazyVStackScrollObserverIvars {
    key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryLazyVStackScrollObserver"]
    #[ivars = PerryLazyVStackScrollObserverIvars]
    pub struct PerryLazyVStackScrollObserver;

    impl PerryLazyVStackScrollObserver {
        #[unsafe(method(boundsDidChange:))]
        fn bounds_did_change(&self, _notification: &AnyObject) {
            let key = self.ivars().key.get();
            let handle = SCROLL_END_OBSERVER_TO_HANDLE.with(|m| {
                m.borrow().get(&key).copied().unwrap_or(0)
            });
            if handle == 0 {
                return;
            }
            check_scroll_end(handle);
        }
    }
);

impl PerryLazyVStackScrollObserver {
    fn new(key: usize) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryLazyVStackScrollObserverIvars {
            key: std::cell::Cell::new(key),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn check_scroll_end(handle: i64) {
    let Some(idx) = find_entry_idx(handle) else {
        return;
    };
    let (closure, in_zone) = LAZY_VSTACKS.with(|l| {
        let stacks = l.borrow();
        let Some(entry) = stacks.get(idx) else {
            return (0.0, false);
        };
        unsafe {
            let scroll = &entry.scroll_view;
            let clip: Retained<AnyObject> = msg_send![&**scroll, contentView];
            let bounds: objc2_core_foundation::CGRect = msg_send![&*clip, bounds];
            let doc: Retained<AnyObject> = msg_send![&**scroll, documentView];
            let doc_frame: objc2_core_foundation::CGRect = msg_send![&*doc, frame];

            let threshold = SCROLL_END_STATES.with(|s| {
                s.borrow()
                    .get(&handle)
                    .map(|st| st.threshold_items)
                    .unwrap_or(5)
            });
            let threshold_px = (threshold as f64) * entry.row_height;
            let visible_bottom = bounds.origin.y + bounds.size.height;
            let in_zone = visible_bottom >= doc_frame.size.height - threshold_px;
            let closure = SCROLL_END_STATES
                .with(|s| s.borrow().get(&handle).map(|st| st.closure).unwrap_or(0.0));
            (closure, in_zone)
        }
    });
    if closure == 0.0 {
        return;
    }
    let should_fire = SCROLL_END_STATES.with(|s| {
        let mut states = s.borrow_mut();
        let Some(state) = states.get_mut(&handle) else {
            return false;
        };
        if in_zone && state.armed {
            state.armed = false;
            true
        } else if !in_zone && !state.armed {
            state.armed = true;
            false
        } else {
            false
        }
    });
    if should_fire {
        unsafe {
            let ptr = js_nanbox_get_pointer(closure) as *const u8;
            js_closure_call0(ptr);
        }
    }
}

/// macOS no-op: NSScrollView has no native pull-to-refresh. Symbol exists
/// so cross-platform code compiles without conditional branching.
pub fn set_refresh_control(_handle: i64, _callback: f64) {
    // Intentionally empty.
}

/// macOS no-op (matches `set_refresh_control`).
pub fn end_refreshing(_handle: i64) {
    // Intentionally empty.
}

/// Fire `callback` once when the user scrolls within `threshold_items`
/// rows of the bottom. Re-arms when the user scrolls back up past the
/// threshold; the runtime's `lazyvstackUpdate` (count change) does NOT
/// auto-rearm — callers should chain a manual `scroll-end-rearm` if
/// they implement non-Promise loaders, but the standard pattern is to
/// resolve the user's `Promise` after `lazyvstackUpdate` is called and
/// the callback's await completes naturally.
pub fn set_scroll_end_callback(handle: i64, callback: f64, threshold_items: i64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let Some(idx) = find_entry_idx(handle) else {
        return;
    };
    let scroll_ptr = LAZY_VSTACKS.with(|l| {
        let stacks = l.borrow();
        stacks
            .get(idx)
            .map(|e| Retained::as_ptr(&e.scroll_view) as usize)
            .unwrap_or(0)
    });
    if scroll_ptr == 0 {
        return;
    }
    SCROLL_END_STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            ScrollEndState {
                closure: callback,
                threshold_items: if threshold_items > 0 {
                    threshold_items
                } else {
                    5
                },
                armed: true,
            },
        );
    });
    unsafe {
        let scroll = scroll_ptr as *const AnyObject;
        let clip: Retained<AnyObject> = msg_send![scroll, contentView];
        let _: () = msg_send![&*clip, setPostsBoundsChangedNotifications: true];

        let observer = PerryLazyVStackScrollObserver::new(0);
        let observer_addr = Retained::as_ptr(&observer) as usize;
        observer.ivars().key.set(observer_addr);
        SCROLL_END_OBSERVER_TO_HANDLE.with(|m| {
            m.borrow_mut().insert(observer_addr, handle);
        });

        let nc_cls = AnyClass::get(c"NSNotificationCenter").unwrap();
        let nc: Retained<AnyObject> = msg_send![nc_cls, defaultCenter];
        let name = NSString::from_str("NSViewBoundsDidChangeNotification");
        let sel = Sel::register(c"boundsDidChange:");
        let _: () = msg_send![
            &*nc,
            addObserver: &*observer,
            selector: sel,
            name: &*name,
            object: &*clip,
        ];
        std::mem::forget(observer);
    }
}
