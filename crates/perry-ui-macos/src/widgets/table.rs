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
}

thread_local! {
    static TABLES: RefCell<Vec<TableEntry>> = RefCell::new(Vec::new());
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
            let col: i64 = unsafe { msg_send![table_view, indexOfTableColumn: table_column] };
            if col < 0 || col >= col_count {
                return std::ptr::null_mut();
            }
            let render_ptr = unsafe { js_nanbox_get_pointer(render_closure) } as *const u8;
            let child_f64 = unsafe { js_closure_call2(render_ptr, row as f64, col as f64) };
            let child_handle = unsafe { js_nanbox_get_pointer(child_f64) };
            if let Some(view) = super::get_widget(child_handle) {
                Retained::as_ptr(&view) as *mut NSView
            } else {
                std::ptr::null_mut()
            }
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
