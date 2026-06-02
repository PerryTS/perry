//! macOS PDF viewer widget (issue #516).
//!
//! Wraps `PDFView` from PDFKit. The framework is linked at the linker
//! step (see `crates/perry/src/commands/compile/link.rs`) so we
//! instantiate via raw `objc_msgSend` against the `PDFView` /
//! `PDFDocument` classes without adding a dependency.
//!
//! Out of scope this iteration: programmatic PDF generation (CGPDF
//! context drawing API), text-search highlighting, annotation editing,
//! print-friendly rendering. Filed back into #516 as follow-ups —
//! the viewer ships first since it's the higher-leverage piece.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::NSView;
use objc2_foundation::NSString;

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

/// Create a `PDFView` of the requested size. Returns 0 if PDFKit
/// isn't available at runtime.
pub fn create(width: f64, height: f64) -> i64 {
    unsafe {
        let Some(cls) = AnyClass::get(c"PDFView") else {
            return 0;
        };
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(width.max(40.0), height.max(40.0)),
        );
        let raw: *mut AnyObject = msg_send![alloc, initWithFrame: frame];
        let view: Retained<AnyObject> = match Retained::from_raw(raw) {
            Some(r) => r,
            None => return 0,
        };
        // Single-page mode by default — closer to what most users mean
        // by "show this PDF". 1 = kPDFDisplaySinglePageContinuous.
        let _: () = msg_send![&*view, setDisplayMode: 1u64];
        let _: () = msg_send![&*view, setAutoScales: true];
        let nsview: Retained<NSView> = Retained::cast_unchecked(view);
        super::register_widget(nsview)
    }
}

/// Load a PDF from a filesystem path. Returns true on success.
pub fn load_file(handle: i64, path_ptr: *const u8) -> bool {
    let path = str_from_header(path_ptr);
    if path.is_empty() {
        return false;
    }
    let Some(view) = super::get_widget(handle) else {
        return false;
    };
    unsafe {
        let Some(url_cls) = AnyClass::get(c"NSURL") else {
            return false;
        };
        let ns_path = NSString::from_str(&path);
        let url: *mut AnyObject = msg_send![url_cls, fileURLWithPath: &*ns_path];
        if url.is_null() {
            return false;
        }
        let Some(doc_cls) = AnyClass::get(c"PDFDocument") else {
            return false;
        };
        let alloc: *mut AnyObject = msg_send![doc_cls, alloc];
        let doc: *mut AnyObject = msg_send![alloc, initWithURL: url];
        if doc.is_null() {
            return false;
        }
        let _: () = msg_send![&*view, setDocument: doc];
        true
    }
}

/// Number of pages in the loaded document, 0 if none loaded.
pub fn get_page_count(handle: i64) -> i64 {
    let Some(view) = super::get_widget(handle) else {
        return 0;
    };
    unsafe {
        let doc: *mut AnyObject = msg_send![&*view, document];
        if doc.is_null() {
            return 0;
        }
        let count: usize = msg_send![doc, pageCount];
        count as i64
    }
}

/// Jump to `page_index` (0-based). Out-of-range is a no-op.
pub fn go_to_page(handle: i64, page_index: i64) {
    if page_index < 0 {
        return;
    }
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    unsafe {
        let doc: *mut AnyObject = msg_send![&*view, document];
        if doc.is_null() {
            return;
        }
        let count: usize = msg_send![doc, pageCount];
        if (page_index as usize) >= count {
            return;
        }
        let page: *mut AnyObject = msg_send![doc, pageAtIndex: page_index as usize];
        if page.is_null() {
            return;
        }
        let _: () = msg_send![&*view, goToPage: page];
    }
}

/// Get the index of the currently-displayed page (0-based), -1 if no
/// document loaded.
pub fn get_current_page(handle: i64) -> i64 {
    let Some(view) = super::get_widget(handle) else {
        return -1;
    };
    unsafe {
        let doc: *mut AnyObject = msg_send![&*view, document];
        if doc.is_null() {
            return -1;
        }
        let cur: *mut AnyObject = msg_send![&*view, currentPage];
        if cur.is_null() {
            return -1;
        }
        let idx: i64 = msg_send![doc, indexForPage: cur];
        idx
    }
}

/// Set the zoom scale factor (1.0 = 100%).
pub fn set_scale(handle: i64, scale: f64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    unsafe {
        let _: () = msg_send![&*view, setScaleFactor: scale.max(0.1)];
    }
}
