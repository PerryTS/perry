//! macOS Command Palette widget (issue #477, v1).
//!
//! Borderless `NSPanel` with an `NSSearchField` on top and an
//! `NSTableView` below showing commands whose labels match the search
//! query (case-insensitive substring). Arrow keys / Enter / Esc are
//! handled by the standard NSTableView focus chain. Selecting a row
//! invokes the command's `on_run` closure and closes the palette.
//!
//! Out of scope v1 (per #477 scope notes): fuzzy ranking, recent /
//! frequently-used boost, async command sources, command groups /
//! section headers, OS-native menu-bar integration, default global
//! hotkey wiring — user code binds `commandPaletteShow()` to ⌘K
//! themselves via `addKeyboardShortcut`.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread};
use objc2_app_kit::NSView;
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSObject, NSString};
use std::cell::{Cell, RefCell};

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

struct Command {
    id: String,
    label: String,
    subtitle: String,
    on_run: f64,
}

thread_local! {
    static COMMANDS: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
    static FILTERED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static QUERY: RefCell<String> = const { RefCell::new(String::new()) };
    static PANEL: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static TABLE_VIEW: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

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

fn refresh_filter() {
    let q = QUERY.with(|s| s.borrow().to_lowercase());
    let filtered: Vec<usize> = COMMANDS.with(|c| {
        c.borrow()
            .iter()
            .enumerate()
            .filter_map(|(i, cmd)| {
                if q.is_empty() {
                    Some(i)
                } else {
                    let label = cmd.label.to_lowercase();
                    let subtitle = cmd.subtitle.to_lowercase();
                    if label.contains(&q) || subtitle.contains(&q) {
                        Some(i)
                    } else {
                        None
                    }
                }
            })
            .collect()
    });
    FILTERED.with(|f| *f.borrow_mut() = filtered);
    if let Some(tv) = TABLE_VIEW.with(|t| t.borrow().clone()) {
        unsafe {
            let _: () = msg_send![&*tv, reloadData];
        }
    }
}

// ===========================================================================
// Delegate / data source — single class, identical pattern to
// `widgets::table::PerryTableDelegate`.
// ===========================================================================

pub struct PerryCmdPaletteDelegateIvars {
    _marker: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryCmdPaletteDelegate"]
    #[ivars = PerryCmdPaletteDelegateIvars]
    pub struct PerryCmdPaletteDelegate;

    impl PerryCmdPaletteDelegate {
        // NSTableViewDataSource
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &AnyObject) -> i64 {
            FILTERED.with(|f| f.borrow().len() as i64)
        }

        // NSTableViewDelegate — single-column cell view.
        #[unsafe(method(tableView:viewForTableColumn:row:))]
        fn view_for_column(
            &self,
            _table: &AnyObject,
            _column: &AnyObject,
            row: i64,
        ) -> *mut NSView {
            let cmd_idx = FILTERED.with(|f| f.borrow().get(row as usize).copied());
            let Some(cmd_idx) = cmd_idx else { return std::ptr::null_mut() };
            let (label, subtitle) = COMMANDS.with(|c| {
                let cmds = c.borrow();
                cmds.get(cmd_idx)
                    .map(|cmd| (cmd.label.clone(), cmd.subtitle.clone()))
                    .unwrap_or_default()
            });
            unsafe {
                let cell_cls = AnyClass::get(c"NSTextField").unwrap();
                let alloc: *mut AnyObject = msg_send![cell_cls, alloc];
                let frame = objc2_core_foundation::CGRect::new(
                    objc2_core_foundation::CGPoint::new(0.0, 0.0),
                    objc2_core_foundation::CGSize::new(360.0, 28.0),
                );
                let raw: *mut AnyObject = msg_send![alloc, initWithFrame: frame];
                let _: () = msg_send![raw, setBordered: false];
                let _: () = msg_send![raw, setEditable: false];
                let _: () = msg_send![raw, setSelectable: false];
                let _: () = msg_send![raw, setDrawsBackground: false];
                let display = if subtitle.is_empty() {
                    label.clone()
                } else {
                    format!("{}    {}", label, subtitle)
                };
                let ns = NSString::from_str(&display);
                let _: () = msg_send![raw, setStringValue: &*ns];
                raw as *mut NSView
            }
        }

        // NSSearchField target-action — text changed.
        #[unsafe(method(searchTextChanged:))]
        fn search_text_changed(&self, sender: &AnyObject) {
            unsafe {
                let ns: Retained<NSString> = msg_send![sender, stringValue];
                QUERY.with(|s| *s.borrow_mut() = ns.to_string());
            }
            refresh_filter();
        }

        // NSTableView double-click — invoke selected command and dismiss.
        #[unsafe(method(rowDoubleClicked:))]
        fn row_double_clicked(&self, sender: &AnyObject) {
            crate::catch_callback_panic(
                "command-palette double-click",
                std::panic::AssertUnwindSafe(|| {
                    unsafe {
                        let row: i64 = msg_send![sender, clickedRow];
                        invoke_row(row);
                    }
                }),
            );
        }
    }
);

impl PerryCmdPaletteDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryCmdPaletteDelegateIvars {
            _marker: Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

unsafe fn invoke_row(row: i64) {
    if row < 0 {
        return;
    }
    let cmd_idx = FILTERED.with(|f| f.borrow().get(row as usize).copied());
    let Some(cmd_idx) = cmd_idx else { return };
    let on_run = COMMANDS.with(|c| c.borrow().get(cmd_idx).map(|cmd| cmd.on_run).unwrap_or(0.0));
    if on_run != 0.0 {
        let closure_ptr = js_nanbox_get_pointer(on_run) as *const u8;
        js_closure_call0(closure_ptr);
    }
    hide();
}

// ===========================================================================
// Public API.
// ===========================================================================

pub fn register(id_ptr: *const u8, label_ptr: *const u8, subtitle_ptr: *const u8, on_run: f64) {
    let id = str_from_header(id_ptr);
    let label = str_from_header(label_ptr);
    let subtitle = str_from_header(subtitle_ptr);
    COMMANDS.with(|c| {
        let mut cmds = c.borrow_mut();
        if let Some(existing) = cmds.iter_mut().find(|x| x.id == id) {
            existing.label = label;
            existing.subtitle = subtitle;
            existing.on_run = on_run;
        } else {
            cmds.push(Command {
                id,
                label,
                subtitle,
                on_run,
            });
        }
    });
    refresh_filter();
}

pub fn unregister(id_ptr: *const u8) {
    let id = str_from_header(id_ptr);
    COMMANDS.with(|c| c.borrow_mut().retain(|cmd| cmd.id != id));
    refresh_filter();
}

pub fn clear() {
    COMMANDS.with(|c| c.borrow_mut().clear());
    refresh_filter();
}

pub fn show() {
    if PANEL.with(|p| p.borrow().is_some()) {
        return;
    }
    QUERY.with(|s| s.borrow_mut().clear());
    refresh_filter();

    unsafe {
        let panel_cls = AnyClass::get(c"NSPanel").unwrap();
        let alloc: *mut AnyObject = msg_send![panel_cls, alloc];

        let panel_w: CGFloat = 480.0;
        let panel_h: CGFloat = 380.0;

        // Anchor at the screen's visible-frame center.
        let screen_cls = AnyClass::get(c"NSScreen").unwrap();
        let screen: *mut AnyObject = msg_send![screen_cls, mainScreen];
        let mut x: CGFloat = 200.0;
        let mut y: CGFloat = 200.0;
        if !screen.is_null() {
            let frame: objc2_core_foundation::CGRect = msg_send![screen, visibleFrame];
            x = frame.origin.x + (frame.size.width - panel_w) / 2.0;
            y = frame.origin.y + (frame.size.height - panel_h) * 0.66;
        }
        let panel_frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(x, y),
            objc2_core_foundation::CGSize::new(panel_w, panel_h),
        );

        let raw: *mut AnyObject = msg_send![
            alloc,
            initWithContentRect: panel_frame,
            // NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
            // = 1 | 2 — gives a minimal title bar but no window controls
            // distractions; we'll hide the title text below.
            styleMask: 0u64, // borderless
            backing: 2u64,
            defer: false
        ];
        let panel: Retained<AnyObject> = Retained::from_raw(raw).unwrap();

        let _: () = msg_send![&*panel, setLevel: 3i64]; // floating
        let _: () = msg_send![&*panel, setOpaque: false];
        let _: () = msg_send![&*panel, setHasShadow: true];

        let content: *mut AnyObject = msg_send![&*panel, contentView];
        let _: () = msg_send![content, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![content, layer];
        let _: () = msg_send![layer, setCornerRadius: 12.0_f64 as CGFloat];
        let _: () = msg_send![layer, setMasksToBounds: true];
        let bg_color: *mut AnyObject = msg_send![
            AnyClass::get(c"NSColor").unwrap(),
            colorWithCalibratedRed: 0.96 as CGFloat,
            green: 0.96 as CGFloat,
            blue: 0.96 as CGFloat,
            alpha: 0.98 as CGFloat
        ];
        let cg: *mut AnyObject = msg_send![bg_color, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg];

        // Search field at the top.
        let sf_cls = AnyClass::get(c"NSSearchField").unwrap();
        let sf_alloc: *mut AnyObject = msg_send![sf_cls, alloc];
        let sf_frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(12.0, panel_h - 44.0),
            objc2_core_foundation::CGSize::new(panel_w - 24.0, 32.0),
        );
        let search: *mut AnyObject = msg_send![sf_alloc, initWithFrame: sf_frame];
        let placeholder = NSString::from_str("Type a command…");
        let _: () = msg_send![search, setPlaceholderString: &*placeholder];
        let _: () = msg_send![content, addSubview: search];

        // Table view in scroll view, beneath the search field.
        let scroll_cls = AnyClass::get(c"NSScrollView").unwrap();
        let scroll: Retained<AnyObject> = msg_send![scroll_cls, new];
        let scroll_frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(12.0, 12.0),
            objc2_core_foundation::CGSize::new(panel_w - 24.0, panel_h - 64.0),
        );
        let _: () = msg_send![&*scroll, setFrame: scroll_frame];
        let _: () = msg_send![&*scroll, setHasVerticalScroller: true];
        let _: () = msg_send![&*scroll, setHasHorizontalScroller: false];

        let tv_cls = AnyClass::get(c"NSTableView").unwrap();
        let tv: Retained<AnyObject> = msg_send![tv_cls, new];
        let tc_cls = AnyClass::get(c"NSTableColumn").unwrap();
        let col: Retained<AnyObject> = msg_send![tc_cls, new];
        let key = NSString::from_str("cmd");
        let _: () = msg_send![&*col, setIdentifier: &*key];
        let _: () = msg_send![&*col, setWidth: (panel_w - 50.0) as f64];
        let _: () = msg_send![&*tv, addTableColumn: &*col];
        let _: () = msg_send![&*tv, setHeaderView: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![&*scroll, setDocumentView: &*tv];
        let _: () = msg_send![content, addSubview: &*scroll];

        let delegate = PerryCmdPaletteDelegate::new();
        let _: () = msg_send![&*tv, setDataSource: &*delegate];
        let _: () = msg_send![&*tv, setDelegate: &*delegate];
        let _: () = msg_send![&*tv, setTarget: &*delegate];
        let _: () = msg_send![&*tv, setDoubleAction: Sel::register(c"rowDoubleClicked:")];

        // Wire search field target-action.
        let _: () = msg_send![search, setTarget: &*delegate];
        let _: () = msg_send![search, setAction: Sel::register(c"searchTextChanged:")];

        std::mem::forget(delegate);

        TABLE_VIEW.with(|t| *t.borrow_mut() = Some(tv));
        let _: () = msg_send![&*panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![&*panel, makeFirstResponder: search];
        PANEL.with(|p| *p.borrow_mut() = Some(panel));
    }
}

pub fn hide() {
    let panel = PANEL.with(|p| p.borrow_mut().take());
    if let Some(p) = panel {
        unsafe {
            let _: () = msg_send![&*p, orderOut: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*p, close];
        }
    }
    TABLE_VIEW.with(|t| *t.borrow_mut() = None);
}
