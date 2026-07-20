use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

struct TableEntry {
    scroll_view: Retained<NSView>,
    table_view: Retained<NSView>,
    handle: i64,
    row_count: i64,
    col_count: i64,
    render_closure: f64,
    select_closure: f64,
    /// Issue #473 — column-sort callback invoked with (col_index, ascending).
    sort_closure: f64,
    /// Issue #473 — passive filter text. Stored on the table entry but
    /// not used to drive row visibility — the user's TS code is expected
    /// to read this and reduce its `row_count` accordingly via
    /// `tableUpdateRowCount`. Exposed via `tableGetFilterText`.
    filter_text: String,
}

thread_local! {
    static TABLES: RefCell<Vec<TableEntry>> = const { RefCell::new(Vec::new()) };
}

fn find_entry_idx(handle: i64) -> Option<usize> {
    TABLES.with(|t| t.borrow().iter().position(|e| e.handle == handle))
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

// =============================================================================
// Delegate
// =============================================================================

pub struct PerryTableDelegateIvars {
    pub entry_idx: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTableDelegate"]
    #[ivars = PerryTableDelegateIvars]
    pub struct PerryTableDelegate;

    impl PerryTableDelegate {
        /// NSTableViewDataSource: return number of rows
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table_view: &AnyObject) -> i64 {
            let idx = self.ivars().entry_idx.get();
            TABLES.with(|t| t.borrow().get(idx).map(|e| e.row_count).unwrap_or(0))
        }

        /// NSTableViewDelegate: return cell view for (row, col)
        #[unsafe(method(tableView:viewForTableColumn:row:))]
        fn view_for_column(
            &self,
            table_view: &AnyObject,
            table_column: &AnyObject,
            row: i64,
        ) -> *mut NSView {
            let idx = self.ivars().entry_idx.get();
            let (render_closure, col_count) = TABLES.with(|t| {
                t.borrow()
                    .get(idx)
                    .map(|e| (e.render_closure, e.col_count))
                    .unwrap_or((0.0, 0))
            });
            if render_closure == 0.0 {
                return std::ptr::null_mut();
            }
            // Issue #556: replace `[table_view indexOfTableColumn:tc]`
            // with manual iteration over `tableColumns`. The direct
            // selector dispatch returns false for `respondsToSelector:`
            // on a real NSTableView in this delivery path (called from
            // `_addRowViewForVisibleRow`) — objc2 / the runtime then
            // routes via `___forwarding___`, which retains the
            // forwarding context object as if it were the receiver
            // and crashes inside `__retain_OA` on an NSCFString. The
            // pre-fix string-concat repro from the issue
            // (`Text("row " + n)`) consistently triggered this; the
            // array-index counter-example masked it via timing /
            // allocator-state interaction. Manual iteration through
            // `tableColumns objectAtIndex:` uses well-known selectors
            // that always dispatch directly.
            let col: i64 = unsafe {
                let columns: *mut AnyObject = msg_send![table_view, tableColumns];
                if columns.is_null() {
                    -1
                } else {
                    let count: i64 = msg_send![columns, count];
                    let mut found: i64 = -1;
                    let mut i: i64 = 0;
                    while i < count {
                        let c: *mut AnyObject = msg_send![columns, objectAtIndex: i as usize];
                        if c == table_column as *const _ as *mut AnyObject {
                            found = i;
                            break;
                        }
                        i += 1;
                    }
                    found
                }
            };
            if col < 0 || col >= col_count {
                return std::ptr::null_mut();
            }
            let render_ptr = unsafe { js_nanbox_get_pointer(render_closure) } as *const u8;
            let child_f64 = unsafe { js_closure_call2(render_ptr, row as f64, col as f64) };
            // Issue #556: tag-validate before treating the closure
            // return as a widget handle. The closure SHOULD return a
            // Widget (POINTER_TAG = 0x7FFD), but if user code
            // accidentally returns a string or other non-widget value,
            // `js_nanbox_get_pointer` would happily extract its
            // pointer bits — which `get_widget` would then index into
            // WIDGETS, potentially returning a stale/wrong NSView.
            let bits = child_f64.to_bits();
            let tag = (bits >> 48) & 0xFFFF;
            if tag != 0x7FFD {
                return std::ptr::null_mut();
            }
            let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
            if let Some(view) = super::get_widget(child_handle) {
                Retained::as_ptr(&view) as *mut NSView
            } else {
                std::ptr::null_mut()
            }
        }

        /// NSTableViewDataSource: sort descriptors changed (issue #473).
        /// User clicks a column header; NSTableView toggles the sort
        /// descriptor for that column (asc → desc → asc) and posts this
        /// callback. We forward (col_index, ascending) to the user's
        /// `set_on_sort_change` closure.
        #[unsafe(method(tableView:sortDescriptorsDidChange:))]
        fn sort_descriptors_did_change(
            &self,
            table_view: &AnyObject,
            _old_descriptors: &AnyObject,
        ) {
            let idx = self.ivars().entry_idx.get();
            crate::catch_callback_panic("table sort callback", std::panic::AssertUnwindSafe(|| {
                let sort_closure = TABLES.with(|t| {
                    t.borrow().get(idx).map(|e| e.sort_closure).unwrap_or(0.0)
                });
                if sort_closure == 0.0 {
                    return;
                }
                unsafe {
                    let descs: *mut AnyObject = msg_send![table_view, sortDescriptors];
                    let count: usize = msg_send![descs, count];
                    if count == 0 {
                        return;
                    }
                    let first: *mut AnyObject = msg_send![descs, objectAtIndex: 0usize];
                    let key_ns: *mut AnyObject = msg_send![first, key];
                    if key_ns.is_null() {
                        return;
                    }
                    // Identifier shape from `create`: "col0", "col1", … —
                    // parse the trailing integer.
                    let utf8: *const i8 = msg_send![key_ns, UTF8String];
                    if utf8.is_null() {
                        return;
                    }
                    let cstr = std::ffi::CStr::from_ptr(utf8);
                    let key = cstr.to_string_lossy();
                    let col_index: i64 = key
                        .strip_prefix("col")
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(-1);
                    if col_index < 0 {
                        return;
                    }
                    let ascending: objc2::runtime::Bool = msg_send![first, ascending];
                    let asc_f = if ascending.as_bool() { 1.0 } else { 0.0 };
                    let closure_ptr = js_nanbox_get_pointer(sort_closure) as *const u8;
                    js_closure_call2(closure_ptr, col_index as f64, asc_f);
                }
            }));
        }

        /// NSTableViewDelegate notification: row selection changed
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, _notification: &AnyObject) {
            let idx = self.ivars().entry_idx.get();
            crate::catch_callback_panic("table selection callback", std::panic::AssertUnwindSafe(|| {
                let (select_closure, tv_ptr) = TABLES.with(|t| {
                    let tables = t.borrow();
                    if let Some(e) = tables.get(idx) {
                        (e.select_closure, Retained::as_ptr(&e.table_view) as usize)
                    } else {
                        (0.0, 0)
                    }
                });
                if select_closure == 0.0 || tv_ptr == 0 {
                    return;
                }
                let selected_row: i64 =
                    unsafe { msg_send![tv_ptr as *const AnyObject, selectedRow] };
                if selected_row >= 0 {
                    let closure_ptr =
                        unsafe { js_nanbox_get_pointer(select_closure) } as *const u8;
                    unsafe {
                        js_closure_call1(closure_ptr, selected_row as f64);
                    }
                }
            }));
        }
    }
);

impl PerryTableDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTableDelegateIvars {
            entry_idx: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Create a Table backed by NSScrollView + NSTableView.
/// row_count and col_count arrive as f64 (JS numbers) — cast to i64 internally.
/// render_closure is a NaN-boxed closure called as (row: number, col: number) => widget.
pub fn create(row_count: i64, col_count: i64, render_closure: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        // Create NSTableView
        let tv_cls = AnyClass::get(c"NSTableView").unwrap();
        let table_view_obj: Retained<AnyObject> = msg_send![tv_cls, new];

        // Add col_count columns. Use +new (= alloc+init) to avoid the init-family
        // ownership complexity; setIdentifier: assigns an identifier for auto-save.
        let tc_cls = AnyClass::get(c"NSTableColumn").unwrap();
        for i in 0..col_count {
            let col_obj: Retained<AnyObject> = msg_send![tc_cls, new];
            let id_str = NSString::from_str(&format!("col{}", i));
            let _: () = msg_send![&*col_obj, setIdentifier: &*id_str];
            let _: () = msg_send![&*table_view_obj, addTableColumn: &*col_obj];
        }

        // Wrap in NSScrollView
        let scroll_cls = AnyClass::get(c"NSScrollView").unwrap();
        let scroll_obj: Retained<AnyObject> = msg_send![scroll_cls, new];
        let _: () = msg_send![&*scroll_obj, setHasVerticalScroller: true];
        let _: () = msg_send![&*scroll_obj, setHasHorizontalScroller: true];
        let _: () = msg_send![&*scroll_obj, setDocumentView: &*table_view_obj];

        let table_view: Retained<NSView> = Retained::cast_unchecked(table_view_obj);
        let scroll_view: Retained<NSView> = Retained::cast_unchecked(scroll_obj);

        // Register scroll view as the handle
        let handle = super::register_widget(scroll_view.clone());

        // Create delegate and assign to table view
        let entry_idx = TABLES.with(|t| t.borrow().len());
        let delegate = PerryTableDelegate::new();
        delegate.ivars().entry_idx.set(entry_idx);

        let _: () = msg_send![&*table_view, setDataSource: &*delegate];
        let _: () = msg_send![&*table_view, setDelegate: &*delegate];

        // Leak delegate — it must stay alive as long as the table view exists
        std::mem::forget(delegate);

        TABLES.with(|t| {
            t.borrow_mut().push(TableEntry {
                scroll_view,
                table_view,
                handle,
                row_count,
                col_count,
                render_closure,
                select_closure: 0.0,
                sort_closure: 0.0,
                filter_text: String::new(),
            });
        });

        handle
    }
}

/// Set the header title of a column.
/// title_ptr is a StringHeader pointer (length-prefixed UTF-8 bytes).
pub fn set_column_header(handle: i64, col: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = TABLES.with(|t| {
            t.borrow()
                .get(idx)
                .map(|e| Retained::as_ptr(&e.table_view) as usize)
                .unwrap_or(0)
        });
        if tv_ptr == 0 {
            return;
        }
        unsafe {
            let tv = tv_ptr as *const AnyObject;
            let columns: Retained<AnyObject> = msg_send![tv, tableColumns];
            let count: usize = msg_send![&*columns, count];
            if (col as usize) < count {
                let tc: *mut AnyObject = msg_send![&*columns, objectAtIndex: col as usize];
                let header_cell: *mut AnyObject = msg_send![tc, headerCell];
                let ns_title = NSString::from_str(title);
                let _: () = msg_send![header_cell, setStringValue: &*ns_title];
            }
            // Redraw header
            let header_view: *mut AnyObject = msg_send![tv, headerView];
            if !header_view.is_null() {
                let _: () = msg_send![header_view, setNeedsDisplay: true];
            }
        }
    }
}

/// Set the width of a column.
pub fn set_column_width(handle: i64, col: i64, width: f64) {
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = TABLES.with(|t| {
            t.borrow()
                .get(idx)
                .map(|e| Retained::as_ptr(&e.table_view) as usize)
                .unwrap_or(0)
        });
        if tv_ptr == 0 {
            return;
        }
        unsafe {
            let tv = tv_ptr as *const AnyObject;
            let columns: Retained<AnyObject> = msg_send![tv, tableColumns];
            let count: usize = msg_send![&*columns, count];
            if (col as usize) < count {
                let tc: *mut AnyObject = msg_send![&*columns, objectAtIndex: col as usize];
                let _: () = msg_send![tc, setWidth: width];
            }
        }
    }
}

/// Update the total number of rows and reload the table view.
pub fn update_row_count(handle: i64, count: i64) {
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = TABLES.with(|t| {
            let mut tables = t.borrow_mut();
            if let Some(entry) = tables.get_mut(idx) {
                entry.row_count = count;
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

/// Register a closure to call when the selected row changes.
/// callback is a NaN-boxed closure called as (row: number) => void.
pub fn set_on_row_select(handle: i64, callback: f64) {
    if let Some(idx) = find_entry_idx(handle) {
        TABLES.with(|t| {
            let mut tables = t.borrow_mut();
            if let Some(entry) = tables.get_mut(idx) {
                entry.select_closure = callback;
            }
        });
    }
}

/// Return the index of the currently selected row, or -1 if none.
pub fn get_selected_row(handle: i64) -> i64 {
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = TABLES.with(|t| {
            t.borrow()
                .get(idx)
                .map(|e| Retained::as_ptr(&e.table_view) as usize)
                .unwrap_or(0)
        });
        if tv_ptr != 0 {
            return unsafe { msg_send![tv_ptr as *const AnyObject, selectedRow] };
        }
    }
    -1
}

// ===========================================================================
// Issue #473 — sort + filter + multi-select
// ===========================================================================

/// Register a closure to call when the user clicks a column header to
/// re-sort. Invoked as `(colIndex: number, ascending: number) => void`.
/// Installing the callback also turns on per-column sort descriptor
/// prototypes so NSTableView shows the asc/desc indicator.
pub fn set_on_sort_change(handle: i64, callback: f64) {
    let Some(idx) = find_entry_idx(handle) else {
        return;
    };
    let tv_ptr = TABLES.with(|t| {
        let mut tables = t.borrow_mut();
        if let Some(entry) = tables.get_mut(idx) {
            entry.sort_closure = callback;
            Retained::as_ptr(&entry.table_view) as usize
        } else {
            0
        }
    });
    if tv_ptr == 0 {
        return;
    }
    unsafe {
        let columns: Retained<AnyObject> = msg_send![tv_ptr as *const AnyObject, tableColumns];
        let count: usize = msg_send![&*columns, count];
        let sd_cls = AnyClass::get(c"NSSortDescriptor").unwrap();
        for i in 0..count {
            let tc: *mut AnyObject = msg_send![&*columns, objectAtIndex: i];
            let key = NSString::from_str(&format!("col{}", i));
            // alloc + initWithKey:ascending: — caller-owned, NSColumn
            // copies the prototype.
            let alloc: *mut AnyObject = msg_send![sd_cls, alloc];
            let prototype: *mut AnyObject = msg_send![
                alloc, initWithKey: &*key, ascending: true
            ];
            let _: () = msg_send![tc, setSortDescriptorPrototype: prototype];
        }
    }
}

/// Allow multi-row selection on the table.
pub fn set_allows_multiple_selection(handle: i64, allow: bool) {
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = TABLES.with(|t| {
            t.borrow()
                .get(idx)
                .map(|e| Retained::as_ptr(&e.table_view) as usize)
                .unwrap_or(0)
        });
        if tv_ptr != 0 {
            unsafe {
                let _: () =
                    msg_send![tv_ptr as *const AnyObject, setAllowsMultipleSelection: allow];
            }
        }
    }
}

/// Return how many rows are currently selected.
pub fn get_selected_rows_count(handle: i64) -> i64 {
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = TABLES.with(|t| {
            t.borrow()
                .get(idx)
                .map(|e| Retained::as_ptr(&e.table_view) as usize)
                .unwrap_or(0)
        });
        if tv_ptr != 0 {
            unsafe {
                let indexes: *mut AnyObject =
                    msg_send![tv_ptr as *const AnyObject, selectedRowIndexes];
                if !indexes.is_null() {
                    let count: usize = msg_send![indexes, count];
                    return count as i64;
                }
            }
        }
    }
    0
}

/// Return the n-th selected row index (0-based n), or -1 when out of
/// bounds. Iterate from 0 to `get_selected_rows_count(handle) - 1`.
pub fn get_selected_row_at(handle: i64, n: i64) -> i64 {
    if n < 0 {
        return -1;
    }
    if let Some(idx) = find_entry_idx(handle) {
        let tv_ptr = TABLES.with(|t| {
            t.borrow()
                .get(idx)
                .map(|e| Retained::as_ptr(&e.table_view) as usize)
                .unwrap_or(0)
        });
        if tv_ptr != 0 {
            unsafe {
                let indexes: *mut AnyObject =
                    msg_send![tv_ptr as *const AnyObject, selectedRowIndexes];
                if indexes.is_null() {
                    return -1;
                }
                let count: usize = msg_send![indexes, count];
                if n as usize >= count {
                    return -1;
                }
                // NSIndexSet iteration: firstIndex, then indexGreaterThanIndex:
                let mut current: i64 = msg_send![indexes, firstIndex];
                let mut k: i64 = 0;
                while k < n {
                    current = msg_send![indexes, indexGreaterThanIndex: current];
                    k += 1;
                }
                return current;
            }
        }
    }
    -1
}

/// Set the table's filter text. Passive — the user's TS code reads it
/// back via `tableGetFilterText` and adjusts `tableUpdateRowCount`
/// accordingly. (Active row hiding stays the user's responsibility so
/// they can drive it from any reactive store.)
pub fn set_filter_text(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr).to_string();
    if let Some(idx) = find_entry_idx(handle) {
        TABLES.with(|t| {
            if let Some(entry) = t.borrow_mut().get_mut(idx) {
                entry.filter_text = text;
            }
        });
    }
}

/// Get the table's filter text. Returns a pointer to a `StringHeader` —
/// the FFI wrapper casts to `i64` and the dispatch table's `ReturnKind::Str`
/// arranges NaN-boxing on the codegen side. Pinned to survive GC until the
/// caller consumes it (mirrors `textfield::get_string_value`).
pub fn get_filter_text(handle: i64) -> *const u8 {
    let text = if let Some(idx) = find_entry_idx(handle) {
        TABLES.with(|t| {
            t.borrow()
                .get(idx)
                .map(|e| e.filter_text.clone())
                .unwrap_or_default()
        })
    } else {
        String::new()
    };
    let bytes = text.as_bytes();
    unsafe {
        let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        // Pin the GC allocation: GcHeader sits at ptr-8, gc_flags at offset 1.
        let gc_flags_ptr = (ptr as *mut u8).sub(8).add(1);
        *gc_flags_ptr |= 0x04; // GC_FLAG_PINNED
        ptr as *const u8
    }
}
