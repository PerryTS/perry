//! BloomView — a native render-surface host widget (issue #2395).
//!
//! BloomView reserves a child window inside the Perry UI view tree for an
//! external GPU renderer (the Bloom game engine) to draw into. Perry UI does
//! NOT link or know about Bloom: the widget only owns the HWND and exposes it
//! via `bloomViewGetHwnd`. User TypeScript then hands that HWND to the Bloom
//! package (`attachToHwnd`), which builds its wgpu surface on it and subclasses
//! it for resize/input. This keeps `perry-ui-windows` free of any Bloom
//! dependency — apps that never call `BloomView` pull in nothing extra.
//!
//! Like WebView, BloomView reuses `WidgetKind::Image` for its registry slot —
//! it's a leaf widget the layout engine sizes; there is no kind-specific
//! dispatch.

#[cfg(target_os = "windows")]
use super::{register_widget_with_layout, set_fixed_height, set_fixed_width, WidgetKind};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::HBRUSH;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Default window proc for the host window. Bloom classic-subclasses this once
/// attached; until then (and for any messages Bloom doesn't handle) it just
/// defers to the system.
#[cfg(target_os = "windows")]
unsafe extern "system" fn bloom_host_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Register the host window class once. A plain child window whose background
/// is never erased (the GPU swapchain owns every pixel), which avoids flicker
/// between presents.
#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    thread_local! {
        static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    REGISTERED.with(|r| {
        if r.get() {
            return;
        }
        unsafe {
            // Don't unwrap across the FFI boundary — bail and retry on the next
            // create() if the module handle is somehow unavailable.
            let hinstance = match GetModuleHandleW(None) {
                Ok(h) => h,
                Err(_) => return,
            };
            let class_name = to_wide("PerryBloomView");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(bloom_host_wndproc),
                hInstance: HINSTANCE(hinstance.0),
                lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
                // No background brush: the renderer fills the whole client area.
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                ..Default::default()
            };
            // Returns 0 if the class already exists (e.g. registered from another
            // thread) — harmless, the class is usable either way.
            RegisterClassExW(&wc);
        }
        r.set(true);
    });
}

/// Create a BloomView with the given logical width/height. Returns the widget
/// handle. The reserved size is fixed so the viewport claims space in a stack.
pub fn create(width: f64, height: f64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        ensure_class_registered();
        let class_name = to_wide("PerryBloomView");
        let window_text = to_wide("");
        unsafe {
            // Avoid panicking across the FFI boundary; return an invalid (0)
            // handle if the OS refuses the module handle or the window.
            let hinstance = match GetModuleHandleW(None) {
                Ok(h) => h,
                Err(_) => return 0,
            };
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                width as i32,
                height as i32,
                Some(super::get_parking_hwnd()),
                None,
                Some(HINSTANCE::from(hinstance)),
                None,
            ) {
                Ok(h) => h,
                Err(_) => return 0,
            };

            let handle =
                register_widget_with_layout(hwnd, WidgetKind::Image, 0.0, (0.0, 0.0, 0.0, 0.0));
            // Reserve the requested size so the view is visible in a layout.
            set_fixed_width(handle, width as i32);
            set_fixed_height(handle, height as i32);
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (width, height);
        super::register_widget_with_layout(0, super::WidgetKind::Image, 0.0, (0.0, 0.0, 0.0, 0.0))
    }
}

/// Return the raw HWND value for a BloomView handle as an integer, for handing
/// to an external renderer (`attachToHwnd`). Returns 0 if the handle is unknown.
pub fn get_hwnd_value(handle: i64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        match super::get_hwnd(handle) {
            Some(hwnd) => hwnd.0 as i64,
            None => 0,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        super::get_hwnd(handle).unwrap_or(0) as i64
    }
}
