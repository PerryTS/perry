//! PdfView widget — Win32 stub-with-state implementation.
//!
//! macOS uses PDFKit (PDFView); the Windows equivalent would be either
//! the `Windows.Data.Pdf` WinRT API (decode pages to BitmapImages) or
//! the PDFium static lib. Both are multi-day undertakings: the WinRT
//! path needs `Data_Pdf` + `Storage_StorageFile` features and async
//! page decoding plumbing; PDFium needs vendoring or a CMake build.
//!
//! v1 ships a stub-with-state shape — same FFI as macOS, real widget
//! handle backed by a STATIC control showing "[PDF: filename] page X
//! of Y". Page navigation + scale-state both update the label so user
//! code's go-to-page / zoom flows visibly take effect; the actual
//! page-pixel rendering is the deferred half. Mirrors the
//! v0.5.771-style "stubs matching the macOS shape" link-stability
//! pattern documented for the GTK4 audit.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::SS_CENTER;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

struct PdfState {
    path: Option<PathBuf>,
    page_count: i64,
    current_page: i64,
    scale: f64,
}

thread_local! {
    static PDFS: RefCell<HashMap<i64, PdfState>> = RefCell::new(HashMap::new());
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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

pub fn create(width: f64, height: f64) -> i64 {
    let control_id = alloc_control_id();
    let w = if width > 0.0 { width as i32 } else { 600 };
    let h = if height > 0.0 { height as i32 } else { 400 };

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("[no PDF loaded]");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_BORDER.0 | SS_CENTER.0),
                0,
                0,
                w,
                h,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Image, control_id);
            PDFS.with(|p| {
                p.borrow_mut().insert(
                    handle,
                    PdfState {
                        path: None,
                        page_count: 0,
                        current_page: 0,
                        scale: 1.0,
                    },
                );
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (w, h);
        let handle = register_widget(0, WidgetKind::Image, control_id);
        PDFS.with(|p| {
            p.borrow_mut().insert(
                handle,
                PdfState {
                    path: None,
                    page_count: 0,
                    current_page: 0,
                    scale: 1.0,
                },
            );
        });
        handle
    }
}

fn refresh_label(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let display = PDFS.with(|p| {
            p.borrow().get(&handle).map(|state| {
                if let Some(path) = &state.path {
                    let name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unnamed.pdf");
                    if state.page_count > 0 {
                        format!(
                            "[PDF: {} — page {}/{} @ {:.0}%]",
                            name,
                            state.current_page + 1,
                            state.page_count,
                            state.scale * 100.0
                        )
                    } else {
                        format!("[PDF: {} — failed to load]", name)
                    }
                } else {
                    "[no PDF loaded]".to_string()
                }
            })
        });
        if let Some(text) = display {
            if let Some(hwnd) = super::get_hwnd(handle) {
                let wide = to_wide(&text);
                unsafe {
                    let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

/// Load a PDF file. Returns 1 on success (file exists) or 0 otherwise.
/// Page count is read by counting the number of `/Type /Page` entries
/// in the file — a cheap text scan that works for the vast majority of
/// non-encrypted PDFs and matches what users need from
/// `pdf_view.getPageCount()` for index / nav UI. Doesn't render the
/// actual page bitmap (deferred to a follow-up).
pub fn load_file(handle: i64, path_ptr: *const u8) -> i64 {
    let path_str = str_from_header(path_ptr);
    let path = PathBuf::from(&path_str);
    let exists = path.exists();
    let page_count = if exists {
        cheap_page_count(&path).unwrap_or(0)
    } else {
        0
    };
    PDFS.with(|p| {
        if let Some(state) = p.borrow_mut().get_mut(&handle) {
            state.path = Some(path);
            state.page_count = page_count;
            state.current_page = 0;
        }
    });
    refresh_label(handle);
    if exists && page_count > 0 {
        1
    } else {
        0
    }
}

/// Best-effort page count by scanning the PDF for `/Type /Page` markers.
/// PDF spec allows split tokens (`/Type/Page`), so we strip whitespace
/// before matching. Returns None on read error.
fn cheap_page_count(path: &PathBuf) -> Option<i64> {
    let bytes = std::fs::read(path).ok()?;
    // Cap at 64 MB for the scan — anything beyond that, return 1 as a
    // conservative fallback so the user's nav UI gets a non-zero count.
    if bytes.len() > 64 * 1024 * 1024 {
        return Some(1);
    }
    let mut count = 0i64;
    let mut i = 0;
    while i + 9 < bytes.len() {
        // Match "/Type" then any whitespace then "/Page" (not /Pages).
        if &bytes[i..i + 5] == b"/Type" {
            let mut j = i + 5;
            while j < bytes.len()
                && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\r' || bytes[j] == b'\n')
            {
                j += 1;
            }
            if j + 5 <= bytes.len() && &bytes[j..j + 5] == b"/Page" {
                // Reject "/Pages" (would have a trailing 's').
                let after = if j + 5 < bytes.len() { bytes[j + 5] } else { 0 };
                if after != b's' {
                    count += 1;
                }
            }
            i = j + 5;
            continue;
        }
        i += 1;
    }
    Some(count.max(1))
}

pub fn get_page_count(handle: i64) -> i64 {
    PDFS.with(|p| p.borrow().get(&handle).map(|s| s.page_count).unwrap_or(0))
}

pub fn go_to_page(handle: i64, idx: i64) {
    PDFS.with(|p| {
        if let Some(state) = p.borrow_mut().get_mut(&handle) {
            let max = state.page_count.max(1) - 1;
            state.current_page = idx.clamp(0, max);
        }
    });
    refresh_label(handle);
}

pub fn get_current_page(handle: i64) -> i64 {
    PDFS.with(|p| {
        p.borrow()
            .get(&handle)
            .map(|s| s.current_page)
            .unwrap_or(-1)
    })
}

pub fn set_scale(handle: i64, scale: f64) {
    PDFS.with(|p| {
        if let Some(state) = p.borrow_mut().get_mut(&handle) {
            state.scale = scale.max(0.1);
        }
    });
    refresh_label(handle);
}
