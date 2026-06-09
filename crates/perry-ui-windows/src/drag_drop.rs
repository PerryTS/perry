//! Win32 drag & drop for `perry/ui` (issue #4773).
//!
//! **Compile-unverified here:** this file is written on macOS where the
//! `x86_64-pc-windows-msvc` target cannot be `cargo check`ed in the sandbox
//! (the `windows` crate's Win32 bindings only compile for a Windows target).
//! The COM/Win32 code is modelled on the crate's existing OLE usage
//! (`dialog.rs`, `file_dialog.rs`) and subclass usage (`pointer.rs`,
//! `widgets/mod.rs`); it still needs an on-device build + smoke test.
//!
//! Widget-level drag/drop setters that attach behavior to an existing widget
//! handle (1-based index into the `widgets` registry → HWND via
//! `crate::widgets::get_hwnd`).
//!
//! ## Drop destination — `perry_ui_widget_on_drop`
//! Registers an [`IDropTarget`] COM object on the widget's HWND via
//! `RegisterDragDrop`. On `Drop`, the carried [`IDataObject`] is queried for
//! `CF_UNICODETEXT` → `text`, `CF_HDROP` (via `DragQueryFileW`) → `files`,
//! and the `CFSTR_INETURLW` ("UniformResourceLocatorW") format → `urls`. A
//! `{ text?, files?, urls? }` JS object is built and the callback invoked.
//! `DragEnter`/`DragOver` advertise `DROPEFFECT_COPY`.
//!
//! ## Drag source — `perry_ui_widget_set_drag_*`
//! Each setter records a provider closure in a thread-local side table keyed
//! by HWND, then subclasses the widget's HWND (same pattern as `pointer.rs`)
//! so we can intercept `WM_LBUTTONDOWN`. On left-button-down for a widget
//! that has at least one provider, we call the providers (0-arg, returning a
//! string), build an in-process [`IDataObject`] holding the requested clipboard
//! formats, and call `DoDragDrop` with an [`IDropSource`] advertising
//! `DROPEFFECT_COPY`. When no provider is registered the message is forwarded
//! to `DefSubclassProc`, so interactive controls keep working.
//!
//! ## Window-proc glue
//! Drag *source* support needs to see mouse-down on the widget, which this
//! module installs itself via `SetWindowSubclass` (no extra wiring in the
//! crate's main WndProc). Drop *destination* support is entirely
//! `RegisterDragDrop`-driven and needs no WndProc changes. `OleInitialize` is
//! called once on first use (idempotent; tolerates a prior `CoInitializeEx`
//! apartment as `RPC_E_CHANGED_MODE`/`S_FALSE`).

#[cfg(target_os = "windows")]
use std::cell::RefCell;
#[cfg(target_os = "windows")]
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "windows")]
use std::sync::Once;

// The runtime FFI and JS-marshalling helpers below are only referenced by the
// Windows drag/drop implementation; gate them so non-Windows hosts don't warn
// about unused items.
#[cfg(target_os = "windows")]
extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut perry_runtime::string::StringHeader;
    fn js_object_alloc(class_id: u32, field_count: u32)
        -> *mut perry_runtime::object::ObjectHeader;
    fn js_object_set_field_by_name(
        obj: *mut perry_runtime::object::ObjectHeader,
        key: *const perry_runtime::string::StringHeader,
        value: f64,
    );
    fn js_array_alloc(capacity: u32) -> *mut perry_runtime::array::ArrayHeader;
    fn js_array_push_f64(
        arr: *mut perry_runtime::array::ArrayHeader,
        value: f64,
    ) -> *mut perry_runtime::array::ArrayHeader;
    fn js_jsvalue_to_string(value: f64) -> *mut perry_runtime::string::StringHeader;
}

// ---------------------------------------------------------------------------
// JS-side marshalling helpers (mirror macOS drag_drop.rs).
// ---------------------------------------------------------------------------

/// Read a Rust `String` from a runtime `StringHeader` pointer (same layout as
/// `clipboard.rs` / `dialog.rs`).
#[cfg(target_os = "windows")]
fn str_from_header(ptr: *const perry_runtime::string::StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data =
            (ptr as *const u8).add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

/// NaN-box a Rust `&str` as a JS string value.
#[cfg(target_os = "windows")]
unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

/// Build a property-key `StringHeader` for `js_object_set_field_by_name`.
#[cfg(target_os = "windows")]
fn js_key(name: &[u8]) -> *const perry_runtime::string::StringHeader {
    unsafe { js_string_from_bytes(name.as_ptr(), name.len() as u32) }
}

/// Call a provider closure (0-arg) and coerce its return value to a Rust
/// `String`. Returns `None` if the closure pointer is null.
#[cfg(target_os = "windows")]
unsafe fn call_provider(cb: f64) -> Option<String> {
    let p = js_nanbox_get_pointer(cb);
    if p == 0 {
        return None;
    }
    let ret = js_closure_call0(p as *const u8);
    let sh = js_jsvalue_to_string(ret);
    if sh.is_null() {
        None
    } else {
        Some(str_from_header(sh))
    }
}

// ---------------------------------------------------------------------------
// Side tables (keyed by HWND-as-usize so the same widget's registrations
// coalesce regardless of how many times the FFI is called).
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
thread_local! {
    /// Drop callback (NaN-boxed closure) per droppable HWND.
    static DROP_CB: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// Drag-source providers (NaN-boxed closures) per source HWND.
    static DRAG_TEXT: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static DRAG_FILE: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static DRAG_URL: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// HWNDs already registered as drop targets (so we don't double-register).
    static DROP_REGISTERED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    /// HWNDs whose drag-source subclass proc is installed.
    static DRAG_SUBCLASSED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

#[cfg(target_os = "windows")]
fn has_any_drag_source(key: usize) -> bool {
    DRAG_TEXT.with(|m| m.borrow().contains_key(&key))
        || DRAG_FILE.with(|m| m.borrow().contains_key(&key))
        || DRAG_URL.with(|m| m.borrow().contains_key(&key))
}

// ===========================================================================
// Windows implementation.
// ===========================================================================

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use windows::core::{implement, PCWSTR};
    use windows::Win32::Foundation::{
        BOOL, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC,
        DV_E_TYMED, E_NOTIMPL, HGLOBAL, HWND, LPARAM, LRESULT, OLE_E_ADVISENOTSUPPORTED, POINT,
        POINTL, S_OK, WPARAM,
    };
    use windows::Win32::System::Com::{
        IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
        DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL,
    };
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, OleInitialize,
        RegisterDragDrop, ReleaseStgMedium, CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY,
        DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Shell::{DefSubclassProc, DragQueryFileW, SetWindowSubclass, HDROP};
    use windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN;

    const DRAG_SUBCLASS_ID: usize = 0xDD_4773;

    /// Ensure the OLE drag-and-drop runtime is initialized for this thread.
    /// `OleInitialize` is idempotent per thread and returns `S_FALSE` /
    /// `RPC_E_CHANGED_MODE` when an apartment already exists (the file dialogs
    /// call `CoInitializeEx(APARTMENTTHREADED)`), both of which are fine — OLE
    /// drag-and-drop only requires an STA, which APARTMENTTHREADED provides.
    fn ensure_ole_initialized() {
        static OLE_INIT: Once = Once::new();
        OLE_INIT.call_once(|| unsafe {
            let _ = OleInitialize(None);
        });
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The Windows shell URL clipboard format ("UniformResourceLocatorW").
    fn url_clipboard_format() -> u16 {
        let name = to_wide("UniformResourceLocatorW");
        unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) as u16 }
    }

    /// Allocate a movable `HGLOBAL` holding `bytes` and return the handle.
    unsafe fn hglobal_from_bytes(bytes: &[u8]) -> windows::core::Result<HGLOBAL> {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
        let dst = GlobalLock(h);
        if !dst.is_null() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
            let _ = GlobalUnlock(h);
        }
        Ok(h)
    }

    // -----------------------------------------------------------------------
    // IDataObject — minimal in-process data object for the drag SOURCE.
    // -----------------------------------------------------------------------

    /// One offered representation: (clipboard format id, raw HGLOBAL bytes).
    struct StoredFormat {
        cf: u16,
        bytes: Vec<u8>,
    }

    #[implement(IDataObject)]
    struct PerryDataObject {
        formats: Vec<StoredFormat>,
    }

    impl PerryDataObject {
        fn matches(&self, fmt: &FORMATETC) -> Option<&StoredFormat> {
            if fmt.dwAspect != DVASPECT_CONTENT.0 as u32 {
                return None;
            }
            if fmt.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                return None;
            }
            self.formats
                .iter()
                .find(|f| f.cf as u32 == fmt.cfFormat as u32)
        }
    }

    #[allow(non_snake_case)]
    impl IDataObject_Impl for PerryDataObject_Impl {
        fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            let fmt = unsafe { &*pformatetcin };
            let Some(stored) = self.matches(fmt) else {
                return Err(DV_E_FORMATETC.into());
            };
            unsafe {
                let h = hglobal_from_bytes(&stored.bytes)?;
                Ok(STGMEDIUM {
                    tymed: TYMED_HGLOBAL.0 as u32,
                    u: windows::Win32::System::Com::STGMEDIUM_0 { hGlobal: h },
                    pUnkForRelease: std::mem::ManuallyDrop::new(None),
                })
            }
        }

        fn GetDataHere(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *mut STGMEDIUM,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> windows::core::HRESULT {
            let fmt = unsafe { &*pformatetc };
            if self.matches(fmt).is_some() {
                S_OK
            } else if fmt.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                DV_E_TYMED
            } else {
                DV_E_FORMATETC
            }
        }

        fn GetCanonicalFormatEtc(
            &self,
            _pformatectin: *const FORMATETC,
            pformatetcout: *mut FORMATETC,
        ) -> windows::core::HRESULT {
            if !pformatetcout.is_null() {
                unsafe {
                    (*pformatetcout).ptd = std::ptr::null_mut();
                }
            }
            E_NOTIMPL
        }

        fn SetData(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *const STGMEDIUM,
            _frelease: BOOL,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn EnumFormatEtc(&self, _dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
            // A real enumerator is optional for in-process source data objects;
            // targets we hand off to (Explorer, editors) call QueryGetData with
            // the formats they want. Return E_NOTIMPL like many sample sources.
            Err(E_NOTIMPL.into())
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: Option<&IAdviseSink>,
        ) -> windows::core::Result<u32> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }
    }

    // -----------------------------------------------------------------------
    // IDropSource — copy-only, ends on left-button release.
    // -----------------------------------------------------------------------

    #[implement(IDropSource)]
    struct PerryDropSource;

    #[allow(non_snake_case)]
    impl IDropSource_Impl for PerryDropSource_Impl {
        fn QueryContinueDrag(
            &self,
            fescapepressed: BOOL,
            grfkeystate: MODIFIERKEYS_FLAGS,
        ) -> windows::core::HRESULT {
            if fescapepressed.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            // Left button released → drop.
            if grfkeystate.0 & MK_LBUTTON.0 == 0 {
                return DRAGDROP_S_DROP;
            }
            S_OK
        }

        fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> windows::core::HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    // -----------------------------------------------------------------------
    // IDropTarget — accepts CF_UNICODETEXT / CF_HDROP / URL, copy effect.
    // -----------------------------------------------------------------------

    #[implement(IDropTarget)]
    struct PerryDropTarget {
        /// HWND key into `DROP_CB` (so the callback can be looked up at Drop
        /// time, after any re-registration).
        hwnd_key: usize,
    }

    #[allow(non_snake_case)]
    impl IDropTarget_Impl for PerryDropTarget_Impl {
        fn DragEnter(
            &self,
            _pdataobj: Option<&IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            unsafe {
                if !pdweffect.is_null() {
                    *pdweffect = DROPEFFECT_COPY;
                }
            }
            Ok(())
        }

        fn DragOver(
            &self,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            unsafe {
                if !pdweffect.is_null() {
                    *pdweffect = DROPEFFECT_COPY;
                }
            }
            Ok(())
        }

        fn DragLeave(&self) -> windows::core::Result<()> {
            Ok(())
        }

        fn Drop(
            &self,
            pdataobj: Option<&IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            unsafe {
                if !pdweffect.is_null() {
                    *pdweffect = DROPEFFECT_COPY;
                }
            }
            let cb = DROP_CB.with(|m| m.borrow().get(&self.hwnd_key).copied());
            let Some(cb) = cb else {
                return Ok(());
            };
            let Some(data) = pdataobj else {
                return Ok(());
            };
            unsafe {
                dispatch_drop(data, cb);
            }
            Ok(())
        }
    }

    /// Read text / file / url representations off `data` and invoke `cb` with a
    /// `{ text?, files?, urls? }` object.
    unsafe fn dispatch_drop(data: &IDataObject, cb: f64) {
        let obj = js_object_alloc(0, 3);
        if obj.is_null() {
            return;
        }

        // text — CF_UNICODETEXT
        if let Some(text) = read_text(data) {
            js_object_set_field_by_name(obj, js_key(b"text"), nanbox_str(&text));
        }

        // files — CF_HDROP via DragQueryFileW
        let files = read_files(data);
        if !files.is_empty() {
            let mut arr = js_array_alloc(files.len() as u32);
            for f in &files {
                arr = js_array_push_f64(arr, nanbox_str(f));
            }
            js_object_set_field_by_name(obj, js_key(b"files"), js_nanbox_pointer(arr as i64));
        }

        // urls — UniformResourceLocatorW
        if let Some(url) = read_url(data) {
            let mut arr = js_array_alloc(1);
            arr = js_array_push_f64(arr, nanbox_str(&url));
            js_object_set_field_by_name(obj, js_key(b"urls"), js_nanbox_pointer(arr as i64));
        }

        let payload = js_nanbox_pointer(obj as i64);
        let cb_ptr = js_nanbox_get_pointer(cb);
        if cb_ptr != 0 {
            js_closure_call1(cb_ptr as *const u8, payload);
        }
    }

    fn make_formatetc(cf: u16) -> FORMATETC {
        FORMATETC {
            cfFormat: cf,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    /// Pull a UTF-16 string out of a CF-format HGLOBAL medium. The medium's
    /// payload is treated as a NUL-terminated wide string.
    unsafe fn read_wide_string(data: &IDataObject, cf: u16) -> Option<String> {
        let fmt = make_formatetc(cf);
        let medium = data.GetData(&fmt).ok()?;
        if medium.tymed != TYMED_HGLOBAL.0 as u32 {
            ReleaseStgMedium(&medium as *const _ as *mut _);
            return None;
        }
        let h = medium.u.hGlobal;
        let result = {
            let p = GlobalLock(h) as *const u16;
            if p.is_null() {
                None
            } else {
                let mut len = 0usize;
                while *p.add(len) != 0 {
                    len += 1;
                }
                let wide = std::slice::from_raw_parts(p, len);
                let s = String::from_utf16_lossy(wide);
                let _ = GlobalUnlock(h);
                Some(s)
            }
        };
        ReleaseStgMedium(&medium as *const _ as *mut _);
        result
    }

    unsafe fn read_text(data: &IDataObject) -> Option<String> {
        read_wide_string(data, CF_UNICODETEXT.0 as u16)
    }

    unsafe fn read_url(data: &IDataObject) -> Option<String> {
        read_wide_string(data, url_clipboard_format())
    }

    /// Read dropped file paths from a CF_HDROP medium via `DragQueryFileW`.
    unsafe fn read_files(data: &IDataObject) -> Vec<String> {
        let mut out = Vec::new();
        let fmt = make_formatetc(CF_HDROP.0 as u16);
        let Ok(medium) = data.GetData(&fmt) else {
            return out;
        };
        if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            let h = medium.u.hGlobal;
            let locked = GlobalLock(h);
            if !locked.is_null() {
                let hdrop = HDROP(locked);
                let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
                for i in 0..count {
                    // First call with empty buffer returns the char count.
                    let needed = DragQueryFileW(hdrop, i, None);
                    if needed == 0 {
                        continue;
                    }
                    let mut buf = vec![0u16; needed as usize + 1];
                    let written = DragQueryFileW(hdrop, i, Some(&mut buf));
                    if written > 0 {
                        out.push(String::from_utf16_lossy(&buf[..written as usize]));
                    }
                }
                let _ = GlobalUnlock(h);
            }
        }
        ReleaseStgMedium(&medium as *const _ as *mut _);
        out
    }

    // -----------------------------------------------------------------------
    // Drag source: subclass the widget HWND to intercept WM_LBUTTONDOWN.
    // -----------------------------------------------------------------------

    pub(super) fn install_drag_subclass(hwnd: HWND) {
        let key = hwnd.0 as usize;
        let already = DRAG_SUBCLASSED.with(|s| s.borrow().contains(&key));
        if already {
            return;
        }
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(drag_subclass_proc), DRAG_SUBCLASS_ID, 0);
        }
        DRAG_SUBCLASSED.with(|s| {
            s.borrow_mut().insert(key);
        });
    }

    unsafe extern "system" fn drag_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _refdata: usize,
    ) -> LRESULT {
        if msg == WM_LBUTTONDOWN {
            let key = hwnd.0 as usize;
            if has_any_drag_source(key) {
                start_drag(key);
                // Drag handled the gesture; don't forward the button-down (the
                // OLE drag loop owns the mouse until release).
                return LRESULT(0);
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// Build the data object from whatever providers are registered for `key`
    /// and run the OLE drag loop.
    fn start_drag(key: usize) {
        ensure_ole_initialized();

        let mut formats: Vec<StoredFormat> = Vec::new();

        // text → CF_UNICODETEXT (UTF-16, NUL-terminated bytes)
        if let Some(cb) = DRAG_TEXT.with(|m| m.borrow().get(&key).copied()) {
            if let Some(s) = unsafe { call_provider(cb) } {
                formats.push(StoredFormat {
                    cf: CF_UNICODETEXT.0 as u16,
                    bytes: wide_bytes(&s),
                });
            }
        }
        // file → CF_HDROP (DROPFILES header + double-NUL-terminated wide paths)
        if let Some(cb) = DRAG_FILE.with(|m| m.borrow().get(&key).copied()) {
            if let Some(path) = unsafe { call_provider(cb) } {
                formats.push(StoredFormat {
                    cf: CF_HDROP.0 as u16,
                    bytes: build_hdrop(&[path]),
                });
            }
        }
        // url → UniformResourceLocatorW (UTF-16, NUL-terminated)
        if let Some(cb) = DRAG_URL.with(|m| m.borrow().get(&key).copied()) {
            if let Some(s) = unsafe { call_provider(cb) } {
                formats.push(StoredFormat {
                    cf: url_clipboard_format(),
                    bytes: wide_bytes(&s),
                });
            }
        }

        if formats.is_empty() {
            return;
        }

        let data: IDataObject = PerryDataObject { formats }.into();
        let source: IDropSource = PerryDropSource.into();
        let mut effect = DROPEFFECT_NONE;
        unsafe {
            let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect);
        }
    }

    /// UTF-16 NUL-terminated bytes for a string (little-endian, native).
    fn wide_bytes(s: &str) -> Vec<u8> {
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let mut bytes = Vec::with_capacity(wide.len() * 2);
        for w in wide {
            bytes.extend_from_slice(&w.to_ne_bytes());
        }
        bytes
    }

    /// Build a CF_HDROP payload: a `DROPFILES` header followed by the
    /// double-NUL-terminated list of wide file paths.
    fn build_hdrop(paths: &[String]) -> Vec<u8> {
        use windows::Win32::UI::Shell::DROPFILES;
        let header_size = std::mem::size_of::<DROPFILES>();
        let mut list: Vec<u16> = Vec::new();
        for p in paths {
            list.extend(p.encode_utf16());
            list.push(0);
        }
        list.push(0); // final double-NUL terminator

        let mut out = vec![0u8; header_size + list.len() * 2];
        let df = DROPFILES {
            pFiles: header_size as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: BOOL(0),
            fWide: BOOL(1),
        };
        unsafe {
            std::ptr::copy_nonoverlapping(
                &df as *const DROPFILES as *const u8,
                out.as_mut_ptr(),
                header_size,
            );
            std::ptr::copy_nonoverlapping(
                list.as_ptr() as *const u8,
                out.as_mut_ptr().add(header_size),
                list.len() * 2,
            );
        }
        out
    }

    // -----------------------------------------------------------------------
    // Drop target registration.
    // -----------------------------------------------------------------------

    pub(super) fn register_drop_target(hwnd: HWND) {
        ensure_ole_initialized();
        let key = hwnd.0 as usize;
        let already = DROP_REGISTERED.with(|s| s.borrow().contains(&key));
        if already {
            return;
        }
        let target: IDropTarget = PerryDropTarget { hwnd_key: key }.into();
        unsafe {
            // RegisterDragDrop holds a reference to the target for the HWND's
            // lifetime; leak our local strong ref so it stays alive.
            if RegisterDragDrop(hwnd, &target).is_ok() {
                std::mem::forget(target);
                DROP_REGISTERED.with(|s| {
                    s.borrow_mut().insert(key);
                });
            }
        }
    }
}

// ===========================================================================
// FFI surface — identical symbols on every platform.
// ===========================================================================

/// Register `widget` as a drop destination. `callback` (a NaN-boxed closure)
/// is invoked with a `{ text?, files?, urls? }` object describing the payload
/// when text, files, or URLs are dropped onto the widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_on_drop(widget: i64, callback: f64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = crate::widgets::get_hwnd(widget) else {
            return;
        };
        DROP_CB.with(|m| {
            m.borrow_mut().insert(hwnd.0 as usize, callback);
        });
        imp::register_drop_target(hwnd);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (widget, callback);
    }
}

/// Register `widget` as a drag source offering plain text. `provider` (a
/// NaN-boxed closure) returns the text payload when a drag begins.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_text(widget: i64, provider: f64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = crate::widgets::get_hwnd(widget) else {
            return;
        };
        DRAG_TEXT.with(|m| {
            m.borrow_mut().insert(hwnd.0 as usize, provider);
        });
        imp::install_drag_subclass(hwnd);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (widget, provider);
    }
}

/// Register `widget` as a drag source offering a file. `provider` returns the
/// absolute path of the file to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_file(widget: i64, provider: f64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = crate::widgets::get_hwnd(widget) else {
            return;
        };
        DRAG_FILE.with(|m| {
            m.borrow_mut().insert(hwnd.0 as usize, provider);
        });
        imp::install_drag_subclass(hwnd);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (widget, provider);
    }
}

/// Register `widget` as a drag source offering a web URL. `provider` returns
/// the URL string to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_url(widget: i64, provider: f64) {
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = crate::widgets::get_hwnd(widget) else {
            return;
        };
        DRAG_URL.with(|m| {
            m.borrow_mut().insert(hwnd.0 as usize, provider);
        });
        imp::install_drag_subclass(hwnd);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (widget, provider);
    }
}
