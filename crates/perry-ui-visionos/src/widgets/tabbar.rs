use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_ui_kit::UIView;
use objc2_foundation::{NSObject, NSString, MainThreadMarker};
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

/// Pick an SF Symbol name based on the tab label.
fn icon_for_label(label: &str) -> &'static str {
    let lower = label.to_ascii_lowercase();
    if lower.contains("dashboard") || lower.contains("home") || lower.contains("performance") {
        "chart.bar.fill"
    } else if lower.contains("profile") || lower.contains("account") {
        "person.fill"
    } else if lower.contains("setting") {
        "gearshape.fill"
    } else if lower.contains("search") {
        "magnifyingglass"
    } else if lower.contains("site") {
        "globe"
    } else if lower.contains("chart") || lower.contains("stat") || lower.contains("analytic") {
        "chart.bar.fill"
    } else {
        "circle.fill"
    }
}

// ── State ─────────────────────────────────────────────────────

struct TabBarState {
    items: Vec<*mut AnyObject>,    // UITabBarItem raw pointers (kept alive by UITabBar)
    on_change: f64,
}

thread_local! {
    static TABBAR_STATE: RefCell<HashMap<i64, TabBarState>> = RefCell::new(HashMap::new());
    /// Maps delegate address → tabbar handle
    static DELEGATE_MAP: RefCell<HashMap<usize, i64>> = RefCell::new(HashMap::new());
}

// ── UITabBarDelegate ─────────────────────────────────────────

pub struct PerryTabBarDelegateIvars {
    key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTabBarDelegate"]
    #[ivars = PerryTabBarDelegateIvars]
    pub struct PerryTabBarDelegate;

    impl PerryTabBarDelegate {
        #[unsafe(method(tabBar:didSelectItem:))]
        fn tab_bar_did_select_item(&self, _tab_bar: &AnyObject, item: &AnyObject) {
            let tag: i64 = unsafe { msg_send![item, tag] };
            let key = self.ivars().key.get();

            let tabbar_handle = DELEGATE_MAP.with(|m| {
                m.borrow().get(&key).copied().unwrap_or(0)
            });

            if tabbar_handle == 0 { return; }

            let on_change = TABBAR_STATE.with(|s| {
                s.borrow().get(&tabbar_handle).map(|st| st.on_change).unwrap_or(0.0)
            });

            if on_change != 0.0 {
                let packed = Box::new((on_change, tag));
                unsafe {
                    dispatch_async_f(
                        &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                        Box::into_raw(packed) as *mut std::ffi::c_void,
                        tab_callback_trampoline,
                    );
                }
            }
        }
    }
);

impl PerryTabBarDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTabBarDelegateIvars {
            key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

unsafe extern "C" fn tab_callback_trampoline(context: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let packed = Box::from_raw(context as *mut (f64, i64));
        let (closure_f64, tab_index) = *packed;
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        js_closure_call1(closure_ptr as *const u8, tab_index as f64);
    });
}

// ── Public API ────────────────────────────────────────────────

/// Create a native UITabBar.
pub fn create(on_change: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        // Create UITabBar
        let tabbar_cls = objc2::runtime::AnyClass::get(c"UITabBar").unwrap();
        let tabbar: *mut AnyObject = msg_send![tabbar_cls, alloc];
        let tabbar: *mut AnyObject = msg_send![tabbar, init];

        // Google Blue (#4285f4) tint
        let tint: Retained<AnyObject> = msg_send![
            objc2::runtime::AnyClass::get(c"UIColor").unwrap(),
            colorWithRed: 0.263f64,
            green: 0.522f64,
            blue: 0.957f64,
            alpha: 1.0f64
        ];
        let _: () = msg_send![tabbar, setTintColor: &*tint];

        // Create and set delegate
        let delegate = PerryTabBarDelegate::new();
        let delegate_addr = Retained::as_ptr(&delegate) as usize;
        delegate.ivars().key.set(delegate_addr);
        let _: () = msg_send![tabbar, setDelegate: &*delegate];
        std::mem::forget(delegate); // keep alive

        let view: Retained<UIView> = Retained::retain(tabbar as *mut UIView).unwrap();
        let handle = super::register_widget(view);

        DELEGATE_MAP.with(|m| {
            m.borrow_mut().insert(delegate_addr, handle);
        });

        TABBAR_STATE.with(|s| {
            s.borrow_mut().insert(handle, TabBarState {
                items: Vec::new(),
                on_change,
            });
        });

        handle
    }
}

/// Add a tab item with a title and auto-detected SF Symbol icon.
pub fn add_tab(tabbar_handle: i64, label_ptr: *const u8) {
    let label = str_from_header(label_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    let tab_index = TABBAR_STATE.with(|s| {
        s.borrow().get(&tabbar_handle).map(|st| st.items.len() as i64).unwrap_or(0)
    });

    unsafe {
        // Create SF Symbol image
        let icon_name = icon_for_label(label);
        let ns_icon = NSString::from_str(icon_name);
        let image: *mut AnyObject = msg_send![
            objc2::runtime::AnyClass::get(c"UIImage").unwrap(),
            systemImageNamed: &*ns_icon
        ];

        // Create UITabBarItem
        let ns_title = NSString::from_str(label);
        let item_cls = objc2::runtime::AnyClass::get(c"UITabBarItem").unwrap();
        let item: *mut AnyObject = msg_send![item_cls, alloc];
        let item: *mut AnyObject = msg_send![
            item,
            initWithTitle: &*ns_title,
            image: image,
            tag: tab_index
        ];

        // Store item and update UITabBar
        TABBAR_STATE.with(|s| {
            if let Some(state) = s.borrow_mut().get_mut(&tabbar_handle) {
                state.items.push(item);

                // Build NSArray of all items
                if let Some(bar_view) = super::get_widget(tabbar_handle) {
                    let arr_cls = objc2::runtime::AnyClass::get(c"NSMutableArray").unwrap();
                    let arr: *mut AnyObject = msg_send![arr_cls, arrayWithCapacity: state.items.len()];
                    for &itm in &state.items {
                        let _: () = msg_send![arr, addObject: itm];
                    }
                    let _: () = msg_send![&*bar_view, setItems: arr, animated: false];
                }
            }
        });
    }
}

/// Set the selected tab by index.
pub fn set_selected(tabbar_handle: i64, index: i64) {
    TABBAR_STATE.with(|s| {
        if let Some(state) = s.borrow().get(&tabbar_handle) {
            if let Some(&item) = state.items.get(index as usize) {
                if let Some(bar_view) = super::get_widget(tabbar_handle) {
                    unsafe {
                        let _: () = msg_send![&*bar_view, setSelectedItem: item];
                    }
                }
            }
        }
    });
}
