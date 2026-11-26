//! Photo-library image picker (issue #552) — `PHPickerViewController`.
//!
//! `imagePickerPick(max, multiple, cb)` presents the system photo picker
//! (out of process: no photo-library permission prompt, works on iPhone and
//! iPad) and calls `cb` with an array of absolute file paths, or `[]` when
//! the user cancels.
//!
//! Each pick is copied out of the provider into
//! `NSTemporaryDirectory()/perry-picks/<n>/<name>.<ext>`: the provider's file
//! is deleted as soon as its completion block returns. PNG and GIF keep their
//! format; everything else (HEIC, RAW, …) is requested as JPEG, which the
//! provider transcodes, so web servers and browsers can use the result.
//!
//! PhotosUI is loaded with `dlopen` rather than linked: the compiler's iOS
//! link line doesn't name it, and this keeps apps that never pick photos
//! from loading it.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSObject, NSString};

extern "C" {
    fn js_closure_call1(closure: *const u8, this: perry_ffi::JsThis, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_array_alloc(capacity: u32) -> *mut std::ffi::c_void;
    fn js_array_push_f64(arr: *mut std::ffi::c_void, value: f64) -> *mut std::ffi::c_void;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut perry_runtime::string::StringHeader;
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

thread_local! {
    /// The JS callback of the pick in progress (rooted for the GC while the
    /// picker is up). A new pick replaces it; the old one gets no answer.
    static PICK_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// `picker.delegate` is weak, so the delegate lives here.
    static PICK_DELEGATE: RefCell<Option<Retained<PerryPhotoPickerDelegate>>> = const { RefCell::new(None) };
}

/// Bumped per pick: results of a superseded pick are dropped.
static PICK_SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn scan_ios_image_picker_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    PICK_CALLBACK.with(|slot| {
        if let Some(cb) = slot.borrow_mut().as_mut() {
            visitor.visit_nanbox_f64_slot(cb);
        }
    });
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryPhotoPickerDelegate"]
    pub struct PerryPhotoPickerDelegate;

    impl PerryPhotoPickerDelegate {
        /// `PHPickerViewControllerDelegate picker:didFinishPicking:` — also
        /// called (with no results) when the user cancels.
        #[unsafe(method(picker:didFinishPicking:))]
        fn did_finish_picking(&self, picker: &AnyObject, results: &AnyObject) {
            unsafe {
                let _: () = msg_send![picker, dismissViewControllerAnimated: true, completion: std::ptr::null_mut::<AnyObject>()];
                collect_results(results);
            }
        }
    }
);

impl PerryPhotoPickerDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn load_photos_ui() -> bool {
    if AnyClass::get(c"PHPickerViewController").is_some() {
        return true;
    }
    unsafe {
        libc::dlopen(
            c"/System/Library/Frameworks/PhotosUI.framework/PhotosUI".as_ptr(),
            libc::RTLD_NOW,
        );
    }
    AnyClass::get(c"PHPickerViewController").is_some()
}

pub fn pick(max_count: f64, allow_multiple: f64, callback: f64) {
    let multi = allow_multiple.to_bits() == 0x7FFC_0000_0000_0004 // TAG_TRUE
        || (!allow_multiple.is_nan() && allow_multiple != 0.0);
    // PHPicker: 0 = unlimited.
    let limit: i64 = if !multi {
        1
    } else if max_count >= 1.0 {
        max_count as i64
    } else {
        0
    };
    PICK_SEQ.fetch_add(1, Ordering::SeqCst);
    PICK_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
    unsafe {
        if !load_photos_ui() || !present_picker(limit) {
            deliver(PICK_SEQ.load(Ordering::SeqCst), Vec::new());
        }
    }
}

unsafe fn present_picker(limit: i64) -> bool {
    let (Some(config_cls), Some(picker_cls), Some(filter_cls)) = (
        AnyClass::get(c"PHPickerConfiguration"),
        AnyClass::get(c"PHPickerViewController"),
        AnyClass::get(c"PHPickerFilter"),
    ) else {
        return false;
    };
    let vc = crate::widgets::alert::topmost_view_controller();
    if vc.is_null() {
        return false;
    }
    let config: *mut AnyObject = msg_send![config_cls, alloc];
    let config: *mut AnyObject = msg_send![config, init];
    if config.is_null() {
        return false;
    }
    let _: () = msg_send![config, setSelectionLimit: limit];
    let filter: *mut AnyObject = msg_send![filter_cls, imagesFilter];
    if !filter.is_null() {
        let _: () = msg_send![config, setFilter: filter];
    }
    let picker: *mut AnyObject = msg_send![picker_cls, alloc];
    let picker: *mut AnyObject = msg_send![picker, initWithConfiguration: config];
    objc2::ffi::objc_release(config.cast());
    if picker.is_null() {
        return false;
    }
    let delegate = PerryPhotoPickerDelegate::new();
    let _: () = msg_send![picker, setDelegate: &*delegate];
    PICK_DELEGATE.with(|d| *d.borrow_mut() = Some(delegate));
    let _: () = msg_send![vc, presentViewController: picker, animated: true, completion: std::ptr::null_mut::<AnyObject>()];
    objc2::ffi::objc_release(picker.cast());
    true
}

/// Results of one pick, filled from the providers' background queues.
struct Pending {
    seq: u64,
    paths: Mutex<Vec<Option<String>>>,
    left: Mutex<usize>,
}

static PICK_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

unsafe fn collect_results(results: &AnyObject) {
    let seq = PICK_SEQ.load(Ordering::SeqCst);
    let count: usize = msg_send![results, count];
    if count == 0 {
        deliver(seq, Vec::new());
        return;
    }
    let pending = Arc::new(Pending {
        seq,
        paths: Mutex::new(vec![None; count]),
        left: Mutex::new(count),
    });
    for i in 0..count {
        let result: *mut AnyObject = msg_send![results, objectAtIndex: i];
        let provider: *mut AnyObject = if result.is_null() {
            std::ptr::null_mut()
        } else {
            msg_send![result, itemProvider]
        };
        if provider.is_null() {
            finish_one(&pending, i, None);
            continue;
        }
        // Keep PNG/GIF as they are; ask for JPEG otherwise (HEIC → JPEG).
        let (uti, ext) = if conforms(provider, "public.png") {
            ("public.png", "png")
        } else if conforms(provider, "com.compuserve.gif") {
            ("com.compuserve.gif", "gif")
        } else {
            ("public.jpeg", "jpg")
        };
        let name = {
            let n: *mut NSString = msg_send![provider, suggestedName];
            let s = if n.is_null() {
                String::new()
            } else {
                (*n).to_string()
            };
            let s: String = s
                .chars()
                .filter(|c| !matches!(c, '/' | ':' | '\0'))
                .collect();
            if s.trim().is_empty() {
                format!("photo-{}", i + 1)
            } else {
                s
            }
        };
        let p = pending.clone();
        let ext = ext.to_string();
        let block = block2::RcBlock::new(move |url: *mut AnyObject, _err: *mut AnyObject| {
            let copied = if url.is_null() {
                None
            } else {
                copy_out(url, &name, &ext)
            };
            finish_one(&p, i, copied);
        });
        let ns_uti = NSString::from_str(uti);
        let _progress: *mut AnyObject = msg_send![provider, loadFileRepresentationForTypeIdentifier: &*ns_uti, completionHandler: &*block];
    }
}

unsafe fn conforms(provider: *mut AnyObject, uti: &str) -> bool {
    let ns = NSString::from_str(uti);
    msg_send![provider, hasItemConformingToTypeIdentifier: &*ns]
}

/// Copy the provider's temporary file somewhere that outlives the block.
unsafe fn copy_out(url: *mut AnyObject, name: &str, ext: &str) -> Option<String> {
    let path: *mut NSString = msg_send![url, path];
    if path.is_null() {
        return None;
    }
    let src = (*path).to_string();
    let n = PICK_DIR_SEQ.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir()
            .join("perry-picks")
            .join(format!("{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).ok()?;
    let stem = std::path::Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let dest = dir.join(format!("{stem}.{ext}"));
    std::fs::copy(&src, &dest).ok()?;
    Some(dest.to_string_lossy().to_string())
}

fn finish_one(p: &Arc<Pending>, index: usize, path: Option<String>) {
    if let Ok(mut v) = p.paths.lock() {
        if let Some(slot) = v.get_mut(index) {
            *slot = path;
        }
    }
    let done = match p.left.lock() {
        Ok(mut left) => {
            *left = left.saturating_sub(1);
            *left == 0
        }
        Err(_) => false,
    };
    if done {
        let paths: Vec<String> = p
            .paths
            .lock()
            .map(|v| v.iter().flatten().cloned().collect())
            .unwrap_or_default();
        unsafe { deliver(p.seq, paths) };
    }
}

/// Hand the paths to the JS callback on the main thread.
unsafe fn deliver(seq: u64, paths: Vec<String>) {
    let pkg = Box::new((seq, paths));
    dispatch_async_f(
        &_dispatch_main_q as *const _ as *const std::ffi::c_void,
        Box::into_raw(pkg) as *mut std::ffi::c_void,
        deliver_trampoline,
    );
}

unsafe extern "C" fn deliver_trampoline(ctx: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let pkg = Box::from_raw(ctx as *mut (u64, Vec<String>));
        let (seq, paths) = *pkg;
        PICK_DELEGATE.with(|d| *d.borrow_mut() = None);
        if seq != PICK_SEQ.load(Ordering::SeqCst) {
            return;
        }
        let Some(callback) = PICK_CALLBACK.with(|c| c.borrow_mut().take()) else {
            return;
        };
        let mut arr = js_array_alloc(paths.len() as u32);
        for p in &paths {
            let s = js_string_from_bytes(p.as_ptr(), p.len() as u32);
            arr = js_array_push_f64(arr, js_nanbox_string(s as i64));
        }
        let cb_ptr = js_nanbox_get_pointer(callback) as *const u8;
        if !cb_ptr.is_null() {
            js_closure_call1(
                cb_ptr,
                perry_ffi::JsThis::UNDEFINED,
                js_nanbox_pointer(arr as i64),
            );
        }
    });
}
