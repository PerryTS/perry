//! Issue #553 — `ImageGallery` on iOS using a horizontal paging UIScrollView.
//!
//! UIPageViewController would be the more "native" choice but its
//! data-source-driven model is painful to bridge to a flat URL list.
//! A horizontal paging UIScrollView containing UIImageViews gives the
//! right swipe behavior with much simpler bookkeeping; pinch-to-zoom
//! is left as a follow-up (each page would need its own inner zoomable
//! UIScrollView with delegate-driven viewForZooming).

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
    fn dispatch_get_global_queue(identifier: i64, flags: u64) -> *const std::ffi::c_void;
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

struct GalleryState {
    image_views: Vec<*mut AnyObject>,
    on_index_change: f64,
    current_index: i64,
    page_width: f64,
    page_height: f64,
}

thread_local! {
    static STATES: RefCell<HashMap<i64, GalleryState>> = RefCell::new(HashMap::new());
    static DELEGATE_TO_HANDLE: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
}

pub struct PerryGalleryDelegateIvars {
    key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryGalleryDelegate"]
    #[ivars = PerryGalleryDelegateIvars]
    pub struct PerryGalleryDelegate;

    impl PerryGalleryDelegate {
        #[unsafe(method(scrollViewDidEndDecelerating:))]
        fn did_end_decelerating(&self, scroll: &AnyObject) {
            let key = self.ivars().key.get();
            let handle = DELEGATE_TO_HANDLE.with(|m| m.borrow().get(&key).copied().unwrap_or(0));
            if handle == 0 { return; }
            unsafe {
                let offset: CGPoint = msg_send![scroll, contentOffset];
                let page_width = STATES.with(|s| {
                    s.borrow().get(&handle).map(|st| st.page_width).unwrap_or(320.0)
                });
                let new_index = (offset.x / page_width).round() as i64;
                let mut closure = 0.0f64;
                STATES.with(|s| {
                    if let Some(state) = s.borrow_mut().get_mut(&handle) {
                        if new_index != state.current_index {
                            state.current_index = new_index;
                            closure = state.on_index_change;
                        }
                    }
                });
                if closure != 0.0 {
                    let ptr = js_nanbox_get_pointer(closure) as *const u8;
                    js_closure_call1(ptr, new_index as f64);
                }
            }
        }
    }
);

impl PerryGalleryDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryGalleryDelegateIvars {
            key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

const PAGE_WIDTH: f64 = 320.0;
const PAGE_HEIGHT: f64 = 320.0;

pub fn create(on_index_change: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"UIScrollView").unwrap();
        let scroll: *mut AnyObject = msg_send![cls, alloc];
        let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(PAGE_WIDTH, PAGE_HEIGHT));
        let scroll: *mut AnyObject = msg_send![scroll, initWithFrame: frame];
        let _: () = msg_send![scroll, setPagingEnabled: true];
        let _: () = msg_send![scroll, setShowsHorizontalScrollIndicator: false];
        let _: () = msg_send![scroll, setShowsVerticalScrollIndicator: false];
        let _: () = msg_send![scroll, setBounces: true];

        let delegate = PerryGalleryDelegate::new();
        let key = Retained::as_ptr(&delegate) as usize;
        delegate.ivars().key.set(key);
        let _: () = msg_send![scroll, setDelegate: &*delegate];
        std::mem::forget(delegate);

        let view: Retained<UIView> = Retained::retain(scroll as *mut UIView).unwrap();
        let handle = super::register_widget(view);

        DELEGATE_TO_HANDLE.with(|m| {
            m.borrow_mut().insert(key, handle);
        });
        STATES.with(|s| {
            s.borrow_mut().insert(
                handle,
                GalleryState {
                    image_views: Vec::new(),
                    on_index_change,
                    current_index: 0,
                    page_width: PAGE_WIDTH,
                    page_height: PAGE_HEIGHT,
                },
            );
        });
        handle
    }
}

pub fn add_image(handle: i64, url_ptr: *const u8, alt_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    let alt = str_from_header(alt_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    let (page_width, page_height, count) = STATES.with(|s| {
        s.borrow()
            .get(&handle)
            .map(|st| (st.page_width, st.page_height, st.image_views.len() as i64))
            .unwrap_or((PAGE_WIDTH, PAGE_HEIGHT, 0))
    });

    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"UIImageView").unwrap();
        let iv: *mut AnyObject = msg_send![cls, alloc];
        let frame = CGRect::new(
            CGPoint::new(count as f64 * page_width, 0.0),
            CGSize::new(page_width, page_height),
        );
        let iv: *mut AnyObject = msg_send![iv, initWithFrame: frame];
        // UIViewContentModeScaleAspectFit = 1
        let _: () = msg_send![iv, setContentMode: 1i64];
        if !alt.is_empty() {
            let ns_alt = NSString::from_str(alt);
            let _: () = msg_send![iv, setAccessibilityLabel: &*ns_alt];
        }

        if !url.is_empty() {
            if url.starts_with("http://") || url.starts_with("https://") {
                load_remote(url, iv);
            } else {
                let img_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
                let ns_path = NSString::from_str(url);
                let image: *mut AnyObject = msg_send![img_cls, imageWithContentsOfFile: &*ns_path];
                if !image.is_null() {
                    let _: () = msg_send![iv, setImage: image];
                }
            }
        }

        STATES.with(|s| {
            if let Some(state) = s.borrow_mut().get_mut(&handle) {
                state.image_views.push(iv);
                if let Some(scroll) = super::get_widget(handle) {
                    let _: () = msg_send![&*scroll, addSubview: iv];
                    // Resize content area.
                    let total =
                        CGSize::new(state.image_views.len() as f64 * page_width, page_height);
                    let _: () = msg_send![&*scroll, setContentSize: total];
                }
            }
        });
    }
}

pub fn set_index(handle: i64, index: i64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let (page_width, valid) = STATES.with(|s| {
        s.borrow()
            .get(&handle)
            .map(|st| (st.page_width, (index as usize) < st.image_views.len()))
            .unwrap_or((PAGE_WIDTH, false))
    });
    if !valid {
        return;
    }
    if let Some(scroll) = super::get_widget(handle) {
        unsafe {
            let target = CGPoint::new(index as f64 * page_width, 0.0);
            let _: () = msg_send![&*scroll, setContentOffset: target, animated: true];
        }
        STATES.with(|s| {
            if let Some(state) = s.borrow_mut().get_mut(&handle) {
                state.current_index = index;
            }
        });
    }
}

fn load_remote(url: &str, image_view: *mut AnyObject) {
    let url_str = url.to_string();
    struct Pkg {
        url: String,
        view: *mut AnyObject,
    }
    // Retain the view so it survives the async hop; release on completion.
    unsafe {
        let _: *mut AnyObject = msg_send![image_view, retain];
    }
    let pkg = Box::into_raw(Box::new(Pkg {
        url: url_str,
        view: image_view,
    }));

    unsafe extern "C" fn worker(ctx: *mut std::ffi::c_void) {
        let _ = std::panic::catch_unwind(|| {
            let pkg = Box::from_raw(ctx as *mut Pkg);
            unsafe {
                let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
                let ns = NSString::from_str(&pkg.url);
                let nsurl: *mut AnyObject = msg_send![url_cls, URLWithString: &*ns];
                if nsurl.is_null() {
                    let _: () = msg_send![pkg.view, release];
                    return;
                }
                let data_cls = objc2::runtime::AnyClass::get(c"NSData").unwrap();
                let data: *mut AnyObject = msg_send![data_cls, dataWithContentsOfURL: nsurl];
                if data.is_null() {
                    let _: () = msg_send![pkg.view, release];
                    return;
                }
                let _: *mut AnyObject = msg_send![data, retain];
                struct Apply {
                    data: *mut AnyObject,
                    view: *mut AnyObject,
                }
                let apply = Box::into_raw(Box::new(Apply {
                    data,
                    view: pkg.view,
                }));
                unsafe extern "C" fn finish(ctx: *mut std::ffi::c_void) {
                    let _ = std::panic::catch_unwind(|| {
                        let p = Box::from_raw(ctx as *mut Apply);
                        unsafe {
                            let img_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
                            let img: *mut AnyObject = msg_send![img_cls, alloc];
                            let img: *mut AnyObject = msg_send![img, initWithData: p.data];
                            if !img.is_null() {
                                let _: () = msg_send![p.view, setImage: img];
                            }
                            let _: () = msg_send![p.data, release];
                            let _: () = msg_send![p.view, release];
                        }
                    });
                }
                dispatch_async_f(
                    &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                    apply as *mut std::ffi::c_void,
                    finish,
                );
            }
        });
    }

    unsafe {
        let q = dispatch_get_global_queue(0, 0);
        dispatch_async_f(q, pkg as *mut std::ffi::c_void, worker);
    }
}
