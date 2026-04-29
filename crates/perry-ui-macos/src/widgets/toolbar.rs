use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSApplication;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
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

struct ToolbarEntry {
    toolbar: Retained<AnyObject>,
    items: Vec<ToolbarItem>,
}

struct ToolbarItem {
    identifier: String,
    label: String,
    icon: String,
    callback: f64,
}

thread_local! {
    static TOOLBARS: RefCell<Vec<ToolbarEntry>> = RefCell::new(Vec::new());
    static TOOLBAR_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

pub struct PerryToolbarTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryToolbarTarget"]
    #[ivars = PerryToolbarTargetIvars]
    pub struct PerryToolbarTarget;

    impl PerryToolbarTarget {
        #[unsafe(method(toolbarItemClicked:))]
        fn toolbar_item_clicked(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("toolbar callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                let closure_f64 = TOOLBAR_CALLBACKS.with(|cbs| {
                    cbs.borrow().get(&key).copied()
                });
                if let Some(closure_f64) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            }));
        }
    }
);

impl PerryToolbarTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryToolbarTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Create a toolbar. Returns 1-based handle.
pub fn create() -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let toolbar_cls = AnyClass::get(c"NSToolbar").unwrap();
        let ident = NSString::from_str("PerryToolbar");
        let toolbar_ptr: *mut AnyObject = msg_send![toolbar_cls, alloc];
        let toolbar_ptr: *mut AnyObject = msg_send![toolbar_ptr, initWithIdentifier: &*ident];
        let toolbar = Retained::retain(toolbar_ptr).unwrap();
        let _: () = msg_send![&*toolbar, setDisplayMode: 1i64]; // NSToolbarDisplayModeIconAndLabel

        TOOLBARS.with(|t| {
            let mut toolbars = t.borrow_mut();
            toolbars.push(ToolbarEntry {
                toolbar,
                items: Vec::new(),
            });
            toolbars.len() as i64
        })
    }
}

/// Add an item to a toolbar.
pub fn add_item(toolbar_handle: i64, label_ptr: *const u8, icon_ptr: *const u8, callback: f64) {
    let label = str_from_header(label_ptr).to_string();
    let icon = str_from_header(icon_ptr).to_string();
    let identifier = format!("perry_toolbar_item_{}", label.replace(' ', "_"));

    TOOLBARS.with(|t| {
        let mut toolbars = t.borrow_mut();
        let idx = (toolbar_handle - 1) as usize;
        if idx < toolbars.len() {
            toolbars[idx].items.push(ToolbarItem {
                identifier,
                label,
                icon,
                callback,
            });
        }
    });
}

/// Attach a toolbar to the key window.
pub fn attach(toolbar_handle: i64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);

    TOOLBARS.with(|t| {
        let toolbars = t.borrow();
        let idx = (toolbar_handle - 1) as usize;
        if idx < toolbars.len() {
            unsafe {
                let toolbar = &toolbars[idx].toolbar;

                // Create toolbar items and add them
                for item_info in &toolbars[idx].items {
                    let item_id = NSString::from_str(&item_info.identifier);
                    let item_cls = AnyClass::get(c"NSToolbarItem").unwrap();
                    let item_ptr: *mut AnyObject = msg_send![item_cls, alloc];
                    let item_ptr: *mut AnyObject = msg_send![item_ptr, initWithItemIdentifier: &*item_id];
                    let item = Retained::retain(item_ptr).unwrap();

                    let ns_label = NSString::from_str(&item_info.label);
                    let _: () = msg_send![&*item, setLabel: &*ns_label];

                    // Set SF Symbol image if available
                    if !item_info.icon.is_empty() {
                        let ns_icon = NSString::from_str(&item_info.icon);
                        let image_cls = AnyClass::get(c"NSImage").unwrap();
                        let image: *mut AnyObject = msg_send![image_cls, imageWithSystemSymbolName: &*ns_icon, accessibilityDescription: std::ptr::null::<AnyObject>()];
                        if !image.is_null() {
                            let _: () = msg_send![&*item, setImage: image];
                        }
                    }

                    // Set action target
                    let target = PerryToolbarTarget::new();
                    let target_addr = Retained::as_ptr(&target) as usize;
                    target.ivars().callback_key.set(target_addr);
                    TOOLBAR_CALLBACKS.with(|cbs| {
                        cbs.borrow_mut().insert(target_addr, item_info.callback);
                    });
                    let _: () = msg_send![&*item, setTarget: &*target];
                    let _: () = msg_send![&*item, setAction: Sel::register(c"toolbarItemClicked:")];
                    std::mem::forget(target);

                    // Insert item into toolbar
                    let _: () = msg_send![&**toolbar, insertItemWithItemIdentifier: &*item_id, atIndex: 0i64];
                }

                if let Some(key_window) = app.keyWindow() {
                    let _: () = msg_send![&*key_window, setToolbar: &**toolbar];
                }
            }
        }
    });
}
