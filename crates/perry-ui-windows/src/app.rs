//! App lifecycle — window creation, message pump, keyboard shortcuts (Win32)

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::InitCommonControlsEx;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::ICC_BAR_CLASSES;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::ICC_STANDARD_CLASSES;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::INITCOMMONCONTROLSEX;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
// NOTE: SetProcessDpiAwarenessContext + GetDpiForSystem are deliberately NOT
// imported from `windows::Win32::UI::HiDpi` here. Hard-importing them produces
// link-time IAT entries that the OS resolves before main() runs. On Win7 those
// exports don't exist → the loader fails the process with "entry point not
// found in user32.dll" before any Rust code runs. For #303 we need the binary
// to start on Win7, so the Win10 DPI APIs are resolved lazily via
// LoadLibraryW + GetProcAddress in `crate::dpi_compat`. See
// `docs/src/platforms/windows-7.md`.

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
}

/// Timer ID for periodic tick that processes setTimeout/setInterval queues.
const TICK_TIMER_ID: usize = 9998;

/// Timer ID fired when `PERRY_UI_TEST_MODE=1` — writes a screenshot (if configured)
/// and exits cleanly so doc-example programs can be verified in CI without a human.
const TEST_EXIT_TIMER_ID: usize = 9997;

/// Global DPI scale factor (1.0 at 96 DPI, 1.5 at 144 DPI, 2.0 at 192 DPI).
static DPI_SCALE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Get the system DPI for the primary monitor.
///
/// Routes through `crate::dpi_compat::get_system_dpi_compat`, which uses
/// `GetDpiForSystem` (Win10 1607+) when available and falls back to
/// `GetDC + GetDeviceCaps` (Win2000+) on older systems including Win7. See
/// `dpi_compat.rs` for why this can't be a hard import (issue #303).
#[cfg(target_os = "windows")]
fn get_system_dpi() -> u32 {
    crate::dpi_compat::get_system_dpi_compat()
}

/// Store the DPI scale factor for use by font/widget sizing.
fn set_dpi_scale(scale: f64) {
    DPI_SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Get the DPI scale factor. Returns 1.0 if not set.
pub fn get_dpi_scale() -> f64 {
    let bits = DPI_SCALE.load(std::sync::atomic::Ordering::Relaxed);
    if bits == 0 {
        1.0
    } else {
        f64::from_bits(bits)
    }
}

thread_local! {
    static TIMER_TICK_NEEDED: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

/// Extract a &str from a *const StringHeader pointer.
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

pub(crate) struct AppEntry {
    #[cfg(target_os = "windows")]
    pub(crate) hwnd: HWND,
    #[cfg(not(target_os = "windows"))]
    pub(crate) hwnd: isize,
    root_widget: Option<i64>,
    min_size: Option<(f64, f64)>,
    max_size: Option<(f64, f64)>,
}

struct PendingShortcut {
    key: String,
    modifiers: f64,
    callback: f64,
}

struct ShortcutEntry {
    vk: u16,
    ctrl: bool,
    shift: bool,
    alt: bool,
    callback_ptr: *const u8,
}

struct TimerEntry {
    interval_ms: u32,
    callback_ptr: *const u8,
}

thread_local! {
    pub(crate) static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
    static PENDING_SHORTCUTS: RefCell<Vec<PendingShortcut>> = RefCell::new(Vec::new());
    static SHORTCUTS: RefCell<Vec<ShortcutEntry>> = RefCell::new(Vec::new());
    static TIMERS: RefCell<Vec<TimerEntry>> = RefCell::new(Vec::new());
    static ON_ACTIVATE_CALLBACK: RefCell<Option<*const u8>> = RefCell::new(None);
    static ON_TERMINATE_CALLBACK: RefCell<Option<*const u8>> = RefCell::new(None);
    static GLOBAL_HOTKEY_CALLBACKS: RefCell<HashMap<i32, *const u8>> = RefCell::new(HashMap::new());
    static NEXT_HOTKEY_ID: std::cell::Cell<i32> = std::cell::Cell::new(1);
}

/// Get the HWND of the first (main) app window.
#[cfg(target_os = "windows")]
pub fn get_main_hwnd() -> Option<HWND> {
    APPS.with(|apps| {
        let apps = apps.borrow();
        apps.first().map(|a| a.hwnd)
    })
}

#[cfg(not(target_os = "windows"))]
pub fn get_main_hwnd() -> Option<isize> {
    None
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Create an app window. Returns app handle (1-based).
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = str_from_header(title_ptr);
    let w = if width > 0.0 { width as i32 } else { 800 };
    let h = if height > 0.0 { height as i32 } else { 600 };

    #[cfg(target_os = "windows")]
    {
        unsafe {
            // Enable DPI awareness — Win10 1607+ best, Win8.1 ok, Win7 silent
            // fallback to system DPI. See dpi_compat.rs (issue #303).
            crate::dpi_compat::set_process_dpi_awareness_compat();

            // Scale window size by system DPI (96 = 100%, 144 = 150%, 192 = 200%)
            let dpi = get_system_dpi();
            let scale = dpi as f64 / 96.0;
            let w = (w as f64 * scale) as i32;
            let h = (h as f64 * scale) as i32;
            set_dpi_scale(scale);

            // Initialize common controls
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES | ICC_BAR_CLASSES,
            };
            let _ = InitCommonControlsEx(&icc);

            let hinstance = GetModuleHandleW(None).unwrap();

            // Register window class
            let class_name = to_wide("PerryMainWindow");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE::from(hinstance),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let title_wide = to_wide(title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                w,
                h,
                None,
                None,
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Attach any pending menu bar now that the window exists
            crate::menu::attach_pending_menubar(hwnd);

            APPS.with(|apps| {
                let mut apps = apps.borrow_mut();
                apps.push(AppEntry {
                    hwnd,
                    root_widget: None,
                    min_size: None,
                    max_size: None,
                });
                apps.len() as i64
            })
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, w, h);
        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            apps.push(AppEntry {
                hwnd: 0,
                root_widget: None,
                min_size: None,
                max_size: None,
            });
            apps.len() as i64
        })
    }
}

/// Set the root widget of an app.
pub fn app_set_body(app_handle: i64, root_handle: i64) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].root_widget = Some(root_handle);

            #[cfg(target_os = "windows")]
            {
                let hwnd = apps[idx].hwnd;
                // Set the root widget's HWND as a child of the main window
                if let Some(child_hwnd) = crate::widgets::get_hwnd(root_handle) {
                    unsafe {
                        let _ = SetParent(child_hwnd, hwnd);
                        let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                        SetWindowLongW(child_hwnd, GWL_STYLE, (style | WS_CHILD.0) as i32);
                        // Trigger initial layout
                        let mut rect = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rect);
                        let _ = MoveWindow(child_hwnd, 0, 0, rect.right, rect.bottom, true);
                        crate::layout::layout_widget(root_handle, rect.right, rect.bottom);
                    }
                }
            }
        }
    });
}

/// Run the app event loop (blocks until window closes).
pub fn app_run(app_handle: i64) {
    // Phase 2 v3.3: register cross-platform showToast / setText handlers so
    // `perry_arkts_show_toast` and `perry_arkts_set_text` (defined in
    // perry-runtime/src/ui_text_registry.rs) forward to our Win32
    // borderless-popup toast presenter and SetWindowTextW text updater.
    register_cross_platform_text_handlers();

    // Install pending keyboard shortcuts
    PENDING_SHORTCUTS.with(|pending| {
        let shortcuts: Vec<PendingShortcut> = pending.borrow_mut().drain(..).collect();
        for s in shortcuts {
            install_shortcut(&s.key, s.modifiers, s.callback);
        }
    });

    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                unsafe {
                    let _ = ShowWindow(apps[idx].hwnd, SW_SHOW);
                    let _ = UpdateWindow(apps[idx].hwnd);
                }
            }
        });

        // Start a periodic timer to tick setTimeout/setInterval queues
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                unsafe {
                    let _ = SetTimer(apps[idx].hwnd, TICK_TIMER_ID, 50, None);
                }
            }
        });

        // PERRY_UI_TEST_MODE: schedule a one-shot exit timer so CI can verify
        // that the app launched without a human keeping it open.
        if perry_ui_testkit::is_test_mode() {
            APPS.with(|apps| {
                let apps = apps.borrow();
                let idx = (app_handle - 1) as usize;
                if idx < apps.len() {
                    unsafe {
                        let _ = SetTimer(
                            apps[idx].hwnd,
                            TEST_EXIT_TIMER_ID,
                            perry_ui_testkit::exit_delay_ms(),
                            None,
                        );
                    }
                }
            });
        }

        // Register UI function pointers for geisterhand dispatch
        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register_state_set(f: extern "C" fn(i64, f64));
                fn perry_geisterhand_register_screenshot_capture(
                    f: extern "C" fn(*mut usize) -> *mut u8,
                );
                fn perry_geisterhand_register_textfield_set_string(f: extern "C" fn(i64, i64));
                fn perry_geisterhand_register_apply_style(
                    f: extern "C" fn(i64, u32, f64, f64, f64, f64),
                );
            }
            unsafe {
                perry_geisterhand_register_state_set(crate::perry_ui_state_set);
                perry_geisterhand_register_screenshot_capture(
                    crate::screenshot::perry_ui_screenshot_capture,
                );
                perry_geisterhand_register_textfield_set_string(
                    crate::perry_ui_textfield_set_string,
                );
                perry_geisterhand_register_apply_style(crate::geisterhand_style::apply_style);
            }
        }

        // Message loop
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                // WM_HOTKEY is posted to the thread message queue, not to a window
                if msg.message == 0x0312 {
                    // WM_HOTKEY
                    let hotkey_id = msg.wParam.0 as i32;
                    GLOBAL_HOTKEY_CALLBACKS.with(|cbs| {
                        if let Some(cb_ptr) = cbs.borrow().get(&hotkey_id) {
                            unsafe {
                                js_closure_call0(*cb_ptr);
                            }
                        }
                    });
                    continue;
                }
                // Check keyboard shortcuts
                if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
                    if try_handle_shortcut(msg.wParam.0 as u16) {
                        continue;
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                // Process setTimeout/setInterval callbacks outside wndproc to avoid re-entrancy
                if TIMER_TICK_NEEDED.with(|t| t.replace(false)) {
                    unsafe {
                        js_callback_timer_tick();
                        js_interval_timer_tick();
                    }
                }
                // perry/media (#351) — UI-thread state poll for active
                // MediaPlayer instances. Internally throttled to ~10 Hz.
                crate::media_playback::pump_tick();
                #[cfg(feature = "geisterhand")]
                {
                    extern "C" {
                        fn perry_geisterhand_pump();
                    }
                    unsafe {
                        perry_geisterhand_pump();
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app_handle;
    }
}

fn install_shortcut(key: &str, modifiers: f64, callback: f64) {
    let mods = modifiers as u32;
    // Perry modifier flags: 1=Cmd(->Ctrl on Win), 2=Shift, 4=Option(->Alt), 8=Control(->Ctrl)
    let ctrl = (mods & 1) != 0 || (mods & 8) != 0;
    let shift = (mods & 2) != 0;
    let alt = (mods & 4) != 0;

    let vk = key_to_vk(key);
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    SHORTCUTS.with(|s| {
        s.borrow_mut().push(ShortcutEntry {
            vk,
            ctrl,
            shift,
            alt,
            callback_ptr,
        });
    });
}

fn key_to_vk(key: &str) -> u16 {
    match key.to_lowercase().as_str() {
        "a" => 0x41,
        "b" => 0x42,
        "c" => 0x43,
        "d" => 0x44,
        "e" => 0x45,
        "f" => 0x46,
        "g" => 0x47,
        "h" => 0x48,
        "i" => 0x49,
        "j" => 0x4A,
        "k" => 0x4B,
        "l" => 0x4C,
        "m" => 0x4D,
        "n" => 0x4E,
        "o" => 0x4F,
        "p" => 0x50,
        "q" => 0x51,
        "r" => 0x52,
        "s" => 0x53,
        "t" => 0x54,
        "u" => 0x55,
        "v" => 0x56,
        "w" => 0x57,
        "x" => 0x58,
        "y" => 0x59,
        "z" => 0x5A,
        "0" => 0x30,
        "1" => 0x31,
        "2" => 0x32,
        "3" => 0x33,
        "4" => 0x34,
        "5" => 0x35,
        "6" => 0x36,
        "7" => 0x37,
        "8" => 0x38,
        "9" => 0x39,
        "return" | "enter" => 0x0D,
        "escape" | "esc" => 0x1B,
        "tab" => 0x09,
        "space" => 0x20,
        "delete" | "backspace" => 0x08,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        _ => 0,
    }
}

fn try_handle_shortcut(vk: u16) -> bool {
    #[cfg(target_os = "windows")]
    {
        let ctrl_down = unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11 /* VK_CONTROL */)
        } < 0;
        let shift_down = unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10 /* VK_SHIFT */)
        } < 0;
        let alt_down = unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x12 /* VK_MENU */)
        } < 0;

        SHORTCUTS.with(|shortcuts| {
            let shortcuts = shortcuts.borrow();
            for s in shortcuts.iter() {
                if s.vk == vk && s.ctrl == ctrl_down && s.shift == shift_down && s.alt == alt_down {
                    unsafe { js_closure_call0(s.callback_ptr) };
                    return true;
                }
            }
            false
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = vk;
        false
    }
}

/// Add a keyboard shortcut (buffered until app_run).
pub fn add_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64) {
    let key = str_from_header(key_ptr).to_string();
    PENDING_SHORTCUTS.with(|pending| {
        pending.borrow_mut().push(PendingShortcut {
            key,
            modifiers,
            callback,
        });
    });
}

/// Set minimum window size.
pub fn set_min_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].min_size = Some((w, h));
        }
    });
}

/// Set maximum window size.
pub fn set_max_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].max_size = Some((w, h));
        }
    });
}

/// Resize the main app window dynamically.
pub fn app_set_size(app_handle: i64, width: f64, height: f64) {
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                let hwnd = apps[idx].hwnd;
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        width as i32,
                        height as i32,
                        SWP_NOMOVE | SWP_NOZORDER,
                    );
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, width, height);
    }
}

/// Set frameless window mode (no titlebar).
/// `value` is a NaN-boxed boolean — TAG_TRUE = 0x7FFC_0000_0000_0004.
pub fn app_set_frameless(app_handle: i64, value: f64) {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    if value.to_bits() != TAG_TRUE {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                let hwnd = apps[idx].hwnd;
                unsafe {
                    // Change from WS_OVERLAPPEDWINDOW to WS_POPUP for a borderless window
                    SetWindowLongW(
                        hwnd,
                        GWL_STYLE,
                        (WS_POPUP.0 | WS_CLIPCHILDREN.0 | WS_VISIBLE.0) as i32,
                    );
                    // Force redraw after style change
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
                    );

                    // Request rounded corners on Windows 11+ via DWM
                    // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2
                    extern "system" {
                        fn DwmSetWindowAttribute(
                            hwnd: isize,
                            attr: u32,
                            value: *const i32,
                            size: u32,
                        ) -> i32;
                    }
                    let corner_pref: i32 = 2; // DWMWCP_ROUND
                    let _ = DwmSetWindowAttribute(
                        hwnd.0 as isize,
                        33, // DWMWA_WINDOW_CORNER_PREFERENCE
                        &corner_pref,
                        std::mem::size_of::<i32>() as u32,
                    );
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, value);
    }
}

/// Set window level: "floating", "statusBar", "modal", or "normal".
pub fn app_set_level(app_handle: i64, value_ptr: *const u8) {
    let level_str = str_from_header(value_ptr);
    if level_str.is_empty() {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                let hwnd = apps[idx].hwnd;
                unsafe {
                    let insert_after = match level_str {
                        "floating" | "statusBar" => HWND_TOPMOST,
                        "modal" => HWND_TOPMOST,
                        _ => HWND_NOTOPMOST,
                    };
                    let _ = SetWindowPos(hwnd, insert_after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, value_ptr);
    }
}

/// Set window transparency (transparent background).
/// `value` is a NaN-boxed boolean.
pub fn app_set_transparent(app_handle: i64, value: f64) {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    if value.to_bits() != TAG_TRUE {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                let hwnd = apps[idx].hwnd;
                unsafe {
                    // Add WS_EX_LAYERED for per-pixel alpha
                    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                    SetWindowLongW(hwnd, GWL_EXSTYLE, (ex_style | WS_EX_LAYERED.0) as i32);
                    // SetLayeredWindowAttributes for basic transparency
                    // 230 = ~90% opacity as a reasonable default
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), 230, LWA_ALPHA);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, value);
    }
}

/// Set vibrancy/backdrop material.
/// On Windows 11+: uses DwmSetWindowAttribute with DWMWA_SYSTEMBACKDROP_TYPE.
pub fn app_set_vibrancy(app_handle: i64, value_ptr: *const u8) {
    let material_str = str_from_header(value_ptr);
    if material_str.is_empty() {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                let hwnd = apps[idx].hwnd;
                unsafe {
                    // DWMWA_SYSTEMBACKDROP_TYPE = 38 (Windows 11 22H2+)
                    // Values: 0=Auto, 1=None, 2=Mica, 3=Acrylic, 4=MicaAlt
                    let backdrop_type: i32 = match material_str {
                        "sidebar" | "underWindowBackground" | "behindWindow" => 2, // Mica
                        "menu" | "popover" | "tooltip" | "hudWindow" => 3,         // Acrylic
                        "titlebar" | "headerView" => 4,                            // Mica Alt
                        _ => 3, // Acrylic default
                    };

                    // DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop, sizeof)
                    extern "system" {
                        fn DwmSetWindowAttribute(
                            hwnd: isize,
                            attr: u32,
                            value: *const i32,
                            size: u32,
                        ) -> i32;
                    }
                    let _ = DwmSetWindowAttribute(
                        hwnd.0 as isize,
                        38, // DWMWA_SYSTEMBACKDROP_TYPE
                        &backdrop_type,
                        std::mem::size_of::<i32>() as u32,
                    );

                    // Also enable dark mode title bar for better blending
                    let use_dark: i32 = 1;
                    let _ = DwmSetWindowAttribute(
                        hwnd.0 as isize,
                        20, // DWMWA_USE_IMMERSIVE_DARK_MODE
                        &use_dark,
                        std::mem::size_of::<i32>() as u32,
                    );

                    // Extend client area into frame for borderless mica/acrylic
                    extern "system" {
                        fn DwmExtendFrameIntoClientArea(
                            hwnd: isize,
                            margins: *const [i32; 4],
                        ) -> i32;
                    }
                    let margins: [i32; 4] = [-1, -1, -1, -1]; // MARGINS { -1, -1, -1, -1 }
                    let _ = DwmExtendFrameIntoClientArea(hwnd.0 as isize, &margins);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, value_ptr);
    }
}

/// Set activation policy: "regular", "accessory", or "background".
/// On Windows: "accessory" uses WS_EX_TOOLWINDOW (no taskbar entry).
pub fn app_set_activation_policy(app_handle: i64, value_ptr: *const u8) {
    let policy_str = str_from_header(value_ptr);
    if policy_str.is_empty() {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        if policy_str == "accessory" || policy_str == "background" {
            APPS.with(|apps| {
                let apps = apps.borrow();
                let idx = (app_handle - 1) as usize;
                if idx < apps.len() {
                    let hwnd = apps[idx].hwnd;
                    unsafe {
                        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                        // Remove WS_EX_APPWINDOW, add WS_EX_TOOLWINDOW to hide from taskbar
                        let new_style = (ex_style & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0;
                        SetWindowLongW(hwnd, GWL_EXSTYLE, new_style as i32);
                    }
                }
            });
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app_handle, value_ptr);
    }
}

/// Set a repeating timer.
pub fn set_timer(interval_ms: f64, callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    let ms = interval_ms as u32;

    TIMERS.with(|t| {
        let mut timers = t.borrow_mut();
        let timer_id = timers.len() + 1;
        timers.push(TimerEntry {
            interval_ms: ms,
            callback_ptr,
        });

        #[cfg(target_os = "windows")]
        {
            APPS.with(|apps| {
                let apps = apps.borrow();
                if let Some(app) = apps.first() {
                    unsafe {
                        let _ = SetTimer(app.hwnd, timer_id, ms, None);
                    }
                }
            });
        }

        let _ = timer_id;
    });
}

/// Register callback for app activation (WM_ACTIVATEAPP).
pub fn on_activate(callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    ON_ACTIVATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback_ptr);
    });
}

/// Register callback for app termination (WM_CLOSE/WM_DESTROY).
pub fn on_terminate(callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    ON_TERMINATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback_ptr);
    });
}

/// Handle WM_TIMER — dispatch to registered timer callbacks.
#[cfg(target_os = "windows")]
pub fn handle_timer(hwnd: HWND, timer_id: usize) {
    if timer_id == 0 {
        return;
    }

    // Periodic tick — just flag it, actual processing happens in message loop
    if timer_id == TICK_TIMER_ID {
        TIMER_TICK_NEEDED.with(|t| t.set(true));
        return;
    }

    // PERRY_UI_TEST_MODE exit: capture screenshot (if configured), then exit.
    if timer_id == TEST_EXIT_TIMER_ID {
        unsafe {
            let _ = KillTimer(hwnd, TEST_EXIT_TIMER_ID);
        }
        if let Some(path) = perry_ui_testkit::screenshot_path() {
            let mut len: usize = 0;
            let ptr = crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
            if !ptr.is_null() && len > 0 {
                perry_ui_testkit::write_screenshot_bytes(&path, ptr, len);
                unsafe {
                    libc::free(ptr as *mut libc::c_void);
                }
            }
        }
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        std::process::exit(0);
    }

    // Recurring timers (setInterval via Win32 SetTimer / perry_ui_app_set_timer)
    TIMERS.with(|t| {
        let timers = t.borrow();
        let idx = timer_id - 1;
        if idx < timers.len() {
            unsafe { js_closure_call0(timers[idx].callback_ptr) };
        }
    });
}

/// Handle WM_ACTIVATEAPP — call on_activate callback.
#[cfg(target_os = "windows")]
pub fn handle_activate(activating: bool) {
    if activating {
        ON_ACTIVATE_CALLBACK.with(|c| {
            if let Some(ptr) = *c.borrow() {
                unsafe { js_closure_call0(ptr) };
            }
        });
    }
}

/// Handle WM_DESTROY — call on_terminate callback before quit.
#[cfg(target_os = "windows")]
pub fn handle_terminate() {
    ON_TERMINATE_CALLBACK.with(|c| {
        if let Some(ptr) = *c.borrow() {
            unsafe { js_closure_call0(ptr) };
        }
    });
}

/// Get app min/max sizes for WM_GETMINMAXINFO.
pub fn get_size_constraints(app_handle: i64) -> (Option<(f64, f64)>, Option<(f64, f64)>) {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            (apps[idx].min_size, apps[idx].max_size)
        } else {
            (None, None)
        }
    })
}

/// Get root widget handle for an app.
pub fn get_root_widget(app_handle: i64) -> Option<i64> {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].root_widget
        } else {
            None
        }
    })
}

/// Custom message ID for deferred layout requests.
#[cfg(target_os = "windows")]
const WM_PERRY_LAYOUT: u32 = WM_USER + 100;

/// Request a deferred layout pass on the main window's root widget.
/// Uses PostMessage so multiple rapid calls (e.g., during tree rebuild)
/// are coalesced into a single layout pass in the message loop.
pub fn request_layout() {
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            if let Some(app) = apps.first() {
                if app.root_widget.is_some() {
                    unsafe {
                        let _ = PostMessageW(app.hwnd, WM_PERRY_LAYOUT, WPARAM(0), LPARAM(0));
                    }
                }
            }
        });
    }
}

/// Perform an immediate layout pass on the main window's root widget.
fn do_layout() {
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            if let Some(app) = apps.first() {
                if let Some(root) = app.root_widget {
                    unsafe {
                        let mut rect = RECT::default();
                        let _ = GetClientRect(app.hwnd, &mut rect);
                        if rect.right > 0 && rect.bottom > 0 {
                            if let Some(child_hwnd) = crate::widgets::get_hwnd(root) {
                                let _ = MoveWindow(child_hwnd, 0, 0, rect.right, rect.bottom, true);
                                crate::layout::layout_widget(root, rect.right, rect.bottom);
                            }
                            let _ = InvalidateRect(app.hwnd, None, true);
                        }
                    }
                }
            }
        });
    }
}

// =============================================================================
// WndProc — Win32 message handler
// =============================================================================

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            let width = (lparam.0 & 0xFFFF) as i32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as i32;
            // Resize root widget to fill window
            if let Some(root) = get_root_widget(1) {
                if let Some(child_hwnd) = crate::widgets::get_hwnd(root) {
                    let _ = MoveWindow(child_hwnd, 0, 0, width, height, true);
                    crate::layout::layout_widget(root, width, height);
                    // Set a one-shot timer to force repaint
                    let _ = SetTimer(hwnd, 9999, 500, None);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            // Main window has no content to paint — just validate the region
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            let (min, max) = get_size_constraints(1);
            if let Some((w, h)) = min {
                info.ptMinTrackSize = POINT { x: w as i32, y: h as i32 };
            }
            if let Some((w, h)) = max {
                info.ptMaxTrackSize = POINT { x: w as i32, y: h as i32 };
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // Forward mouse wheel to the window under the cursor (not just the focused window).
            // This ensures embedded views (like hone-editor) and ScrollViews receive scroll events.
            let cursor_pos = POINT {
                x: (lparam.0 & 0xFFFF) as i16 as i32,
                y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            };
            let target = WindowFromPoint(cursor_pos);
            if !target.is_invalid() && target != hwnd {
                return SendMessageW(target, msg, wparam, lparam);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_COMMAND => {
            let control_id = (wparam.0 & 0xFFFF) as u16;
            let notify_code = ((wparam.0 >> 16) & 0xFFFF) as u16;
            if notify_code == 0 && lparam.0 == 0 {
                // Menu bar command — lparam==0 distinguishes from control notifications
                crate::menu::dispatch_menu_item(control_id);
            } else {
                crate::widgets::handle_command(control_id, notify_code, lparam);
            }
            LRESULT(0)
        }
        x if x == WM_HSCROLL || x == WM_VSCROLL => {
            crate::widgets::handle_scroll(wparam, lparam);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            let child_hwnd = HWND(lparam.0 as *mut _);
            if let Some(result) = crate::widgets::text::handle_ctlcolor(hdc, child_hwnd) {
                return result;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        x if x == 0x0133 /* WM_CTLCOLOREDIT */ => {
            let hdc = HDC(wparam.0 as *mut _);
            let child_hwnd = HWND(lparam.0 as *mut _);
            if let Some(brush) = crate::widgets::textfield::handle_ctlcoloredit(hdc, child_hwnd) {
                return LRESULT(brush);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CTLCOLORBTN => {
            // Make buttons use the nearest ancestor's background color
            let hdc = HDC(wparam.0 as *mut _);
            let mut walk = HWND(lparam.0 as *mut _);
            for _ in 0..10 {
                if let Ok(parent_hwnd) = GetParent(walk) {
                    if parent_hwnd.0.is_null() { break; }
                    let parent_handle = crate::widgets::find_handle_by_hwnd(parent_hwnd);
                    if parent_handle > 0 {
                        if let (Some(color), Some(brush)) = (
                            crate::widgets::get_bg_color(parent_handle),
                            crate::widgets::get_bg_brush(parent_handle),
                        ) {
                            SetBkColor(hdc, COLORREF(color));
                            SetBkMode(hdc, TRANSPARENT);
                            return LRESULT(brush.0 as isize);
                        }
                    }
                    walk = parent_hwnd;
                } else {
                    break;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DRAWITEM => {
            if crate::widgets::button::handle_draw_item(lparam) {
                return LRESULT(1); // TRUE = handled
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CONTEXTMENU => {
            let child_hwnd = HWND(wparam.0 as *mut _);
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            crate::menu::handle_context_menu(hwnd, child_hwnd, x, y);
            LRESULT(0)
        }
        WM_TIMER => {
            let timer_id = wparam.0;
            if timer_id == 9999 {
                let _ = KillTimer(hwnd, 9999);
                if let Some(root) = get_root_widget(1) {
                    crate::layout::force_paint_backgrounds(root);
                }
            } else {
                crate::app::handle_timer(hwnd, timer_id);
            }
            LRESULT(0)
        }
        WM_ACTIVATEAPP => {
            let activating = wparam.0 != 0;
            crate::app::handle_activate(activating);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND => {
            // Paint the root widget's background if set
            if let Some(root) = get_root_widget(1) {
                if let Some(brush) = crate::widgets::get_bg_brush(root) {
                    let hdc = HDC(wparam.0 as *mut _);
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    FillRect(hdc, &rect, brush);
                    return LRESULT(1);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            crate::app::handle_terminate();
            PostQuitMessage(0);
            LRESULT(0)
        }
        x if x == WM_PERRY_LAYOUT => {
            do_layout();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Register a system-wide global hotkey. Uses Win32 `RegisterHotKey` API.
/// `key_ptr` is a StringHeader pointer to the key (e.g., "s").
/// `modifiers` is a bitfield: 1=Cmd(->Ctrl), 2=Shift, 4=Option(->Alt), 8=Control(->Ctrl).
/// `callback` is a NaN-boxed closure pointer.
pub fn register_global_hotkey(key_ptr: *const u8, modifiers: f64, callback: f64) {
    let key_str = str_from_header(key_ptr);
    if key_str.is_empty() {
        return;
    }

    let mod_bits = modifiers as u32;
    // Perry: 1=Cmd(->Ctrl on Win), 2=Shift, 4=Option(->Alt), 8=Control(->Ctrl)
    let mut win_mods: u32 = 0;
    if mod_bits & 1 != 0 || mod_bits & 8 != 0 {
        win_mods |= 0x0002;
    } // MOD_CONTROL
    if mod_bits & 2 != 0 {
        win_mods |= 0x0004;
    } // MOD_SHIFT
    if mod_bits & 4 != 0 {
        win_mods |= 0x0001;
    } // MOD_ALT
    win_mods |= 0x4000; // MOD_NOREPEAT

    let vk = key_to_vk(key_str);
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    let id = NEXT_HOTKEY_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    });

    GLOBAL_HOTKEY_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(id, callback_ptr);
    });

    #[cfg(target_os = "windows")]
    {
        unsafe {
            extern "system" {
                fn RegisterHotKey(hwnd: isize, id: i32, modifiers: u32, vk: u32) -> i32;
            }
            RegisterHotKey(0, id, win_mods, vk as u32);
        }
    }
}

/// Get the icon for an application at the given path.
/// Returns 0 (stub) — full implementation requires GDI bitmap conversion.
pub fn get_app_icon(_path_ptr: *const u8) -> i64 {
    // TODO: Full implementation with SHGetFileInfo + HICON -> bitmap conversion
    0
}

// ============================================================================
// Phase 2 v3.3: cross-platform showToast / setText wiring.
// ============================================================================

extern "C" {
    /// Defined in `perry-runtime/src/ui_text_registry.rs`. Stores the passed
    /// handler in an AtomicPtr consulted by `perry_arkts_show_toast` on each
    /// call. No-op when `ohos-napi` feature is active.
    fn js_register_show_toast_handler(f: extern "C" fn(msg_ptr: *const u8, msg_len: usize));
    fn js_register_set_text_handler(
        f: extern "C" fn(id_ptr: *const u8, id_len: usize, val_ptr: *const u8, val_len: usize),
    );
    fn js_register_text_id_handler(
        f: extern "C" fn(widget_handle: i64, id_ptr: *const u8, id_len: usize),
    );
}

fn register_cross_platform_text_handlers() {
    unsafe {
        js_register_show_toast_handler(crate::widgets::toast::show_toast_handler);
        js_register_set_text_handler(crate::widgets::text_registry::set_text_handler);
        js_register_text_id_handler(crate::widgets::text_registry::register_text_id_handler);
    }
}
