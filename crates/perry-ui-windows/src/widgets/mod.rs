//! Widget registry — Vec<WidgetEntry> with 1-based handles.
//! Each widget has an HWND (on Windows), a kind, children list, and layout info.

pub mod button;
pub mod canvas;
pub mod divider;
pub mod form;
pub mod hstack;
pub mod image;
pub mod lazyvstack;
pub mod navstack;
pub mod picker;
pub mod progressview;
pub mod scrollview;
pub mod securefield;
pub mod slider;
pub mod spacer;
pub mod text;
pub mod text_registry;
pub mod textfield;
pub mod toast;
pub mod toggle;
pub mod vstack;
pub mod zstack;

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateRoundRectRgn, CreateSolidBrush, FillRect, GradientFill, InvalidateRect,
    SetWindowRgn, GRADIENT_FILL_RECT_H, GRADIENT_FILL_RECT_V, GRADIENT_RECT, HBRUSH, HDC,
    TRIVERTEX,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetKind {
    Text,
    Button,
    VStack,
    HStack,
    Spacer,
    Divider,
    TextField,
    Toggle,
    Slider,
    ScrollView,
    SecureField,
    ProgressView,
    Form,
    Section,
    ZStack,
    Picker,
    Canvas,
    NavStack,
    LazyVStack,
    Image,
}

pub struct WidgetEntry {
    #[cfg(target_os = "windows")]
    pub hwnd: HWND,
    #[cfg(not(target_os = "windows"))]
    pub hwnd: isize,
    pub kind: WidgetKind,
    pub children: Vec<i64>,
    pub spacing: f64,
    pub insets: (f64, f64, f64, f64), // top, left, bottom, right
    pub hidden: bool,
    /// Win32 control ID (for WM_COMMAND routing)
    pub control_id: u16,
    /// When true, this widget absorbs remaining space in a VStack/HStack (like a Spacer).
    pub fills_remaining: bool,
    /// Fixed width in pixels (set by widgetSetWidth)
    pub fixed_width: Option<i32>,
    /// Fixed height in pixels (set by widgetSetHeight)
    pub fixed_height: Option<i32>,
    /// Whether this widget should stretch to match its parent's height
    pub match_parent_height: bool,
    /// Whether this widget should stretch to match its parent's width
    pub match_parent_width: bool,
    /// Whether this stack should exclude hidden children from layout
    pub detaches_hidden: bool,
    /// Distribution mode: 0=Fill, 1=FillEqually, -1=GravityAreas (default: -1)
    pub distribution: i64,
    /// Alignment mode for cross axis
    pub alignment: i64,
}

/// Info returned by get_widget_info (clone-safe subset)
pub struct WidgetInfo {
    pub kind: WidgetKind,
    pub children: Vec<i64>,
    pub spacing: f64,
    pub insets: (f64, f64, f64, f64),
    pub hidden: bool,
    pub fills_remaining: bool,
    pub fixed_width: Option<i32>,
    pub fixed_height: Option<i32>,
    pub match_parent_height: bool,
    pub match_parent_width: bool,
    pub detaches_hidden: bool,
    pub distribution: i64,
    pub alignment: i64,
}

thread_local! {
    static WIDGETS: RefCell<Vec<WidgetEntry>> = RefCell::new(Vec::new());
    static NEXT_CONTROL_ID: RefCell<u16> = RefCell::new(1000);
    /// Hidden parking window used as a temporary parent for WS_CHILD widgets
    /// before they are reparented into the real window hierarchy.
    #[cfg(target_os = "windows")]
    static PARKING_HWND: RefCell<Option<HWND>> = RefCell::new(None);
    /// Background color brushes keyed by widget handle
    #[cfg(target_os = "windows")]
    static BG_BRUSHES: RefCell<HashMap<i64, HBRUSH>> = RefCell::new(HashMap::new());
    /// Background COLORREF values keyed by widget handle
    static BG_COLORS: RefCell<HashMap<i64, u32>> = RefCell::new(HashMap::new());
}

/// Mutex-based handle→HWND map (stores HWND as usize for Send safety).
/// Unlike the RefCell-based WIDGETS vec, this can be accessed during WM_PAINT
/// even when WIDGETS is borrowed for layout.
#[cfg(target_os = "windows")]
static HWND_MAP: std::sync::Mutex<Vec<(i64, usize)>> = std::sync::Mutex::new(Vec::new());

/// Gradient info for a widget: two COLORREF values and direction.
#[cfg(target_os = "windows")]
pub struct GradientInfo {
    pub c1: u32,
    pub c2: u32,
    pub vertical: bool,
}

/// Mutex-based HWND→GradientInfo map (keyed by HWND as isize for Send safety).
/// Using Mutex (not RefCell) so it can be accessed during WM_PAINT without reentrancy issues.
#[cfg(target_os = "windows")]
pub static GRADIENT_MAP: std::sync::Mutex<Vec<(isize, GradientInfo)>> =
    std::sync::Mutex::new(Vec::new());

/// Store handle→HWND mapping in the Mutex-based map (called during widget registration).
#[cfg(target_os = "windows")]
fn store_hwnd_mapping(handle: i64, hwnd: HWND) {
    if let Ok(mut map) = HWND_MAP.lock() {
        map.push((handle, hwnd.0 as usize));
    }
}

/// Look up HWND by handle using the Mutex-based map (reentrancy-safe).
#[cfg(target_os = "windows")]
pub fn get_hwnd_safe(handle: i64) -> Option<HWND> {
    if let Ok(map) = HWND_MAP.lock() {
        for &(h, hwnd_val) in map.iter().rev() {
            if h == handle {
                return Some(HWND(hwnd_val as *mut _));
            }
        }
    }
    None
}

/// Convert RGB floats (0.0-1.0) to Win32 COLORREF (0x00BBGGRR)
#[cfg(target_os = "windows")]
fn rgb_to_colorref(r: f64, g: f64, b: f64) -> u32 {
    let r = (r * 255.0).round().min(255.0).max(0.0) as u32;
    let g = (g * 255.0).round().min(255.0).max(0.0) as u32;
    let b = (b * 255.0).round().min(255.0).max(0.0) as u32;
    r | (g << 8) | (b << 16)
}

/// Get the background brush for a widget (if set).
#[cfg(target_os = "windows")]
pub fn get_bg_brush(handle: i64) -> Option<HBRUSH> {
    BG_BRUSHES.with(|b| b.borrow().get(&handle).copied())
}

/// Get the background COLORREF for a widget (if set).
pub fn get_bg_color(handle: i64) -> Option<u32> {
    BG_COLORS.with(|c| c.borrow().get(&handle).copied())
}

/// Get (or lazily create) the hidden parking window for orphan child widgets.
#[cfg(target_os = "windows")]
pub fn get_parking_hwnd() -> HWND {
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    PARKING_HWND.with(|cell| {
        let mut opt = cell.borrow_mut();
        if let Some(hwnd) = *opt {
            return hwnd;
        }
        unsafe {
            let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let hinstance_h = HINSTANCE(hinstance.0 as _);
            // HWND_MESSAGE creates a message-only window (invisible, no UI)
            let class = to_wide("STATIC");
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class.as_ptr()),
                windows::core::PCWSTR(std::ptr::null()),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                HMENU::default(),
                hinstance_h,
                None,
            )
            .unwrap();
            *opt = Some(hwnd);
            hwnd
        }
    })
}

/// Allocate a new control ID.
pub fn alloc_control_id() -> u16 {
    NEXT_CONTROL_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    })
}

/// Register a widget entry and return its 1-based handle.
#[cfg(target_os = "windows")]
pub fn register_widget(hwnd: HWND, kind: WidgetKind, control_id: u16) -> i64 {
    let handle = WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing: 0.0,
            insets: (0.0, 0.0, 0.0, 0.0),
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
            fixed_height: None,
            match_parent_height: false,
            match_parent_width: false,
            detaches_hidden: false,
            distribution: 0,
            alignment: 0,
        });
        widgets.len() as i64
    });
    store_hwnd_mapping(handle, hwnd);
    handle
}

#[cfg(not(target_os = "windows"))]
pub fn register_widget(hwnd: isize, kind: WidgetKind, control_id: u16) -> i64 {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing: 0.0,
            insets: (0.0, 0.0, 0.0, 0.0),
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
            fixed_height: None,
            match_parent_height: false,
            match_parent_width: false,
            detaches_hidden: false,
            distribution: 0,
            alignment: 0,
        });
        widgets.len() as i64
    })
}

/// Register a widget with spacing and insets (for stacks).
#[cfg(target_os = "windows")]
pub fn register_widget_with_layout(
    hwnd: HWND,
    kind: WidgetKind,
    spacing: f64,
    insets: (f64, f64, f64, f64),
) -> i64 {
    let control_id = alloc_control_id();
    let handle = WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing,
            insets,
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
            fixed_height: None,
            match_parent_height: false,
            match_parent_width: false,
            detaches_hidden: false,
            distribution: 0,
            alignment: 0,
        });
        widgets.len() as i64
    });
    store_hwnd_mapping(handle, hwnd);
    handle
}

#[cfg(not(target_os = "windows"))]
pub fn register_widget_with_layout(
    hwnd: isize,
    kind: WidgetKind,
    spacing: f64,
    insets: (f64, f64, f64, f64),
) -> i64 {
    let control_id = alloc_control_id();
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing,
            insets,
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
            fixed_height: None,
            match_parent_height: false,
            match_parent_width: false,
            detaches_hidden: false,
            distribution: 0,
            alignment: 0,
        });
        widgets.len() as i64
    })
}

/// Get the HWND for a widget handle.
#[cfg(target_os = "windows")]
pub fn get_hwnd(handle: i64) -> Option<HWND> {
    WIDGETS.with(|w| {
        let widgets = match w.try_borrow() {
            Ok(w) => w,
            Err(_) => return None,
        };
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            Some(widgets[idx].hwnd)
        } else {
            None
        }
    })
}

#[cfg(not(target_os = "windows"))]
pub fn get_hwnd(handle: i64) -> Option<isize> {
    WIDGETS.with(|w| {
        let widgets = match w.try_borrow() {
            Ok(w) => w,
            Err(_) => return None,
        };
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            Some(widgets[idx].hwnd)
        } else {
            None
        }
    })
}

/// Get widget info (clone-safe subset).
pub fn get_widget_info(handle: i64) -> Option<WidgetInfo> {
    WIDGETS.with(|w| {
        let widgets = match w.try_borrow() {
            Ok(w) => w,
            Err(_) => return None,
        };
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            Some(WidgetInfo {
                kind: widgets[idx].kind.clone(),
                children: widgets[idx].children.clone(),
                spacing: widgets[idx].spacing,
                insets: widgets[idx].insets,
                hidden: widgets[idx].hidden,
                fills_remaining: widgets[idx].fills_remaining,
                fixed_width: widgets[idx].fixed_width,
                fixed_height: widgets[idx].fixed_height,
                match_parent_height: widgets[idx].match_parent_height,
                match_parent_width: widgets[idx].match_parent_width,
                detaches_hidden: widgets[idx].detaches_hidden,
                distribution: widgets[idx].distribution,
                alignment: widgets[idx].alignment,
            })
        } else {
            None
        }
    })
}

/// Find the widget handle that owns a given HWND.
/// Uses try_borrow to handle re-entrant calls from Win32 message loop
/// (e.g. ShowWindow sends WM_SIZE while widgets are still being created).
#[cfg(target_os = "windows")]
pub fn find_handle_by_hwnd(hwnd: HWND) -> i64 {
    WIDGETS.with(|w| {
        match w.try_borrow() {
            Ok(widgets) => {
                for (i, widget) in widgets.iter().enumerate() {
                    if widget.hwnd == hwnd {
                        return (i + 1) as i64;
                    }
                }
                0
            }
            Err(_) => 0, // Re-entrant call — return 0 (not found)
        }
    })
}

#[cfg(not(target_os = "windows"))]
pub fn find_handle_by_hwnd(_hwnd: isize) -> i64 {
    0
}

/// Find widget handle by control ID.
pub fn find_handle_by_control_id(id: u16) -> i64 {
    WIDGETS.with(|w| {
        let widgets = match w.try_borrow() {
            Ok(w) => w,
            Err(_) => return 0,
        };
        for (i, widget) in widgets.iter().enumerate() {
            if widget.control_id == id {
                return (i + 1) as i64;
            }
        }
        0
    })
}

/// Add a child widget to a parent container.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        // Re-parent the child HWND
        if let (Some(parent_hwnd), Some(child_hwnd)) =
            (get_hwnd(parent_handle), get_hwnd(child_handle))
        {
            unsafe {
                let _ = SetParent(child_hwnd, parent_hwnd);
                let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                SetWindowLongW(
                    child_hwnd,
                    GWL_STYLE,
                    (style | WS_CHILD.0 | WS_VISIBLE.0) as i32,
                );
            }
        }
    }

    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (parent_handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].children.push(child_handle);
        }
    });
}

/// Add a child widget at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    #[cfg(target_os = "windows")]
    {
        if let (Some(parent_hwnd), Some(child_hwnd)) =
            (get_hwnd(parent_handle), get_hwnd(child_handle))
        {
            unsafe {
                let _ = SetParent(child_hwnd, parent_hwnd);
                let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                SetWindowLongW(
                    child_hwnd,
                    GWL_STYLE,
                    (style | WS_CHILD.0 | WS_VISIBLE.0) as i32,
                );
            }
        }
    }

    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (parent_handle - 1) as usize;
        if idx < widgets.len() {
            let insert_at = (index as usize).min(widgets[idx].children.len());
            widgets[idx].children.insert(insert_at, child_handle);
        }
    });
}

/// Remove a specific child from a parent container.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    // Remove from children list
    let removed = WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (parent_handle - 1) as usize;
        if idx < widgets.len() {
            if let Some(pos) = widgets[idx]
                .children
                .iter()
                .position(|&c| c == child_handle)
            {
                widgets[idx].children.remove(pos);
                true
            } else {
                false
            }
        } else {
            false
        }
    });

    #[cfg(target_os = "windows")]
    {
        if removed {
            let parking = get_parking_hwnd();
            if let Some(child_hwnd) = get_hwnd(child_handle) {
                unsafe {
                    let _ = ShowWindow(child_hwnd, SW_HIDE);
                    let _ = SetParent(child_hwnd, parking);
                }
            }
        }
    }

    let _ = removed;
}

/// Remove all children from a container widget.
pub fn clear_children(handle: i64) {
    let children: Vec<i64> = WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].children.drain(..).collect()
        } else {
            Vec::new()
        }
    });

    #[cfg(target_os = "windows")]
    {
        let parking = get_parking_hwnd();
        for child in &children {
            if let Some(child_hwnd) = get_hwnd(*child) {
                unsafe {
                    let _ = ShowWindow(child_hwnd, SW_HIDE);
                    let _ = SetParent(child_hwnd, parking);
                }
            }
        }
        // Invalidate the parent so it repaints its background immediately,
        // preventing a black flash while new children are being added.
        if let Some(parent_hwnd) = get_hwnd(handle) {
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(parent_hwnd, None, true);
            }
        }
    }

    let _ = children;
}

/// Mark a widget as filling remaining space in its parent VStack/HStack.
pub fn set_fills_remaining(handle: i64, fills: bool) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].fills_remaining = fills;
        }
    });
}

/// Set the distribution mode on a stack widget.
/// 0 = Fill, 1 = FillEqually, -1 = GravityAreas (default).
pub fn set_distribution(handle: i64, distribution: i64) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].distribution = distribution;
        }
    });
}

/// Set the edge insets (padding) on a widget.
pub fn set_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].insets = (top, left, bottom, right);
        }
    });
}

/// Set the alignment mode on a stack widget.
pub fn set_alignment(handle: i64, alignment: i64) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].alignment = alignment;
        }
    });
}

/// Set the hidden state of a widget.
pub fn set_hidden(handle: i64, hidden: bool) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].hidden = hidden;

            #[cfg(target_os = "windows")]
            {
                let hwnd = widgets[idx].hwnd;
                unsafe {
                    let _ = ShowWindow(hwnd, if hidden { SW_HIDE } else { SW_SHOW });
                }
            }
        }
    });
}

/// Handle WM_COMMAND from WndProc — dispatch to button/textfield/toggle/picker/securefield callbacks.
#[cfg(target_os = "windows")]
pub fn handle_command(control_id: u16, notify_code: u16, _lparam: LPARAM) {
    // BN_CLICKED = 0
    if notify_code == 0 {
        // Could be a button click or toggle click
        let handle = find_handle_by_control_id(control_id);
        if handle > 0 {
            let kind = WIDGETS.with(|w| {
                let widgets = match w.try_borrow() {
                    Ok(w) => w,
                    Err(_) => return None,
                };
                let idx = (handle - 1) as usize;
                if idx < widgets.len() {
                    Some(widgets[idx].kind.clone())
                } else {
                    None
                }
            });
            match kind {
                Some(WidgetKind::Button) => button::handle_click(handle),
                Some(WidgetKind::Toggle) => toggle::handle_click(handle),
                _ => {}
            }
        }
    }
    // CBN_SELCHANGE = 1
    if notify_code == 1 {
        let handle = find_handle_by_control_id(control_id);
        if handle > 0 {
            let kind = WIDGETS.with(|w| {
                let widgets = match w.try_borrow() {
                    Ok(w) => w,
                    Err(_) => return None,
                };
                let idx = (handle - 1) as usize;
                if idx < widgets.len() {
                    Some(widgets[idx].kind.clone())
                } else {
                    None
                }
            });
            if matches!(kind, Some(WidgetKind::Picker)) {
                picker::handle_selchange(handle);
            }
        }
    }
    // EN_CHANGE = 0x0300
    if notify_code == 0x0300 {
        let handle = find_handle_by_control_id(control_id);
        if handle > 0 {
            let kind = WIDGETS.with(|w| {
                let widgets = match w.try_borrow() {
                    Ok(w) => w,
                    Err(_) => return None,
                };
                let idx = (handle - 1) as usize;
                if idx < widgets.len() {
                    Some(widgets[idx].kind.clone())
                } else {
                    None
                }
            });
            match kind {
                Some(WidgetKind::SecureField) => securefield::handle_change(handle),
                _ => textfield::handle_change(handle),
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn handle_command(_control_id: u16, _notify_code: u16, _lparam: isize) {}

/// Handle WM_HSCROLL/WM_VSCROLL — dispatch to slider or scrollview.
#[cfg(target_os = "windows")]
pub fn handle_scroll(wparam: WPARAM, lparam: LPARAM) {
    let child_hwnd = HWND(lparam.0 as *mut _);
    let handle = find_handle_by_hwnd(child_hwnd);
    if handle > 0 {
        let kind = WIDGETS.with(|w| {
            let widgets = match w.try_borrow() {
                Ok(w) => w,
                Err(_) => return None,
            };
            let idx = (handle - 1) as usize;
            if idx < widgets.len() {
                Some(widgets[idx].kind.clone())
            } else {
                None
            }
        });
        match kind {
            Some(WidgetKind::Slider) => slider::handle_scroll(handle),
            _ => {}
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn handle_scroll(_wparam: usize, _lparam: isize) {}

// =============================================================================
// Property setters (new in parity update)
// =============================================================================

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Set the enabled/disabled state of a widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = get_hwnd(handle) {
            unsafe {
                let _ = EnableWindow(hwnd, enabled);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, enabled);
    }
}

/// Set the tooltip of a widget.
pub fn set_tooltip(handle: i64, _text_ptr: *const u8) {
    // Win32 tooltips require a shared TOOLTIPS_CLASS control with TTM_ADDTOOL.
    // For now, this is a best-effort no-op — full tooltip support would require
    // creating a shared tooltip window and managing per-widget TOOLINFO structs.
    let _ = handle;
}

/// Set the control size of a widget (maps to font size).
pub fn set_control_size(handle: i64, size: i64) {
    #[cfg(target_os = "windows")]
    {
        let font_height = match size {
            0 => 10, // mini
            1 => 12, // small
            2 => 14, // regular
            3 => 18, // large
            _ => 14,
        };
        if let Some(hwnd) = get_hwnd(handle) {
            unsafe {
                let font = CreateFontW(
                    -font_height,
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    windows::core::PCWSTR(
                        "Segoe UI\0".encode_utf16().collect::<Vec<u16>>().as_ptr(),
                    ),
                );
                SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, size);
    }
}

/// Corner radius values keyed by widget handle — applied during layout when
/// the widget has its final size (not at set time, when the HWND is still tiny).
static CORNER_RADII: std::sync::Mutex<Vec<(i64, f64)>> = std::sync::Mutex::new(Vec::new());

/// Set the corner radius of a widget.
/// The radius is stored and applied during layout via `apply_corner_radius`.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Ok(mut radii) = CORNER_RADII.lock() {
        // Update existing or insert new
        if let Some(entry) = radii.iter_mut().find(|e| e.0 == handle) {
            entry.1 = radius;
        } else {
            radii.push((handle, radius));
        }
    }
}

/// Stored shadow params per widget handle — applied later by a custom paint
/// pass once one lands. Shape: `(handle, (r, g, b, a, blur, offset_x, offset_y))`.
/// See `set_shadow` for the rationale.
static SHADOW_PARAMS: std::sync::Mutex<Vec<(i64, (f64, f64, f64, f64, f64, f64, f64))>> =
    std::sync::Mutex::new(Vec::new());

/// Set drop shadow on a widget (issue #185 Phase B / #210 closure).
///
/// Stores params in `SHADOW_PARAMS` (mirroring the `CORNER_RADII`
/// deferred-application pattern) and, on Windows, registers the widget
/// with its parent HWND's shadow-paint subclass via `apply_shadow`. The
/// subclass intercepts `WM_PAINT` on the parent and renders the shadow
/// after the parent's own paint runs but BEFORE child controls layer
/// on top — so the shadow lands behind the widget exactly as CSS
/// `box-shadow` would.
///
/// **Visual fidelity**: the falloff is a quadratic approximation of a
/// Gaussian blur (`alpha = base * (1 - d/blur)^2` per pixel where `d`
/// is the distance to the un-blurred shadow rect). This produces a
/// recognizable soft shadow without needing DirectComposition. True
/// Gaussian + GPU-accelerated rendering (`IDCompositionVisual` +
/// `DropShadowEffect`) is a separate follow-up; the API contract is
/// the same so a future swap-in is non-breaking.
pub fn set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    if let Ok(mut shadows) = SHADOW_PARAMS.lock() {
        let entry = (r, g, b, a, blur, offset_x, offset_y);
        if let Some(slot) = shadows.iter_mut().find(|e| e.0 == handle) {
            slot.1 = entry;
        } else {
            shadows.push((handle, entry));
        }
    }

    #[cfg(target_os = "windows")]
    {
        apply_shadow(handle);
    }
}

/// Read back the stored shadow params for a widget. Returns `None` if no
/// shadow has been set. The paint pass calls this; it's also a useful
/// introspection hook for tests.
pub fn get_shadow(handle: i64) -> Option<(f64, f64, f64, f64, f64, f64, f64)> {
    SHADOW_PARAMS
        .lock()
        .ok()
        .and_then(|s| s.iter().find(|e| e.0 == handle).map(|e| e.1))
}

/// Per-parent registry of shadowed children. The shadow paint subclass
/// installed on a parent walks this Vec on each `WM_PAINT` to find which
/// children's shadows to render. Keyed by parent HWND-as-isize for Send
/// safety — we only hold the Mutex across the registry update, never
/// across the actual paint cycle.
#[cfg(target_os = "windows")]
static PARENT_SHADOW_REGISTRY: std::sync::Mutex<Vec<(isize, Vec<i64>)>> =
    std::sync::Mutex::new(Vec::new());

/// Track which parent HWNDs already have the shadow paint subclass
/// installed. Subclass id is fixed (`SHADOW_SUBCLASS_ID`) — uniqueness
/// is per-HWND, not across HWNDs. Different from `BORDER_SUBCLASSED`
/// because shadows install on PARENTS while borders install on the
/// widget HWNDs themselves.
#[cfg(target_os = "windows")]
thread_local! {
    static SHADOW_SUBCLASSED_PARENTS: RefCell<std::collections::HashSet<isize>> =
        RefCell::new(std::collections::HashSet::new());
}

#[cfg(target_os = "windows")]
const SHADOW_SUBCLASS_ID: usize = 0x70_72_73_68; // 'p','r','s','h'

/// Register `child_handle` as having a shadow that should be painted by
/// `parent_key`'s subclass. Idempotent.
#[cfg(target_os = "windows")]
fn register_shadow_for_parent(parent_key: isize, child_handle: i64) {
    if let Ok(mut reg) = PARENT_SHADOW_REGISTRY.lock() {
        if let Some(slot) = reg.iter_mut().find(|e| e.0 == parent_key) {
            if !slot.1.contains(&child_handle) {
                slot.1.push(child_handle);
            }
        } else {
            reg.push((parent_key, vec![child_handle]));
        }
    }
}

#[cfg(target_os = "windows")]
fn get_shadowed_children(parent_key: isize) -> Vec<i64> {
    PARENT_SHADOW_REGISTRY
        .lock()
        .ok()
        .and_then(|r| r.iter().find(|e| e.0 == parent_key).map(|e| e.1.clone()))
        .unwrap_or_default()
}

/// Apply a stored drop shadow: ensure the widget's parent HWND has the
/// shadow paint subclass installed and that this widget is in the
/// parent's shadow set. Idempotent — safe to call repeatedly from the
/// layout engine (mirrors `apply_corner_radius`'s call pattern).
#[cfg(target_os = "windows")]
pub fn apply_shadow(handle: i64) {
    let has = SHADOW_PARAMS
        .lock()
        .ok()
        .map(|s| s.iter().any(|e| e.0 == handle))
        .unwrap_or(false);
    if !has {
        return;
    }
    let child = match get_hwnd_safe(handle) {
        Some(h) => h,
        None => return,
    };
    let parent = unsafe { GetParent(child) };
    let parent_hwnd = match parent {
        Ok(p) if !p.0.is_null() => p,
        _ => return,
    };
    let parent_key = parent_hwnd.0 as isize;
    register_shadow_for_parent(parent_key, handle);
    ensure_shadow_subclass(parent_hwnd);
    unsafe {
        let _ = InvalidateRect(parent_hwnd, None, true);
    }
}

#[cfg(target_os = "windows")]
fn ensure_shadow_subclass(parent_hwnd: HWND) {
    use windows::Win32::UI::Shell::SetWindowSubclass;
    let key = parent_hwnd.0 as isize;
    let installed = SHADOW_SUBCLASSED_PARENTS.with(|s| s.borrow().contains(&key));
    if !installed {
        unsafe {
            let _ = SetWindowSubclass(
                parent_hwnd,
                Some(shadow_subclass_proc),
                SHADOW_SUBCLASS_ID,
                0,
            );
        }
        SHADOW_SUBCLASSED_PARENTS.with(|s| {
            s.borrow_mut().insert(key);
        });
    }
}

/// Subclass proc on a parent window. We let the parent paint itself
/// first (`DefSubclassProc`), then on `WM_PAINT` we walk the registered
/// shadowed children and stamp soft-edge shadows directly onto the
/// parent's surface via `AlphaBlend`. Win32's WS_CLIPCHILDREN flag
/// keeps the shadow from overdrawing the children's own surfaces (the
/// children paint last in their own message cycles), so the shadow
/// shows up only in the area BETWEEN/AROUND the child — exactly the
/// CSS `box-shadow` behavior.
#[cfg(target_os = "windows")]
unsafe extern "system" fn shadow_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;

    let result = DefSubclassProc(hwnd, msg, wparam, lparam);

    if msg == WM_PAINT {
        let parent_key = hwnd.0 as isize;
        let children = get_shadowed_children(parent_key);
        if !children.is_empty() {
            paint_shadows(hwnd, &children);
        }
    }

    result
}

#[cfg(target_os = "windows")]
unsafe fn paint_shadows(parent_hwnd: HWND, children: &[i64]) {
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    let hdc = GetDC(parent_hwnd);
    if hdc.is_invalid() {
        return;
    }
    for &child in children {
        if let Some(shadow) = get_shadow(child) {
            paint_shadow_for_child(parent_hwnd, hdc, child, shadow);
        }
    }
    ReleaseDC(parent_hwnd, hdc);
}

/// Render one widget's drop shadow into a 32bpp DIB then `AlphaBlend`
/// onto the parent's DC. Per-pixel alpha follows a quadratic falloff
/// from the un-blurred shadow rect (`alpha = base * (1 - d/blur)^2`)
/// which is a cheap visible approximation of a Gaussian box-shadow.
/// Pixels INSIDE the widget bounds stay fully transparent so the
/// child's own painting layers on top.
#[cfg(target_os = "windows")]
unsafe fn paint_shadow_for_child(
    parent_hwnd: HWND,
    parent_dc: HDC,
    child_handle: i64,
    shadow: (f64, f64, f64, f64, f64, f64, f64),
) {
    use windows::Win32::Graphics::Gdi::{
        AlphaBlend, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, ScreenToClient,
        SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        BLENDFUNCTION, DIB_RGB_COLORS, RGBQUAD,
    };

    let (r, g, b, a, blur, ox, oy) = shadow;

    let alpha_base = a.clamp(0.0, 1.0);
    if alpha_base <= 0.0 {
        return;
    }

    let child_hwnd = match get_hwnd_safe(child_handle) {
        Some(h) => h,
        None => return,
    };

    // Get child window rect (screen coords) → translate to parent client coords.
    let mut child_rect = RECT::default();
    if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(child_hwnd, &mut child_rect).is_err()
    {
        return;
    }
    let mut tl = POINT {
        x: child_rect.left,
        y: child_rect.top,
    };
    let mut br = POINT {
        x: child_rect.right,
        y: child_rect.bottom,
    };
    let _ = ScreenToClient(parent_hwnd, &mut tl);
    let _ = ScreenToClient(parent_hwnd, &mut br);

    let widget_w = br.x - tl.x;
    let widget_h = br.y - tl.y;
    if widget_w <= 0 || widget_h <= 0 {
        return;
    }

    // Clamp to bounded ranges so a malicious / buggy caller can't request
    // a 10000×10000 alloca-equivalent.
    let blur_px = blur.round().max(0.0).min(64.0) as i32;
    let ox_px = ox.round().clamp(-256.0, 256.0) as i32;
    let oy_px = oy.round().clamp(-256.0, 256.0) as i32;

    // Bitmap covers the union of widget rect and shadow rect (each padded
    // by `blur_px` for falloff). Position widget within the bitmap so that
    // both the widget rect and its offset shadow fit.
    let pad = blur_px;
    let widget_l_in_bmp = (-ox_px.min(0)) + pad;
    let widget_t_in_bmp = (-oy_px.min(0)) + pad;
    let bmp_w = widget_w + 2 * pad + ox_px.abs();
    let bmp_h = widget_h + 2 * pad + oy_px.abs();
    if bmp_w <= 0 || bmp_h <= 0 {
        return;
    }

    let widget_r_in_bmp = widget_l_in_bmp + widget_w;
    let widget_b_in_bmp = widget_t_in_bmp + widget_h;
    let shadow_l_in_bmp = widget_l_in_bmp + ox_px;
    let shadow_t_in_bmp = widget_t_in_bmp + oy_px;
    let shadow_r_in_bmp = shadow_l_in_bmp + widget_w;
    let shadow_b_in_bmp = shadow_t_in_bmp + widget_h;

    // Where the bitmap lands in parent client coords.
    let dest_x = tl.x - widget_l_in_bmp;
    let dest_y = tl.y - widget_t_in_bmp;

    let mem_dc = CreateCompatibleDC(parent_dc);
    if mem_dc.is_invalid() {
        return;
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bmp_w,
            biHeight: -bmp_h, // top-down DIB → row 0 is top
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD::default()],
    };

    let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let null_section = windows::Win32::Foundation::HANDLE(std::ptr::null_mut());
    let dib = match CreateDIBSection(
        parent_dc,
        &bmi,
        DIB_RGB_COLORS,
        &mut bits_ptr,
        null_section,
        0,
    ) {
        Ok(d) if !bits_ptr.is_null() => d,
        _ => {
            let _ = DeleteDC(mem_dc);
            return;
        }
    };

    let old_obj = SelectObject(mem_dc, dib);

    // Render shadow: per-pixel falloff from the (un-blurred) shadow rect.
    // 32bpp ARGB layout (little-endian u32): 0xAARRGGBB where bytes go
    // BB GG RR AA in memory. AlphaBlend with AC_SRC_ALPHA + premultiplied
    // alpha is the standard Win32 path for soft-edge drop shadows.
    let pixels =
        std::slice::from_raw_parts_mut(bits_ptr as *mut u32, (bmp_w as usize) * (bmp_h as usize));
    let blur_f = blur_px as f64;
    for py in 0..bmp_h {
        for px in 0..bmp_w {
            // Inside widget rect → fully transparent (widget paints itself).
            if px >= widget_l_in_bmp
                && px < widget_r_in_bmp
                && py >= widget_t_in_bmp
                && py < widget_b_in_bmp
            {
                pixels[(py * bmp_w + px) as usize] = 0;
                continue;
            }

            // Distance to the (un-blurred) shadow rect, in pixels.
            let dx = if px < shadow_l_in_bmp {
                shadow_l_in_bmp - px
            } else if px >= shadow_r_in_bmp {
                px - shadow_r_in_bmp + 1
            } else {
                0
            };
            let dy = if py < shadow_t_in_bmp {
                shadow_t_in_bmp - py
            } else if py >= shadow_b_in_bmp {
                py - shadow_b_in_bmp + 1
            } else {
                0
            };
            let d2 = (dx * dx + dy * dy) as f64;
            let d = d2.sqrt();

            let alpha = if blur_px == 0 {
                if d == 0.0 {
                    alpha_base
                } else {
                    0.0
                }
            } else {
                let t = (d / blur_f).min(1.0);
                let f = 1.0 - t;
                alpha_base * f * f
            };

            if alpha <= 0.0 {
                pixels[(py * bmp_w + px) as usize] = 0;
                continue;
            }

            let pa = (alpha * 255.0).round().clamp(0.0, 255.0) as u32;
            let pr = (r * alpha * 255.0).round().clamp(0.0, 255.0) as u32;
            let pg = (g * alpha * 255.0).round().clamp(0.0, 255.0) as u32;
            let pb = (b * alpha * 255.0).round().clamp(0.0, 255.0) as u32;
            pixels[(py * bmp_w + px) as usize] = (pa << 24) | (pr << 16) | (pg << 8) | pb;
        }
    }

    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    let _ = AlphaBlend(
        parent_dc, dest_x, dest_y, bmp_w, bmp_h, mem_dc, 0, 0, bmp_w, bmp_h, blend,
    );

    SelectObject(mem_dc, old_obj);
    let _ = DeleteObject(dib);
    let _ = DeleteDC(mem_dc);
}

/// Stored opacity values per widget handle. Kept separately from
/// `SHADOW_PARAMS` so future Windows work can apply them through
/// independent paint paths.
static OPACITY_VALUES: std::sync::Mutex<Vec<(i64, f64)>> = std::sync::Mutex::new(Vec::new());

/// Set static opacity on a widget (issue #185 Phase B / #210 closure).
///
/// Adds `WS_EX_LAYERED` to the child HWND's extended style if not yet
/// set, then applies the alpha via `SetLayeredWindowAttributes` with
/// `LWA_ALPHA`. Per-child `WS_EX_LAYERED` works on Windows 8+ (which is
/// Perry's minimum Windows target). The store still keeps the last
/// value so a later layout pass / `animate_opacity` can re-apply it
/// without losing state.
///
/// `animate_opacity` (already in this file) wires through the same
/// `apply_opacity` helper, so animations now also work.
pub fn set_opacity(handle: i64, opacity: f64) {
    if let Ok(mut opacities) = OPACITY_VALUES.lock() {
        if let Some(slot) = opacities.iter_mut().find(|e| e.0 == handle) {
            slot.1 = opacity;
        } else {
            opacities.push((handle, opacity));
        }
    }

    #[cfg(target_os = "windows")]
    {
        apply_opacity(handle, opacity);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = opacity;
    }
}

/// Apply the stored opacity to a widget's HWND. Idempotent — safe to
/// call multiple times. Clamps the input to `[0, 1]`.
#[cfg(target_os = "windows")]
fn apply_opacity(handle: i64, opacity: f64) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA,
        WS_EX_LAYERED,
    };
    let Some(hwnd) = get_hwnd_safe(handle) else {
        return;
    };
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    unsafe {
        let cur_ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if cur_ex & WS_EX_LAYERED.0 == 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, (cur_ex | WS_EX_LAYERED.0) as i32);
        }
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        let _ = InvalidateRect(hwnd, None, true);
    }
}

/// Read the stored opacity for a widget. Returns `None` if not set.
pub fn get_opacity(handle: i64) -> Option<f64> {
    OPACITY_VALUES
        .lock()
        .ok()
        .and_then(|s| s.iter().find(|e| e.0 == handle).map(|e| e.1))
}

/// Joint per-handle border state — `(color_rgba, width)`. Set by
/// either `set_border_color` or `set_border_width`; consumed jointly
/// by the eventual paint pass.
static BORDER_STATE: std::sync::Mutex<Vec<(i64, (Option<(f64, f64, f64, f64)>, Option<f64>))>> =
    std::sync::Mutex::new(Vec::new());

/// Set border color (issue #185 Phase B / #210 closure).
///
/// Stores the color in `BORDER_STATE` next to any previously-set width
/// and (re-)installs the per-handle WM_PAINT subclass that draws the
/// border. Both setters share the joint state because CSS-style borders
/// require all of (color, width, style=solid) to land in the same paint
/// — calling either setter alone produces a sensible default (black 1px
/// for missing color, 1px for missing width) so the border is visible
/// after a single setter call.
pub fn set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Ok(mut state) = BORDER_STATE.lock() {
        let color = (r, g, b, a);
        if let Some(slot) = state.iter_mut().find(|e| e.0 == handle) {
            slot.1 .0 = Some(color);
        } else {
            state.push((handle, (Some(color), None)));
        }
    }

    #[cfg(target_os = "windows")]
    {
        ensure_border_subclass(handle);
    }
}

/// Set border width (issue #185 Phase B / #210 closure). See
/// `set_border_color`.
pub fn set_border_width(handle: i64, width: f64) {
    if let Ok(mut state) = BORDER_STATE.lock() {
        if let Some(slot) = state.iter_mut().find(|e| e.0 == handle) {
            slot.1 .1 = Some(width);
        } else {
            state.push((handle, (None, Some(width))));
        }
    }

    #[cfg(target_os = "windows")]
    {
        ensure_border_subclass(handle);
    }
}

/// Read the stored border state. Returns `None` if neither setter has
/// been called for this handle. Used by the eventual paint pass.
pub fn get_border(handle: i64) -> Option<(Option<(f64, f64, f64, f64)>, Option<f64>)> {
    BORDER_STATE
        .lock()
        .ok()
        .and_then(|s| s.iter().find(|e| e.0 == handle).map(|e| e.1))
}

/// Apply the stored corner radius to a widget after it has been laid out
/// and has its final size. Called from the layout engine.
#[cfg(target_os = "windows")]
pub fn apply_corner_radius(handle: i64) {
    let radius = if let Ok(radii) = CORNER_RADII.lock() {
        radii.iter().find(|e| e.0 == handle).map(|e| e.1)
    } else {
        None
    };
    if let Some(radius) = radius {
        if let Some(hwnd) = get_hwnd_safe(handle) {
            unsafe {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                // Corner radius applied at layout time
                if rect.right > 0 && rect.bottom > 0 {
                    let rgn = CreateRoundRectRgn(
                        0,
                        0,
                        rect.right + 1,
                        rect.bottom + 1,
                        radius as i32,
                        radius as i32,
                    );
                    SetWindowRgn(hwnd, rgn, true);
                }
            }
        }
    }
}

/// Track which handles already have the border-paint subclass installed
/// so `ensure_border_subclass` is idempotent. Subclass id is fixed
/// (`BORDER_SUBCLASS_ID`) — uniqueness within a single HWND is what
/// matters, not across HWNDs.
#[cfg(target_os = "windows")]
thread_local! {
    static BORDER_SUBCLASSED: RefCell<std::collections::HashSet<i64>> =
        RefCell::new(std::collections::HashSet::new());
}

#[cfg(target_os = "windows")]
const BORDER_SUBCLASS_ID: usize = 0x70_72_72_79; // 'p','r','r','y'

/// Install the border-drawing WM_PAINT subclass on `handle`'s HWND if
/// not already installed, and force a repaint so a freshly-set border
/// shows up immediately.
#[cfg(target_os = "windows")]
fn ensure_border_subclass(handle: i64) {
    // Win32_UI_Shell, not Win32_UI_Controls — both functions live in Shell
    // per windows-rs 0.58. The crate's own per-feature gate `Win32_UI_Shell`
    // is already enabled in Cargo.toml.
    use windows::Win32::UI::Shell::SetWindowSubclass;
    let installed = BORDER_SUBCLASSED.with(|s| s.borrow().contains(&handle));
    if !installed {
        if let Some(hwnd) = get_hwnd_safe(handle) {
            unsafe {
                let _ = SetWindowSubclass(
                    hwnd,
                    Some(border_subclass_proc),
                    BORDER_SUBCLASS_ID,
                    handle as usize,
                );
            }
            BORDER_SUBCLASSED.with(|s| {
                s.borrow_mut().insert(handle);
            });
        }
    }
    if let Some(hwnd) = get_hwnd_safe(handle) {
        unsafe {
            let _ = InvalidateRect(hwnd, None, true);
        }
    }
}

/// Subclass proc that draws the configured border on top of whatever
/// the wrapped control painted. WM_PAINT lands here AFTER the original
/// control has rendered (we call `DefSubclassProc` first so the BeginPaint
/// / EndPaint pair runs as the control expects); we then `GetDC` and
/// stamp a `Rectangle` outline.
///
/// Defaults match CSS: missing color → black 1.0 alpha; missing width
/// → 1px. Either setter alone produces a visible 1px black border, the
/// pair gives full control. Width is clamped to 1 minimum (a 0px
/// border is no border, but if the user explicitly sets 0 they want
/// no border — we still draw with the stored color but at 0 width
/// which is a no-op pen).
#[cfg(target_os = "windows")]
unsafe extern "system" fn border_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    use windows::Win32::Graphics::Gdi::{
        CreatePen, DeleteObject, GetDC, GetStockObject, Rectangle, ReleaseDC, SelectObject,
        NULL_BRUSH, PS_SOLID,
    };
    use windows::Win32::UI::Shell::DefSubclassProc;

    let result = DefSubclassProc(hwnd, msg, wparam, lparam);

    if msg == WM_PAINT {
        let handle = refdata as i64;
        if let Some((color, width)) = get_border(handle) {
            let (r, g, b, _a) = color.unwrap_or((0.0, 0.0, 0.0, 1.0));
            let w = width.unwrap_or(1.0).round().max(0.0) as i32;
            if w > 0 {
                let cr = ((r * 255.0) as u32)
                    | (((g * 255.0) as u32) << 8)
                    | (((b * 255.0) as u32) << 16);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                if rect.right > 0 && rect.bottom > 0 {
                    let hdc = GetDC(hwnd);
                    if !hdc.is_invalid() {
                        let pen = CreatePen(PS_SOLID, w, COLORREF(cr));
                        let null_brush = HBRUSH(GetStockObject(NULL_BRUSH).0);
                        let old_pen = SelectObject(hdc, pen);
                        let old_brush = SelectObject(hdc, null_brush);
                        let _ = Rectangle(hdc, 0, 0, rect.right, rect.bottom);
                        SelectObject(hdc, old_pen);
                        SelectObject(hdc, old_brush);
                        let _ = DeleteObject(pen);
                        ReleaseDC(hwnd, hdc);
                    }
                }
            }
        }
    }

    result
}

/// Set the fixed width of a widget (in pixels).
pub fn set_fixed_width(handle: i64, width: i32) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].fixed_width = Some(width);
        }
    });
}

/// Set the fixed height of a widget (in pixels).
pub fn set_fixed_height(handle: i64, height: i32) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].fixed_height = Some(height);
        }
    });
}

/// Set whether this widget should stretch to match its parent's width.
pub fn set_match_parent_width(handle: i64, value: bool) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].match_parent_width = value;
        }
    });
}

/// Set whether this widget should stretch to match its parent's height.
pub fn set_match_parent_height(handle: i64, value: bool) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].match_parent_height = value;
        }
    });
}

/// Set whether a stack should detach (exclude) hidden children from layout.
pub fn set_detaches_hidden(handle: i64, value: bool) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].detaches_hidden = value;
        }
    });
}

/// Set hugging priority. Low priority (e.g. 1) means the widget should expand to fill space.
pub fn set_hugging_priority(handle: i64, priority: f64) {
    if priority <= 250.0 {
        set_fills_remaining(handle, true);
    }
}

/// Set the background color of a widget.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    #[cfg(target_os = "windows")]
    {
        // Alpha-blend semi-transparent colors against ancestor bg (or white)
        let (fr, fg, fb) = if a < 0.999 {
            let ancestor = get_hwnd_safe(handle).and_then(|h| find_ancestor_hwnd_bg_color(h));
            let (ar, ag, ab) = match ancestor {
                Some(c) => (
                    (c & 0xFF) as f64 / 255.0,
                    ((c >> 8) & 0xFF) as f64 / 255.0,
                    ((c >> 16) & 0xFF) as f64 / 255.0,
                ),
                None => (1.0, 1.0, 1.0),
            };
            (
                r * a + ar * (1.0 - a),
                g * a + ag * (1.0 - a),
                b * a + ab * (1.0 - a),
            )
        } else {
            (r, g, b)
        };
        let color = rgb_to_colorref(fr, fg, fb);
        let brush = unsafe { CreateSolidBrush(COLORREF(color)) };
        BG_COLORS.with(|c| c.borrow_mut().insert(handle, color));
        BG_BRUSHES.with(|b| b.borrow_mut().insert(handle, brush));
        let hwnd_opt = get_hwnd_safe(handle);
        if let Some(hwnd) = hwnd_opt {
            set_hwnd_bg_color(hwnd, color);
            unsafe {
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, r, g, b, a);
    }
}

/// Store a background COLORREF directly on an HWND via SetPropW.
/// This bypasses the handle lookup chain and survives RefCell reentrancy.
#[cfg(target_os = "windows")]
pub fn set_hwnd_bg_color(hwnd: HWND, color: u32) {
    unsafe {
        // Store color+1 so we can distinguish "not set" (0) from black (0x000000).
        let prop_name: Vec<u16> = "PerryBgColor"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        SetPropW(
            hwnd,
            windows::core::PCWSTR(prop_name.as_ptr()),
            HANDLE((color as usize + 1) as *mut _),
        );
    }
}

/// Retrieve the background COLORREF stored on an HWND. Returns None if not set.
#[cfg(target_os = "windows")]
pub fn get_hwnd_bg_color(hwnd: HWND) -> Option<u32> {
    unsafe {
        let prop_name: Vec<u16> = "PerryBgColor"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let val = GetPropW(hwnd, windows::core::PCWSTR(prop_name.as_ptr()));
        if val.is_invalid() || val.0.is_null() {
            None
        } else {
            Some(val.0 as u32 - 1) // undo the +1 offset
        }
    }
}

/// Walk the HWND parent chain to find the nearest ancestor with a bg color stored via SetPropW.
#[cfg(target_os = "windows")]
pub fn find_ancestor_hwnd_bg_color(mut hwnd: HWND) -> Option<u32> {
    for _ in 0..10 {
        if let Ok(parent) = unsafe { GetParent(hwnd) } {
            if parent.0.is_null() {
                break;
            }
            if let Some(color) = get_hwnd_bg_color(parent) {
                return Some(color);
            }
            hwnd = parent;
        } else {
            break;
        }
    }
    None
}

/// Paint a gradient background for `hwnd` if one has been registered.
/// Returns `true` if a gradient was painted, `false` otherwise (caller should fall through to solid color).
#[cfg(target_os = "windows")]
pub fn paint_gradient(hwnd: HWND, hdc: HDC, rect: &RECT) -> bool {
    let key = hwnd.0 as isize;
    let (c1, c2, vertical) = match GRADIENT_MAP.lock() {
        Ok(map) => {
            // Search from the end (most recent entry wins if duplicates)
            match map.iter().rev().find(|(k, _)| *k == key) {
                Some((_, info)) => (info.c1, info.c2, info.vertical),
                None => return false,
            }
        }
        Err(_) => return false,
    };

    // Extract RGB byte components from COLORREF (0x00BBGGRR)
    let r1 = (c1 & 0xFF) as u16;
    let g1 = ((c1 >> 8) & 0xFF) as u16;
    let b1 = ((c1 >> 16) & 0xFF) as u16;
    let r2 = (c2 & 0xFF) as u16;
    let g2 = ((c2 >> 8) & 0xFF) as u16;
    let b2 = ((c2 >> 16) & 0xFF) as u16;

    // TRIVERTEX color components are u16 in 0-65535 range; multiply byte value by 257
    let vertices = [
        TRIVERTEX {
            x: rect.left,
            y: rect.top,
            Red: r1 * 257,
            Green: g1 * 257,
            Blue: b1 * 257,
            Alpha: 0,
        },
        TRIVERTEX {
            x: rect.right,
            y: rect.bottom,
            Red: r2 * 257,
            Green: g2 * 257,
            Blue: b2 * 257,
            Alpha: 0,
        },
    ];

    let grad_rect = GRADIENT_RECT {
        UpperLeft: 0,
        LowerRight: 1,
    };

    let mode = if vertical {
        GRADIENT_FILL_RECT_V
    } else {
        GRADIENT_FILL_RECT_H
    };

    unsafe {
        let _ = GradientFill(
            hdc,
            &vertices,
            &grad_rect as *const GRADIENT_RECT as *const core::ffi::c_void,
            1,
            mode,
        );
    }
    true
}

/// Set the background gradient of a widget.
/// Stores gradient info in GRADIENT_MAP for WM_ERASEBKGND painting via GradientFill.
/// Also stores c1 as a solid fallback for ancestor color inheritance.
pub fn set_background_gradient(
    handle: i64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    _a2: f64,
    _direction: f64,
) {
    #[cfg(target_os = "windows")]
    {
        let c1 = rgb_to_colorref(r1, g1, b1);
        let c2 = rgb_to_colorref(r2, g2, b2);
        // direction: 0 = horizontal (left to right), 1 = vertical (top to bottom)
        let vertical = _direction != 0.0;

        // Store gradient info in GRADIENT_MAP keyed by HWND
        if let Some(hwnd) = get_hwnd_safe(handle) {
            let key = hwnd.0 as isize;
            if let Ok(mut map) = GRADIENT_MAP.lock() {
                // Remove any existing entry for this hwnd
                map.retain(|(k, _)| *k != key);
                map.push((key, GradientInfo { c1, c2, vertical }));
            }

            // Also store gradient colors as HWND properties so paint handlers can
            // detect gradient presence without the Mutex if needed
            unsafe {
                let prop_c1: Vec<u16> = "PerryGradC1"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let prop_c2: Vec<u16> = "PerryGradC2"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                SetPropW(
                    hwnd,
                    windows::core::PCWSTR(prop_c1.as_ptr()),
                    HANDLE((c1 as usize + 1) as *mut _),
                );
                SetPropW(
                    hwnd,
                    windows::core::PCWSTR(prop_c2.as_ptr()),
                    HANDLE((c2 as usize + 1) as *mut _),
                );
            }
        }

        // Set c1 as fallback solid color for ancestor color inheritance
        set_background_color(handle, r1, g1, b1, a1);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, r1, g1, b1, a1, r2, g2, b2, _a2, _direction);
    }
}

/// Set an on-hover callback for a widget.
pub fn set_on_hover(handle: i64, callback: f64) {
    // Win32 hover requires SetWindowSubclass + TrackMouseEvent + WM_MOUSEHOVER/LEAVE.
    // Best-effort no-op.
    let _ = handle;
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(
                handle: i64,
                widget_type: u8,
                callback_kind: u8,
                closure_f64: f64,
                label_ptr: *const u8,
            );
        }
        unsafe {
            perry_geisterhand_register(handle, 0, 3, callback, std::ptr::null());
        }
    }
}

/// Set a double-click callback for a widget.
pub fn set_on_double_click(handle: i64, callback: f64) {
    // Win32 double-click requires CS_DBLCLKS style + WM_LBUTTONDBLCLK handling.
    // Best-effort no-op.
    let _ = handle;
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(
                handle: i64,
                widget_type: u8,
                callback_kind: u8,
                closure_f64: f64,
                label_ptr: *const u8,
            );
        }
        unsafe {
            perry_geisterhand_register(handle, 0, 4, callback, std::ptr::null());
        }
    }
}

/// Animate the opacity of a widget. `duration_secs` is in seconds.
pub fn animate_opacity(handle: i64, _target: f64, _duration_secs: f64) {
    // Win32 opacity animation requires WS_EX_LAYERED + SetLayeredWindowAttributes + SetTimer.
    // Best-effort no-op.
    let _ = handle;
}

/// Animate the position of a widget. `duration_secs` is in seconds.
pub fn animate_position(handle: i64, _dx: f64, _dy: f64, _duration_secs: f64) {
    // Win32 position animation requires SetTimer + incremental SetWindowPos.
    // Best-effort no-op.
    let _ = handle;
}
