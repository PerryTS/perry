//! Table widget — Win32 ListView (LVS_REPORT) backend.
//!
//! macOS uses NSTableView with view-based cells; the closest Win32
//! equivalent is `WC_LISTVIEW` with the `LVS_REPORT` style + `LVS_OWNERDATA`
//! virtual-list mode so we don't have to push every row into the control's
//! internal storage. `LVN_GETDISPINFO` is dispatched per visible cell, at
//! which point we call the user's render closure and extract the cell text.
//!
//! API surface mirrors macOS one-for-one (12 FFIs):
//! - `create(rows, cols, render)`           → ListView + headers.
//! - `set_column_header(h, col, title)`     → LVM_SETCOLUMNW header text.
//! - `set_column_width(h, col, w)`          → LVM_SETCOLUMNWIDTH.
//! - `update_row_count(h, n)`               → LVM_SETITEMCOUNT.
//! - `set_on_row_select(h, cb)`             → wired in `handle_notify` LVN_ITEMCHANGED.
//! - `get_selected_row(h)` → i64            → LVM_GETNEXTITEM with LVNI_SELECTED.
//! - `set_on_sort_change(h, cb)`            → wired in `handle_notify` LVN_COLUMNCLICK.
//! - `set_allows_multiple_selection(h, on)` → LVS_SINGLESEL toggle.
//! - `get_selected_rows_count(h)` → i64     → LVM_GETSELECTEDCOUNT.
//! - `get_selected_row_at(h, n)` → i64      → walk LVM_GETNEXTITEM(LVNI_SELECTED).
//! - `set_filter_text(h, text_ptr)`         → store on entry (passive — user reduces rows).
//! - `get_filter_text(h)` → ptr to StringHeader.
//!
//! Primitive string results use the ListView's native text path. Widget
//! results are cached per cell and their HWNDs are reparented into the
//! ListView, then positioned over `LVM_GETSUBITEMRECT` bounds. A subclass
//! keeps them aligned while the table scrolls/resizes and forwards child
//! notifications so interactive cells behave like the view-based macOS
//! implementation.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
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

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Clone)]
enum CellContent {
    Text(String),
    Widget(i64),
}

struct TableEntry {
    handle: i64,
    row_count: i64,
    col_count: i64,
    render_closure: f64,
    select_closure: f64,
    sort_closure: f64,
    sort_ascending: HashMap<i64, bool>,
    filter_text: String,
    /// Cached per-cell content. The dispinfo callback fills this on demand;
    /// rebuilt on `update_row_count`. Indexed by `(row, col)`.
    cell_cache: HashMap<(i64, i64), CellContent>,
}

thread_local! {
    static TABLES: RefCell<HashMap<i64, TableEntry>> = RefCell::new(HashMap::new());
    static TABLE_LAYOUT_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
}

#[cfg(target_os = "windows")]
const TABLE_SUBCLASS_ID: usize = 0x6616;

/// True when `handle` belongs to a table widget (vs a real tree). Used by
/// `handle_notify` to route LVN_*/TVN_* sharing the same TVN code space.
pub fn is_registered(handle: i64) -> bool {
    TABLES.with(|t| t.borrow().contains_key(&handle))
}

fn decode_widget_handle(value: f64) -> i64 {
    let bits = value.to_bits();
    match (bits >> 48) as u16 {
        // POINTER_TAG — lower 48 bits are the handle int.
        0x7FFD => (bits & 0x0000_FFFF_FFFF_FFFF) as i64,
        // INT32_TAG.
        0x7FFE => ((bits as u32) as i32) as i64,
        // Plain f64 number.
        _ => value as i64,
    }
}

/// Render a single cell by invoking the user's render closure.
fn render_cell(handle: i64, row: i64, col: i64) -> CellContent {
    let render_closure = TABLES.with(|t| {
        t.borrow()
            .get(&handle)
            .map(|e| e.render_closure)
            .unwrap_or(0.0)
    });
    if render_closure == 0.0 {
        return CellContent::Text(String::new());
    }
    let closure_ptr = unsafe { js_nanbox_get_pointer(render_closure) } as *const u8;
    if closure_ptr.is_null() {
        return CellContent::Text(String::new());
    }
    // The closure is `(row, col) => widget`. We get back a NaN-boxed widget
    // handle (POINTER_TAG'd small integer) or a string-typed widget for the
    // simplest Text-only path. The string-typed return shape (top16 ==
    // 0x7FFF) is the common case — extract the bytes directly.
    let result = unsafe { js_closure_call2(closure_ptr, row as f64, col as f64) };
    let bits = result.to_bits();
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFF {
        // STRING_TAG — pointer is the lower 48 bits.
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
        return CellContent::Text(str_from_header(ptr).to_string());
    }
    let widget_handle = decode_widget_handle(result);
    if widget_handle <= 0 {
        return CellContent::Text(String::new());
    }

    #[cfg(target_os = "windows")]
    {
        if super::get_hwnd_safe(widget_handle).is_some() {
            return CellContent::Widget(widget_handle);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = widget_handle;
    }

    CellContent::Text(String::new())
}

#[cfg(target_os = "windows")]
fn detach_widget_cells(contents: impl IntoIterator<Item = CellContent>) {
    let parking = super::get_parking_hwnd();
    for content in contents {
        if let CellContent::Widget(widget_handle) = content {
            if let Some(child_hwnd) = super::get_hwnd_safe(widget_handle) {
                unsafe {
                    let _ = ShowWindow(child_hwnd, SW_HIDE);
                    let _ = SetParent(child_hwnd, Some(parking));
                }
            }
        }
    }
}

fn drain_cell_contents(handle: i64) -> Vec<CellContent> {
    TABLES.with(|tables| {
        tables
            .borrow_mut()
            .get_mut(&handle)
            .map(|entry| {
                entry
                    .cell_cache
                    .drain()
                    .map(|(_, content)| content)
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[cfg(target_os = "windows")]
fn layout_widget_cells(handle: i64) {
    let Some(table_hwnd) = super::get_hwnd_safe(handle) else {
        return;
    };
    if TABLE_LAYOUT_IN_PROGRESS.with(|in_progress| in_progress.replace(true)) {
        return;
    }
    struct LayoutGuard;
    impl Drop for LayoutGuard {
        fn drop(&mut self) {
            TABLE_LAYOUT_IN_PROGRESS.with(|in_progress| in_progress.set(false));
        }
    }
    let _guard = LayoutGuard;

    let cells: Vec<(i64, i64, i64)> = TABLES.with(|tables| {
        tables
            .borrow()
            .get(&handle)
            .map(|entry| {
                entry
                    .cell_cache
                    .iter()
                    .filter_map(|(&(row, col), content)| match content {
                        CellContent::Widget(widget) => Some((row, col, *widget)),
                        CellContent::Text(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    });

    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(table_hwnd, &mut client);
    }
    let mut content_top = client.top;
    unsafe {
        let header =
            HWND(SendMessageW(table_hwnd, LVM_GETHEADER, None, None).0 as *mut std::ffi::c_void);
        if !header.0.is_null() {
            let mut header_rect = RECT::default();
            if GetWindowRect(header, &mut header_rect).is_ok() {
                let mut header_bottom = POINT {
                    x: header_rect.right,
                    y: header_rect.bottom,
                };
                let _ = ScreenToClient(table_hwnd, &mut header_bottom);
                content_top = content_top.max(header_bottom.y);
            }
        }
    }
    for (row, col, widget_handle) in cells {
        let Some(child_hwnd) = super::get_hwnd_safe(widget_handle) else {
            continue;
        };
        if child_hwnd == table_hwnd {
            continue;
        }

        let mut cell_rect = RECT {
            left: LVIR_BOUNDS as i32,
            top: col as i32,
            ..Default::default()
        };
        let found = unsafe {
            SendMessageW(
                table_hwnd,
                LVM_GETSUBITEMRECT,
                Some(WPARAM(row as usize)),
                Some(LPARAM(&mut cell_rect as *mut _ as isize)),
            )
            .0 != 0
        };
        let visible = found
            && cell_rect.right > client.left
            && cell_rect.left < client.right
            && cell_rect.bottom > content_top
            && cell_rect.top < client.bottom
            && super::get_widget_info(widget_handle)
                .map(|info| !info.hidden)
                .unwrap_or(false);
        if !visible {
            unsafe {
                let _ = ShowWindow(child_hwnd, SW_HIDE);
            }
            continue;
        }

        let x = cell_rect.left + 1;
        let y = cell_rect.top.max(content_top) + 1;
        let width = (cell_rect.right - cell_rect.left - 2).max(1);
        let height = (cell_rect.bottom - cell_rect.top.max(content_top) - 2).max(1);
        unsafe {
            if GetParent(child_hwnd).ok() != Some(table_hwnd) {
                let _ = SetParent(child_hwnd, Some(table_hwnd));
                let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                SetWindowLongW(
                    child_hwnd,
                    GWL_STYLE,
                    ((style | WS_CHILD.0 | WS_VISIBLE.0) & !WS_POPUP.0) as i32,
                );
            }
            let _ = SetWindowPos(
                child_hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            );
        }
        crate::layout::layout_widget(widget_handle, width, height);
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn table_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    if let Some(result) = crate::theme::handle_container_message(hwnd, msg, wparam, lparam) {
        return result;
    }

    let handle = refdata as i64;
    if msg == WM_NCDESTROY {
        let contents = TABLES.with(|tables| {
            tables
                .borrow_mut()
                .remove(&handle)
                .map(|entry| entry.cell_cache.into_values().collect::<Vec<_>>())
                .unwrap_or_default()
        });
        detach_widget_cells(contents);
        let _ = RemoveWindowSubclass(hwnd, Some(table_subclass_proc), TABLE_SUBCLASS_ID);
        return DefSubclassProc(hwnd, msg, wparam, lparam);
    }

    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
    if matches!(
        msg,
        WM_PAINT
            | WM_SIZE
            | WM_HSCROLL
            | WM_VSCROLL
            | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL
            | WM_NOTIFY
            | WM_WINDOWPOSCHANGED
    ) {
        layout_widget_cells(handle);
    }
    result
}

/// Create a table backed by a Win32 ListView (LVS_REPORT + LVS_OWNERDATA).
pub fn create(row_count: f64, col_count: f64, render_closure: f64) -> i64 {
    let row_count = row_count as i64;
    let col_count = col_count as i64;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("SysListView32");
        let window_text = to_wide("");
        unsafe {
            // INITCOMMONCONTROLSEX is required to load the common-controls
            // window class. Idempotent.
            let mut iccex = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_LISTVIEW_CLASSES,
            };
            let _ = InitCommonControlsEx(&mut iccex);

            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(
                    WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_TABSTOP.0
                        | WS_CLIPCHILDREN.0
                        | LVS_REPORT as u32
                        | LVS_OWNERDATA as u32
                        | LVS_SHOWSELALWAYS as u32
                        | LVS_SINGLESEL as u32,
                ),
                0,
                0,
                400,
                300,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            // LVS_EX_FULLROWSELECT + LVS_EX_GRIDLINES match the macOS look.
            SendMessageW(
                hwnd,
                LVM_SETEXTENDEDLISTVIEWSTYLE,
                Some(WPARAM(0)),
                Some(LPARAM(
                    (LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES | LVS_EX_HEADERDRAGDROP) as isize,
                )),
            );

            // Insert the columns. Default header is "Col N".
            for i in 0..col_count {
                let col_title = to_wide(&format!("Col {}", i + 1));
                let mut lvc = LVCOLUMNW {
                    mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
                    fmt: LVCFMT_LEFT,
                    cx: 120,
                    pszText: windows::core::PWSTR(col_title.as_ptr() as *mut u16),
                    cchTextMax: 0,
                    iSubItem: i as i32,
                    iImage: 0,
                    iOrder: 0,
                    cxMin: 0,
                    cxDefault: 0,
                    cxIdeal: 0,
                };
                SendMessageW(
                    hwnd,
                    LVM_INSERTCOLUMNW,
                    Some(WPARAM(i as usize)),
                    Some(LPARAM(&mut lvc as *mut _ as isize)),
                );
            }

            // Set virtual row count.
            SendMessageW(
                hwnd,
                LVM_SETITEMCOUNT,
                Some(WPARAM(row_count as usize)),
                Some(LPARAM(0)),
            );

            let table_handle = register_widget(hwnd, WidgetKind::TreeView, control_id);
            // We reuse WidgetKind::TreeView as a bucket for "ListView-like
            // controls" since adding a new variant cascades into many
            // unrelated match-arms. handle_notify routes by HWND class
            // lookup (via the shared `find_handle_by_hwnd` registry) so
            // the kind here only matters for layout — both Tree and
            // Table share the same default 200×240 size.

            TABLES.with(|t| {
                t.borrow_mut().insert(
                    table_handle,
                    TableEntry {
                        handle: table_handle,
                        row_count,
                        col_count,
                        render_closure,
                        select_closure: 0.0,
                        sort_closure: 0.0,
                        sort_ascending: HashMap::new(),
                        filter_text: String::new(),
                        cell_cache: HashMap::new(),
                    },
                );
            });
            let _ = SetWindowSubclass(
                hwnd,
                Some(table_subclass_proc),
                TABLE_SUBCLASS_ID,
                table_handle as usize,
            );

            table_handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (row_count, col_count, render_closure);
        let table_handle = register_widget(0, WidgetKind::TreeView, control_id);
        TABLES.with(|t| {
            t.borrow_mut().insert(
                table_handle,
                TableEntry {
                    handle: table_handle,
                    row_count,
                    col_count,
                    render_closure,
                    select_closure: 0.0,
                    sort_closure: 0.0,
                    sort_ascending: HashMap::new(),
                    filter_text: String::new(),
                    cell_cache: HashMap::new(),
                },
            );
        });
        table_handle
    }
}

/// Set the title of a column header.
pub fn set_column_header(handle: i64, col: i64, title_ptr: *const u8) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let title = str_from_header(title_ptr);
            let wide = to_wide(title);
            let mut lvc = LVCOLUMNW {
                mask: LVCF_TEXT,
                pszText: windows::core::PWSTR(wide.as_ptr() as *mut u16),
                cchTextMax: 0,
                iSubItem: col as i32,
                ..Default::default()
            };
            unsafe {
                SendMessageW(
                    hwnd,
                    LVM_SETCOLUMNW,
                    Some(WPARAM(col as usize)),
                    Some(LPARAM(&mut lvc as *mut _ as isize)),
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, col, title_ptr);
    }
}

/// Set the pixel width of a column.
pub fn set_column_width(handle: i64, col: i64, width: f64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                SendMessageW(
                    hwnd,
                    LVM_SETCOLUMNWIDTH,
                    Some(WPARAM(col as usize)),
                    Some(LPARAM(width as isize)),
                );
            }
            layout_widget_cells(handle);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, col, width);
    }
}

/// Update the virtual row count and force a redraw.
pub fn update_row_count(handle: i64, count: i64) {
    TABLES.with(|t| {
        if let Some(entry) = t.borrow_mut().get_mut(&handle) {
            entry.row_count = count;
        }
    });
    let old_contents = drain_cell_contents(handle);
    #[cfg(target_os = "windows")]
    {
        detach_widget_cells(old_contents);
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                SendMessageW(
                    hwnd,
                    LVM_SETITEMCOUNT,
                    Some(WPARAM(count as usize)),
                    Some(LPARAM(0)),
                );
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, old_contents);
    }
}

/// Wire the on-row-select callback. Fires from `handle_notify` LVN_ITEMCHANGED.
pub fn set_on_row_select(handle: i64, callback: f64) {
    TABLES.with(|t| {
        if let Some(entry) = t.borrow_mut().get_mut(&handle) {
            entry.select_closure = callback;
        }
    });
}

/// Return the single selected row index, or -1 if none.
pub fn get_selected_row(handle: i64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let res = SendMessageW(
                    hwnd,
                    LVM_GETNEXTITEM,
                    Some(WPARAM(usize::MAX)),
                    Some(LPARAM(LVNI_SELECTED as isize)),
                );
                return res.0 as i64;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
    -1
}

/// Wire the on-sort-change callback. Fires from `handle_notify` LVN_COLUMNCLICK.
pub fn set_on_sort_change(handle: i64, callback: f64) {
    TABLES.with(|t| {
        if let Some(entry) = t.borrow_mut().get_mut(&handle) {
            entry.sort_closure = callback;
        }
    });
}

/// Toggle multi-selection support by adding/removing LVS_SINGLESEL.
pub fn set_allows_multiple_selection(handle: i64, allow: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let new_style = if allow != 0 {
                    style & !(LVS_SINGLESEL as u32)
                } else {
                    style | LVS_SINGLESEL as u32
                };
                SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, allow);
    }
}

/// Return the count of currently selected rows.
pub fn get_selected_rows_count(handle: i64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let res =
                    SendMessageW(hwnd, LVM_GETSELECTEDCOUNT, Some(WPARAM(0)), Some(LPARAM(0)));
                return res.0 as i64;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
    0
}

/// Walk the selected-row chain and return the n-th selected index.
pub fn get_selected_row_at(handle: i64, n: i64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let mut cur: i64 = -1;
                for _ in 0..=n {
                    let res = SendMessageW(
                        hwnd,
                        LVM_GETNEXTITEM,
                        Some(WPARAM(cur as usize)),
                        Some(LPARAM(LVNI_SELECTED as isize)),
                    );
                    cur = res.0 as i64;
                    if cur < 0 {
                        return -1;
                    }
                }
                return cur;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, n);
    }
    -1
}

/// Store a passive filter string. Mirrors the macOS contract: the user's
/// TS code reads this and reduces `row_count` accordingly.
pub fn set_filter_text(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    TABLES.with(|t| {
        if let Some(entry) = t.borrow_mut().get_mut(&handle) {
            entry.filter_text = text.to_string();
        }
    });
}

/// Read back the stored filter text as a NaN-boxed StringHeader pointer.
pub fn get_filter_text(handle: i64) -> i64 {
    let text = TABLES.with(|t| {
        t.borrow()
            .get(&handle)
            .map(|e| e.filter_text.clone())
            .unwrap_or_default()
    });
    let bytes = text.as_bytes();
    let str_ptr = perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    str_ptr as i64
}

/// Dispatch LVN_GETDISPINFO — caller (handle_notify) provides the lparam.
/// Fills the requested cell text by invoking the render closure.
#[cfg(target_os = "windows")]
pub fn handle_dispinfo(handle: i64, lparam: LPARAM) {
    #[repr(C)]
    struct NmlvDispInfoW {
        hdr: super::TableNmhdr,
        item: LvitemW,
    }
    #[repr(C)]
    struct LvitemW {
        mask: u32,
        i_item: i32,
        i_sub_item: i32,
        state: u32,
        state_mask: u32,
        psz_text: *mut u16,
        cch_text_max: i32,
        i_image: i32,
        l_param: isize,
        i_indent: i32,
        i_group_id: i32,
        c_columns: u32,
        pu_columns: *mut u32,
        pi_col_fmt: *mut i32,
        i_group: i32,
    }

    if lparam.0 == 0 {
        return;
    }
    let info = unsafe { &mut *(lparam.0 as *mut NmlvDispInfoW) };
    if info.item.mask & LVIF_TEXT.0 == 0 {
        return;
    }
    let row = info.item.i_item as i64;
    let col = info.item.i_sub_item as i64;

    // Render or pull from cache.
    let cached = TABLES.with(|t| {
        t.borrow()
            .get(&handle)
            .and_then(|e| e.cell_cache.get(&(row, col)).cloned())
    });
    let content = match cached {
        Some(content) => content,
        None => {
            let content = render_cell(handle, row, col);
            TABLES.with(|t| {
                if let Some(entry) = t.borrow_mut().get_mut(&handle) {
                    entry.cell_cache.insert((row, col), content.clone());
                }
            });
            content
        }
    };
    let text = match content {
        CellContent::Text(text) => text,
        CellContent::Widget(_) => {
            layout_widget_cells(handle);
            String::new()
        }
    };

    if info.item.psz_text.is_null() || info.item.cch_text_max <= 0 {
        return;
    }
    let cap = info.item.cch_text_max as usize;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let copy_len = wide.len().min(cap.saturating_sub(1));
    unsafe {
        let dst = std::slice::from_raw_parts_mut(info.item.psz_text, cap);
        for i in 0..copy_len {
            dst[i] = wide[i];
        }
        dst[copy_len] = 0;
    }
}

/// Dispatch LVN_ITEMCHANGED — fire the on-row-select callback.
#[cfg(target_os = "windows")]
pub fn handle_itemchanged(handle: i64, lparam: LPARAM) {
    #[repr(C)]
    struct NmlistView {
        hdr: super::TableNmhdr,
        i_item: i32,
        i_sub_item: i32,
        u_new_state: u32,
        u_old_state: u32,
        u_changed: u32,
        pt_action: POINT,
        l_param: isize,
    }

    if lparam.0 == 0 {
        return;
    }
    let info = unsafe { &*(lparam.0 as *const NmlistView) };
    // LVIF_STATE = 0x8 — only fire for state transitions.
    if info.u_changed & 0x8 == 0 {
        return;
    }
    // 0x2 = LVIS_SELECTED. Fire only when transitioning INTO selected.
    let became_selected = (info.u_new_state & 0x2 != 0) && (info.u_old_state & 0x2 == 0);
    if !became_selected {
        return;
    }
    let select_closure = TABLES.with(|t| {
        t.borrow()
            .get(&handle)
            .map(|e| e.select_closure)
            .unwrap_or(0.0)
    });
    if select_closure == 0.0 {
        return;
    }
    let closure_ptr = unsafe { js_nanbox_get_pointer(select_closure) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    unsafe {
        js_closure_call1(closure_ptr, info.i_item as f64);
    }
}

/// Dispatch LVN_COLUMNCLICK — fire the sort-change callback. Tracks
/// per-column ascending/descending toggle so consecutive clicks flip the
/// direction, matching NSTableView's column-click semantics.
#[cfg(target_os = "windows")]
pub fn handle_columnclick(handle: i64, lparam: LPARAM) {
    #[repr(C)]
    struct NmlistView {
        hdr: super::TableNmhdr,
        i_item: i32,
        i_sub_item: i32,
        u_new_state: u32,
        u_old_state: u32,
        u_changed: u32,
        pt_action: POINT,
        l_param: isize,
    }

    if lparam.0 == 0 {
        return;
    }
    let info = unsafe { &*(lparam.0 as *const NmlistView) };
    let col = info.i_sub_item as i64;
    let (sort_closure, ascending) = TABLES.with(|t| {
        let mut tables = t.borrow_mut();
        if let Some(entry) = tables.get_mut(&handle) {
            let prev = entry.sort_ascending.get(&col).copied().unwrap_or(false);
            let next = !prev;
            entry.sort_ascending.insert(col, next);
            (entry.sort_closure, next)
        } else {
            (0.0, true)
        }
    });
    if sort_closure == 0.0 {
        return;
    }
    let closure_ptr = unsafe { js_nanbox_get_pointer(sort_closure) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    unsafe {
        js_closure_call2(closure_ptr, col as f64, if ascending { 1.0 } else { 0.0 });
    }
    // Clear the cache so next paint re-renders post-sort.
    let old_contents = drain_cell_contents(handle);
    detach_widget_cells(old_contents);
    if let Some(hwnd) = super::get_hwnd(handle) {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_widget_handles_from_runtime_number_shapes() {
        let pointer = f64::from_bits((0x7FFDu64 << 48) | 42);
        let int32 = f64::from_bits((0x7FFEu64 << 48) | u32::MAX as u64);
        assert_eq!(decode_widget_handle(pointer), 42);
        assert_eq!(decode_widget_handle(int32), -1);
        assert_eq!(decode_widget_handle(17.0), 17);
    }

    #[test]
    fn draining_cell_contents_removes_cached_widgets_and_text() {
        let handle = i64::MAX - 6616;
        TABLES.with(|tables| {
            tables.borrow_mut().insert(
                handle,
                TableEntry {
                    handle,
                    row_count: 1,
                    col_count: 2,
                    render_closure: 0.0,
                    select_closure: 0.0,
                    sort_closure: 0.0,
                    sort_ascending: HashMap::new(),
                    filter_text: String::new(),
                    cell_cache: HashMap::from([
                        ((0, 0), CellContent::Text("value".to_owned())),
                        ((0, 1), CellContent::Widget(99)),
                    ]),
                },
            );
        });
        assert_eq!(drain_cell_contents(handle).len(), 2);
        assert!(TABLES.with(|tables| tables
            .borrow()
            .get(&handle)
            .is_some_and(|entry| entry.cell_cache.is_empty())));
    }
}
