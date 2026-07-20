//! Photo-library image picker (issue #552) — macOS NSOpenPanel filtered
//! to common image UTIs. Returns an array of absolute filesystem paths.

use crate::ffi::js_array_push_f64;
use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2_app_kit::NSOpenPanel;
use objc2_foundation::{MainThreadMarker, NSArray, NSString};

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_array_alloc(capacity: u32) -> *mut std::ffi::c_void;
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

pub fn pick(max_count: f64, allow_multiple: f64, callback: f64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let allow_multi = !allow_multiple.is_nan() && allow_multiple != 0.0
        || allow_multiple.to_bits() == 0x7FFC_0000_0000_0004; // TAG_TRUE
    let max = if max_count > 0.0 {
        max_count as usize
    } else {
        0
    };

    unsafe {
        let panel: objc2::rc::Retained<NSOpenPanel> = msg_send![
            objc2::runtime::AnyClass::get(c"NSOpenPanel").unwrap(),
            openPanel
        ];
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(allow_multi);

        // Restrict to common image extensions. Keeps the impl simple and
        // avoids the deprecated NSImage.imageTypes (UTType) ladder.
        let extensions = [
            "jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "tiff", "bmp",
        ];
        let ns_strings: Vec<objc2::rc::Retained<NSString>> =
            extensions.iter().map(|s| NSString::from_str(s)).collect();
        let refs: Vec<&NSString> = ns_strings.iter().map(|s| s.as_ref()).collect();
        let array = NSArray::from_slice(&refs);
        let _: () = msg_send![&*panel, setAllowedFileTypes: &*array];

        let response: isize = msg_send![&*panel, runModal];

        // Build result array.
        let mut paths: Vec<String> = Vec::new();
        if response == 1 {
            let urls = panel.URLs();
            let len = urls.len();
            for i in 0..len {
                if max > 0 && paths.len() >= max {
                    break;
                }
                let url = &urls.objectAtIndex(i);
                let path_str: objc2::rc::Retained<NSString> = msg_send![url, path];
                paths.push(path_str.to_string());
            }
        }

        js_run_stdlib_pump();
        js_promise_run_microtasks();

        let arr = js_array_alloc(paths.len() as u32);
        let mut cur = arr;
        for p in &paths {
            let nb = nanbox_str(p);
            cur = js_array_push_f64(cur, nb);
        }
        let nb_arr = js_nanbox_pointer(cur as i64);

        let cb_ptr = js_nanbox_get_pointer(callback) as *const u8;
        js_closure_call1(cb_ptr, nb_arr);
    }
}
