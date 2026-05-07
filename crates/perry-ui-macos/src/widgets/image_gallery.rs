//! Issue #553 — `ImageGallery` (swipeable carousel of images).
//!
//! macOS doesn't have a native page-view controller in AppKit (NSPageController
//! comes close but its API requires identifier-based content lookup which
//! makes a flat URL list awkward). Here we build the gallery as a horizontal
//! NSScrollView containing an NSStackView of NSImageViews, with snap-to-page
//! enabled. The user swipes with two fingers on a trackpad or scrolls with
//! a mouse wheel; programmatic page jumps are supported via `set_index`.
//!
//! Image source: local path or remote URL. Local paths load synchronously
//! via `+[NSImage imageWithContentsOfFile:]`; remote URLs spin up a
//! detached NSURLSession data task.

use crate::string_header::StringHeader;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

struct GalleryState {
    scroll_view: Retained<NSView>,
    stack: Retained<AnyObject>,
    image_views: Vec<Retained<AnyObject>>,
    on_index_change: f64,
    current_index: i64,
}

thread_local! {
    static GALLERIES: RefCell<HashMap<i64, GalleryState>> = RefCell::new(HashMap::new());
    static OBSERVER_TO_HANDLE: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
}

pub struct PerryGalleryObserverIvars {
    key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryGalleryObserver"]
    #[ivars = PerryGalleryObserverIvars]
    pub struct PerryGalleryObserver;

    impl PerryGalleryObserver {
        #[unsafe(method(boundsChanged:))]
        fn bounds_changed(&self, _notification: &AnyObject) {
            let key = self.ivars().key.get();
            let handle = OBSERVER_TO_HANDLE.with(|m| m.borrow().get(&key).copied().unwrap_or(0));
            if handle == 0 {
                return;
            }
            recompute_index(handle);
        }
    }
);

impl PerryGalleryObserver {
    fn new(key: usize) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryGalleryObserverIvars {
            key: std::cell::Cell::new(key),
        });
        unsafe { msg_send![super(this), init] }
    }
}

const PAGE_WIDTH: f64 = 320.0;
const PAGE_HEIGHT: f64 = 320.0;

/// Create an empty ImageGallery. Add images with `add_image`.
pub fn create(on_index_change: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let scroll_cls = AnyClass::get(c"NSScrollView").unwrap();
        let scroll: Retained<AnyObject> = msg_send![scroll_cls, new];
        let _: () = msg_send![&*scroll, setHasHorizontalScroller: true];
        let _: () = msg_send![&*scroll, setHasVerticalScroller: false];
        // NSScrollerStyleOverlay = 1
        let _: () = msg_send![&*scroll, setScrollerStyle: 1i64];
        let _: () = msg_send![&*scroll, setHorizontalScrollElasticity: 1i64];
        let _: () = msg_send![&*scroll, setTranslatesAutoresizingMaskIntoConstraints: false];

        // Force a default visible size so the gallery has somewhere to live.
        let h_anchor: Retained<AnyObject> = msg_send![&*scroll, heightAnchor];
        let h_constraint: Retained<AnyObject> =
            msg_send![&*h_anchor, constraintEqualToConstant: PAGE_HEIGHT];
        let _: () = msg_send![&*h_constraint, setActive: true];
        let w_anchor: Retained<AnyObject> = msg_send![&*scroll, widthAnchor];
        let w_constraint: Retained<AnyObject> =
            msg_send![&*w_anchor, constraintEqualToConstant: PAGE_WIDTH];
        let _: () = msg_send![&*w_constraint, setActive: true];

        let stack_cls = AnyClass::get(c"NSStackView").unwrap();
        let stack: Retained<AnyObject> = msg_send![stack_cls, new];
        let _: () = msg_send![&*stack, setOrientation: 0i64]; // horizontal
        let _: () = msg_send![&*stack, setSpacing: 0.0f64];
        let _: () = msg_send![&*stack, setDistribution: 1i64]; // fill equally
        let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: false];

        let _: () = msg_send![&*scroll, setDocumentView: &*stack];

        // Listen to clip-view bounds changes so we can fire onIndexChange
        // and update current_index when the user pages.
        let clip: Retained<AnyObject> = msg_send![&*scroll, contentView];
        let _: () = msg_send![&*clip, setPostsBoundsChangedNotifications: true];

        let observer = PerryGalleryObserver::new(0);
        let observer_addr = Retained::as_ptr(&observer) as usize;
        observer.ivars().key.set(observer_addr);

        let nc_cls = AnyClass::get(c"NSNotificationCenter").unwrap();
        let nc: Retained<AnyObject> = msg_send![nc_cls, defaultCenter];
        let name = NSString::from_str("NSViewBoundsDidChangeNotification");
        let sel = objc2::runtime::Sel::register(c"boundsChanged:");
        let _: () = msg_send![
            &*nc,
            addObserver: &*observer,
            selector: sel,
            name: &*name,
            object: &*clip,
        ];

        let view: Retained<NSView> = Retained::cast_unchecked(scroll);
        let handle = super::register_widget(view.clone());

        OBSERVER_TO_HANDLE.with(|m| {
            m.borrow_mut().insert(observer_addr, handle);
        });
        std::mem::forget(observer);

        GALLERIES.with(|g| {
            g.borrow_mut().insert(
                handle,
                GalleryState {
                    scroll_view: view,
                    stack,
                    image_views: Vec::new(),
                    on_index_change,
                    current_index: 0,
                },
            );
        });
        handle
    }
}

/// Add an image to the gallery. `url_ptr` may be a local path or http(s) URL;
/// `alt_ptr` is currently used as the accessibilityLabel.
pub fn add_image(handle: i64, url_ptr: *const u8, alt_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    let alt = str_from_header(alt_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let iv_cls = AnyClass::get(c"NSImageView").unwrap();
        let image_view: Retained<AnyObject> = msg_send![iv_cls, new];
        let _: () = msg_send![&*image_view, setTranslatesAutoresizingMaskIntoConstraints: false];
        // NSImageScaleProportionallyUpOrDown = 0
        let _: () = msg_send![&*image_view, setImageScaling: 0i64];

        if !alt.is_empty() {
            let ns_alt = NSString::from_str(alt);
            let _: () = msg_send![&*image_view, setAccessibilityLabel: &*ns_alt];
        }

        // Local file path → load synchronously. Remote URL → background
        // dispatch (we leave dispatching to NSURLSession to keep this
        // synchronous-on-the-main-thread loader simple).
        if !url.is_empty() {
            if url.starts_with("http://") || url.starts_with("https://") {
                load_remote(url, image_view.clone());
            } else {
                let img_cls = AnyClass::get(c"NSImage").unwrap();
                let ns_path = NSString::from_str(url);
                let image: *mut AnyObject = msg_send![img_cls, alloc];
                let image: *mut AnyObject = msg_send![image, initWithContentsOfFile: &*ns_path];
                if !image.is_null() {
                    let _: () = msg_send![&*image_view, setImage: image];
                }
            }
        }

        // Each page is exactly PAGE_WIDTH wide so paging snaps cleanly.
        let w_anchor: Retained<AnyObject> = msg_send![&*image_view, widthAnchor];
        let constraint: Retained<AnyObject> =
            msg_send![&*w_anchor, constraintEqualToConstant: PAGE_WIDTH];
        let _: () = msg_send![&*constraint, setActive: true];

        GALLERIES.with(|g| {
            if let Some(state) = g.borrow_mut().get_mut(&handle) {
                let _: () = msg_send![&*state.stack, addArrangedSubview: &*image_view];
                state.image_views.push(image_view);
            }
        });
    }
}

/// Programmatically jump to a given image index. Animated.
pub fn set_index(handle: i64, index: i64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    GALLERIES.with(|g| {
        let mut galleries = g.borrow_mut();
        let Some(state) = galleries.get_mut(&handle) else {
            return;
        };
        if (index as usize) >= state.image_views.len() {
            return;
        }
        state.current_index = index;
        unsafe {
            let scroll = &state.scroll_view;
            let clip: Retained<AnyObject> = msg_send![&**scroll, contentView];
            let target_x = index as f64 * PAGE_WIDTH;

            // animator() form so the jump is animated.
            let animator: Retained<AnyObject> = msg_send![&*clip, animator];
            let point = CGPoint::new(target_x, 0.0);
            let _: () = msg_send![&*animator, setBoundsOrigin: point];
            let _: () = msg_send![&**scroll, reflectScrolledClipView: &*clip];
        }
    });
}

fn recompute_index(handle: i64) {
    let (closure, new_index) = GALLERIES.with(|g| {
        let mut galleries = g.borrow_mut();
        let Some(state) = galleries.get_mut(&handle) else {
            return (0.0, -1i64);
        };
        unsafe {
            let scroll = &state.scroll_view;
            let clip: Retained<AnyObject> = msg_send![&**scroll, contentView];
            let bounds: CGRect = msg_send![&*clip, bounds];
            let center_x = bounds.origin.x + bounds.size.width / 2.0;
            let new_index = (center_x / PAGE_WIDTH).floor() as i64;
            let new_index = new_index
                .max(0)
                .min((state.image_views.len() as i64).saturating_sub(1));
            if new_index != state.current_index {
                state.current_index = new_index;
                (state.on_index_change, new_index)
            } else {
                (0.0, -1)
            }
        }
    });
    if new_index >= 0 && closure != 0.0 {
        unsafe {
            let closure_ptr = js_nanbox_get_pointer(closure) as *const u8;
            js_closure_call1(closure_ptr, new_index as f64);
        }
    }
}

/// Load a remote image on a background thread and apply it on the main
/// thread when the bytes arrive. We use `+[NSData dataWithContentsOfURL:]`
/// (synchronous, but on a background dispatch queue) — sufficient for
/// the gallery use case and avoids the block-bridging complexity of
/// NSURLSession's completion-handler form.
fn load_remote(url: &str, image_view: Retained<AnyObject>) {
    extern "C" {
        fn dispatch_get_global_queue(identifier: i64, flags: u64) -> *const std::ffi::c_void;
        static _dispatch_main_q: std::ffi::c_void;
        fn dispatch_async_f(
            queue: *const std::ffi::c_void,
            context: *mut std::ffi::c_void,
            work: unsafe extern "C" fn(*mut std::ffi::c_void),
        );
    }

    let url_string = url.to_string();
    let leaked_view = Box::into_raw(Box::new(image_view));

    struct WorkPkg {
        url: String,
        view_box: *mut Retained<AnyObject>,
    }

    let pkg = Box::into_raw(Box::new(WorkPkg {
        url: url_string,
        view_box: leaked_view,
    }));

    unsafe extern "C" fn worker(ctx: *mut std::ffi::c_void) {
        let _ = std::panic::catch_unwind(|| {
            let pkg = Box::from_raw(ctx as *mut WorkPkg);
            unsafe {
                let url_cls = AnyClass::get(c"NSURL").unwrap();
                let ns_url_str = NSString::from_str(&pkg.url);
                let nsurl: *mut AnyObject = msg_send![url_cls, URLWithString: &*ns_url_str];
                if nsurl.is_null() {
                    let _ = Box::from_raw(pkg.view_box);
                    return;
                }
                let data_cls = AnyClass::get(c"NSData").unwrap();
                let data: *mut AnyObject = msg_send![data_cls, dataWithContentsOfURL: nsurl];
                if data.is_null() {
                    let _ = Box::from_raw(pkg.view_box);
                    return;
                }
                // Hop back to main thread to install the image.
                struct ApplyPkg {
                    data: *mut AnyObject,
                    view_box: *mut Retained<AnyObject>,
                }
                // Retain the data so it survives until the main-thread block runs.
                let _: *mut AnyObject = msg_send![data, retain];
                let apply = Box::into_raw(Box::new(ApplyPkg {
                    data,
                    view_box: pkg.view_box,
                }));

                unsafe extern "C" fn apply_main(ctx: *mut std::ffi::c_void) {
                    let _ = std::panic::catch_unwind(|| {
                        let p = Box::from_raw(ctx as *mut ApplyPkg);
                        let view = *Box::from_raw(p.view_box);
                        unsafe {
                            let img_cls = AnyClass::get(c"NSImage").unwrap();
                            let image: *mut AnyObject = msg_send![img_cls, alloc];
                            let image: *mut AnyObject = msg_send![image, initWithData: p.data];
                            if !image.is_null() {
                                let _: () = msg_send![&*view, setImage: image];
                            }
                            let _: () = msg_send![p.data, release];
                        }
                    });
                }
                dispatch_async_f(
                    &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                    apply as *mut std::ffi::c_void,
                    apply_main,
                );
            }
        });
    }

    unsafe {
        let q = dispatch_get_global_queue(0, 0);
        dispatch_async_f(q, pkg as *mut std::ffi::c_void, worker);
    }
}

#[allow(dead_code)]
fn _touch(state: &GalleryState) -> *const AnyObject {
    Retained::as_ptr(&state.scroll_view) as *const AnyObject
}
