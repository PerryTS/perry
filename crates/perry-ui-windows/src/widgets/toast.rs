//! Windows toast presenter for `showToast(msg)` (Phase 2 v3.3).
//!
//! Renders a HUD-style banner near the bottom-center of the main app window
//! for ~2.5s using a borderless `WS_EX_LAYERED` popup with a dark background
//! and white centered text. Alpha fades 0→255 over 250ms, holds for 2s, then
//! fades 255→0 over 250ms before the window is destroyed. Multiple toasts
//! queued in quick succession render back-to-back — PRESENTING gate prevents
//! "b" from overwriting "a".
//!
//! ## Wiring
//!
//! `app::register_cross_platform_text_handlers` calls
//! `js_register_show_toast_handler` (defined in
//! `perry-runtime/src/ui_text_registry.rs`) at `app_run` startup, passing
//! `show_toast_handler` here as the registered fn pointer. When user TS code
//! later calls `showToast("Saved!")`, the codegen emits a call to
//! `perry_arkts_show_toast`; the runtime decodes the NaN-boxed string and
//! forwards to this handler on the main thread.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
    EndPaint, FillRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, UpdateWindow,
    DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HBRUSH, HGDIOBJ, PAINTSTRUCT, TRANSPARENT,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

const TOAST_WIDTH: i32 = 320;
const TOAST_HEIGHT: i32 = 56;
const TOAST_BOTTOM_MARGIN: i32 = 48;
/// How many 50ms timer ticks to spend fading in or out.
const TOAST_FADE_TICKS: u32 = 5; // 5 × 50ms = 250ms
/// How many 50ms timer ticks to hold at full opacity before fading out.
const TOAST_HOLD_TICKS: u32 = 40; // 40 × 50ms = 2000ms
const TIMER_ID: usize = 6001;

/// Dark background colour (0x00BBGGRR COLORREF — no alpha, handled by LWA_ALPHA).
const BG_COLOR: u32 = 0x00_22_22_22;
/// Corner radius for the rounded clip region.
const CORNER_RADIUS: i32 = 14;

enum ToastPhase {
    FadeIn { ticks: u32 },
    Hold { ticks: u32 },
    FadeOut { ticks: u32 },
}

struct ToastState {
    /// HWND stored as isize so it can live in thread_local! on non-Windows.
    hwnd: isize,
    phase: ToastPhase,
    msg: String,
}

thread_local! {
    static TOAST_QUEUE: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
    static PRESENTING: Cell<bool> = const { Cell::new(false) };
    static ACTIVE_TOAST: RefCell<Option<ToastState>> = const { RefCell::new(None) };
}

/// Cross-platform handler entry point. Registered with
/// `js_register_show_toast_handler` at app startup.
pub extern "C" fn show_toast_handler(msg_ptr: *const u8, msg_len: usize) {
    if msg_ptr.is_null() {
        return;
    }
    let msg = unsafe {
        let bytes = std::slice::from_raw_parts(msg_ptr, msg_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    TOAST_QUEUE.with(|q| q.borrow_mut().push_back(msg));
    drain_if_idle();
}

/// If no toast is currently active, pop the next queued message and show it.
fn drain_if_idle() {
    if PRESENTING.with(|p| p.get()) {
        return;
    }
    let next = TOAST_QUEUE.with(|q| q.borrow_mut().pop_front());
    let Some(msg) = next else { return };
    PRESENTING.with(|p| p.set(true));
    present_toast(msg);
}

fn present_toast(msg: String) {
    #[cfg(target_os = "windows")]
    {
        present_toast_win32(msg);
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Non-Windows host: no UI to show; just reset so the queue can advance.
        PRESENTING.with(|p| p.set(false));
        let _ = msg;
        drain_if_idle();
    }
}

// ===========================================================================
// Win32 implementation
// ===========================================================================

#[cfg(target_os = "windows")]
static CLASS_INIT: std::sync::Once = std::sync::Once::new();

#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    CLASS_INIT.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryToastClass");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(toast_wnd_proc),
            hInstance: HINSTANCE::from(hinstance),
            // NULL background brush — we fill in WM_ERASEBKGND / WM_PAINT.
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // RegisterClassExW returns 0 on failure (e.g. already registered) — ignore.
        RegisterClassExW(&wc);
    });
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn alpha_for_tick(ticks_done: u32, total_ticks: u32) -> u8 {
    ((ticks_done as u64 * 255) / total_ticks.max(1) as u64).min(255) as u8
}

#[cfg(target_os = "windows")]
fn present_toast_win32(msg: String) {
    unsafe {
        ensure_class_registered();

        // Position: bottom-center of the main app window, or near the bottom
        // of the primary monitor if no app window exists yet.
        let (x, y) = if let Some(main_hwnd) = crate::app::get_main_hwnd() {
            let mut rect = RECT::default();
            let _ = GetWindowRect(main_hwnd, &mut rect);
            let cx = rect.left + (rect.right - rect.left - TOAST_WIDTH) / 2;
            let cy = rect.bottom - TOAST_HEIGHT - TOAST_BOTTOM_MARGIN;
            (cx, cy)
        } else {
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            (
                (screen_w - TOAST_WIDTH) / 2,
                screen_h - TOAST_HEIGHT - TOAST_BOTTOM_MARGIN,
            )
        };

        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryToastClass");

        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            windows::core::PCWSTR(class_name.as_ptr()),
            windows::core::PCWSTR(std::ptr::null()),
            WS_POPUP,
            x,
            y,
            TOAST_WIDTH,
            TOAST_HEIGHT,
            None,
            None,
            HINSTANCE::from(hinstance),
            None,
        ) {
            Ok(h) => h,
            Err(_) => {
                PRESENTING.with(|p| p.set(false));
                drain_if_idle();
                return;
            }
        };

        // Clip the window to a rounded rectangle.
        let rgn = CreateRoundRectRgn(
            0,
            0,
            TOAST_WIDTH,
            TOAST_HEIGHT,
            CORNER_RADIUS,
            CORNER_RADIUS,
        );
        if !rgn.is_invalid() {
            SetWindowRgn(hwnd, rgn, false);
            // SetWindowRgn takes ownership of the region — don't DeleteObject(rgn).
        }

        // Start fully transparent; the tick timer fades us in.
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA).ok();

        // Store state before ShowWindow so WM_PAINT has a message to draw.
        ACTIVE_TOAST.with(|s| {
            *s.borrow_mut() = Some(ToastState {
                hwnd: hwnd.0 as isize,
                phase: ToastPhase::FadeIn { ticks: 0 },
                msg,
            });
        });

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(hwnd);

        // 50ms tick timer drives the fade-in / hold / fade-out animation.
        let _ = SetTimer(hwnd, TIMER_ID, 50, None);
    }
}

/// Advance the animation state by one tick. Called from `WM_TIMER` in the
/// toast's own WndProc (same thread as the message loop, so thread-local
/// access is safe).
#[cfg(target_os = "windows")]
fn tick_animation(hwnd: HWND) {
    let done = ACTIVE_TOAST.with(|state| {
        let mut guard = state.borrow_mut();
        let Some(ref mut s) = *guard else {
            return false;
        };

        match s.phase {
            ToastPhase::FadeIn { ref mut ticks } => {
                *ticks += 1;
                let alpha = alpha_for_tick(*ticks, TOAST_FADE_TICKS);
                unsafe {
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA).ok();
                }
                if *ticks >= TOAST_FADE_TICKS {
                    s.phase = ToastPhase::Hold { ticks: 0 };
                }
                false
            }
            ToastPhase::Hold { ref mut ticks } => {
                *ticks += 1;
                if *ticks >= TOAST_HOLD_TICKS {
                    s.phase = ToastPhase::FadeOut { ticks: 0 };
                }
                false
            }
            ToastPhase::FadeOut { ref mut ticks } => {
                *ticks += 1;
                let remaining = TOAST_FADE_TICKS.saturating_sub(*ticks);
                let alpha = alpha_for_tick(remaining, TOAST_FADE_TICKS);
                unsafe {
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA).ok();
                }
                *ticks >= TOAST_FADE_TICKS
            }
        }
    });

    if done {
        ACTIVE_TOAST.with(|s| *s.borrow_mut() = None);
        unsafe {
            let _ = KillTimer(hwnd, TIMER_ID);
            let _ = DestroyWindow(hwnd);
        }
        PRESENTING.with(|p| p.set(false));
        drain_if_idle();
    }
}

/// Draw the dark background and centered white message text.
#[cfg(target_os = "windows")]
unsafe fn paint_toast(hwnd: HWND) {
    let msg = ACTIVE_TOAST.with(|s| {
        s.borrow()
            .as_ref()
            .map(|t| t.msg.clone())
            .unwrap_or_default()
    });

    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    // Fill the dark background.
    let bg_brush = CreateSolidBrush(COLORREF(BG_COLOR));
    let mut client_rect = RECT::default();
    GetClientRect(hwnd, &mut client_rect).ok();
    FillRect(hdc, &client_rect, bg_brush);
    let _ = DeleteObject(bg_brush);

    // Draw white text, centered.
    let mut wide: Vec<u16> = msg.encode_utf16().collect();
    if !wide.is_empty() {
        SetTextColor(hdc, COLORREF(0x00_FF_FF_FF));
        SetBkMode(hdc, TRANSPARENT);

        let font_family = to_wide("Segoe UI");
        let scaled_size = (14.0 * crate::app::get_dpi_scale()) as i32;
        let hfont = CreateFontW(
            -scaled_size,
            0,
            0,
            0,
            400, // FW_NORMAL
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            windows::core::PCWSTR(font_family.as_ptr()),
        );
        let old_font: HGDIOBJ = if !hfont.is_invalid() {
            SelectObject(hdc, hfont)
        } else {
            HGDIOBJ::default()
        };

        let padding = (12.0 * crate::app::get_dpi_scale()) as i32;
        let mut text_rect = RECT {
            left: padding,
            top: 0,
            right: client_rect.right - padding,
            bottom: client_rect.bottom,
        };
        DrawTextW(
            hdc,
            &mut wide,
            &mut text_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        if !old_font.is_invalid() {
            SelectObject(hdc, old_font);
        }
        if !hfont.is_invalid() {
            let _ = DeleteObject(hfont);
        }
    }

    EndPaint(hwnd, &ps);
}

/// WndProc for `"PerryToastClass"` windows.
#[cfg(target_os = "windows")]
unsafe extern "system" fn toast_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        x if x == WM_PAINT => {
            paint_toast(hwnd);
            LRESULT(0)
        }
        x if x == WM_ERASEBKGND => {
            // Handled in WM_PAINT — tell Windows we erased.
            LRESULT(1)
        }
        x if x == WM_TIMER => {
            if wparam.0 == TIMER_ID {
                tick_animation(hwnd);
            }
            LRESULT(0)
        }
        x if x == WM_DESTROY => {
            // Kill the timer defensively in case DestroyWindow is called
            // externally (e.g. app shutdown before the fade completes).
            let _ = KillTimer(hwnd, TIMER_ID);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
