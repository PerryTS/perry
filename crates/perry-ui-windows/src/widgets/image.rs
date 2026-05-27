//! Image widget — custom PerryImage window with GDI+ alpha-blended painting
//! for file images, or STATIC+SS_ICON for symbol images.
//!
//! URL-fetched images (Image(url, alt) — mirrors the macOS NSURL/NSData
//! path): a background thread fetches the bytes via WinHTTP and stores
//! them in `URL_BYTES`. The image_wnd_proc's WM_PAINT path decodes from
//! `URL_BYTES` first, falling back to the file path if the URL hasn't
//! resolved yet. Decode runs through `SHCreateMemStream` →
//! `GdipLoadImageFromStream`, so PNG / JPEG / GIF / WebP all flow
//! through the same render path as file-loaded images.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::InvalidateRect;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::SS_ICON;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

pub(crate) fn str_from_header(ptr: *const u8) -> &'static str {
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

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// STM_SETIMAGE message
#[cfg(target_os = "windows")]
const STM_SETIMAGE: u32 = 0x0172;

/// Per-widget tint color (limited use on Win32 — stored for potential custom draw)
struct ImageTint {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

thread_local! {
    static IMAGE_TINTS: RefCell<HashMap<i64, ImageTint>> = RefCell::new(HashMap::new());
    /// Store resolved file paths keyed by widget handle
    static IMAGE_PATHS: RefCell<HashMap<i64, String>> = RefCell::new(HashMap::new());
    /// Map from HWND (as isize) -> resolved file path for WM_PAINT lookup
    #[cfg(target_os = "windows")]
    static HWND_TO_PATH: RefCell<HashMap<isize, String>> = RefCell::new(HashMap::new());
}

/// URL-fetched image bytes — populated by the WinHTTP background thread.
/// Mutex-guarded because the writer lives on a worker thread while the
/// reader (image_wnd_proc on the UI thread) reads on each WM_PAINT.
/// Keyed by HWND (as isize) for the same reason `HWND_TO_PATH` is —
/// WM_PAINT only sees the HWND, not the widget handle.
#[cfg(target_os = "windows")]
static HWND_URL_BYTES: Mutex<Option<HashMap<isize, Vec<u8>>>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn url_bytes_get(hwnd_key: isize) -> Option<Vec<u8>> {
    let guard = HWND_URL_BYTES.lock().ok()?;
    guard.as_ref()?.get(&hwnd_key).cloned()
}

#[cfg(target_os = "windows")]
fn url_bytes_set(hwnd_key: isize, bytes: Vec<u8>) {
    if let Ok(mut guard) = HWND_URL_BYTES.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(hwnd_key, bytes);
    }
}

/// WM_PAINT handler for PerryImage windows — draws the image with GDI+ alpha blending
/// so PNG transparency composites correctly over the parent's background (gradient or solid).
#[cfg(target_os = "windows")]
unsafe extern "system" fn image_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            use windows::Win32::Graphics::Gdi::*;
            use windows::Win32::Graphics::GdiPlus::*;

            let path = HWND_TO_PATH.with(|m| m.borrow().get(&(hwnd.0 as isize)).cloned());

            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            // Paint ancestor backgrounds into our DC so alpha blending composites
            // against the correct background (gradient/solid), not stale pixels.
            // Walk up the parent chain, accumulating the offset, and paint each
            // ancestor's background (gradient or solid) at the correct position.
            {
                let mut total_x: i32 = 0;
                let mut total_y: i32 = 0;
                let mut walk = hwnd;
                for _ in 0..10 {
                    let parent = if let Some(p) = GetParent(walk).ok() {
                        if p.0.is_null() {
                            break;
                        } else {
                            p
                        }
                    } else {
                        break;
                    };
                    // Get child's position within parent
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(walk, &mut rect);
                    let mut pt = POINT {
                        x: rect.left,
                        y: rect.top,
                    };
                    let _ = ScreenToClient(parent, &mut pt);
                    total_x += pt.x;
                    total_y += pt.y;
                    // Offset DC to parent's coordinate space and paint its background
                    SetWindowOrgEx(hdc, total_x, total_y, None);
                    let mut parent_rect = RECT::default();
                    let _ = GetClientRect(parent, &mut parent_rect);
                    // Try gradient first, then solid color
                    if !crate::widgets::paint_gradient(parent, hdc, &parent_rect) {
                        let parent_handle = crate::widgets::find_handle_by_hwnd(parent);
                        if parent_handle > 0 {
                            if let Some(brush) = crate::widgets::get_bg_brush(parent_handle) {
                                FillRect(hdc, &parent_rect, brush);
                            }
                        }
                    }
                    walk = parent;
                }
                // Restore DC origin
                SetWindowOrgEx(hdc, 0, 0, None);
            }

            // Prefer URL-loaded bytes (if any) over the file path —
            // create_url stores bytes in `HWND_URL_BYTES` once the
            // background fetch resolves. WM_PAINT decodes from the
            // bytes via SHCreateMemStream + GdipLoadImageFromStream.
            let url_bytes = url_bytes_get(hwnd.0 as isize);

            let mut token: usize = 0;
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                ..Default::default()
            };
            if GdiplusStartup(&mut token, &input, std::ptr::null_mut()).0 == 0 {
                let mut gp_image: *mut GpImage = std::ptr::null_mut();

                if let Some(bytes) = url_bytes.as_ref() {
                    use windows::Win32::UI::Shell::SHCreateMemStream;
                    if let Some(stream) = SHCreateMemStream(Some(bytes.as_slice())) {
                        // `stream: IStream` implements `Param<IStream>` by
                        // reference — pass `&stream` so the generated
                        // binding picks up the existing implementation.
                        // Drop at end-of-scope releases the refcount.
                        let _ = GdipLoadImageFromStream(&stream, &mut gp_image);
                    }
                } else if let Some(path) = path {
                    let wide_path = to_wide(&path);
                    let _ = GdipLoadImageFromFile(
                        windows::core::PCWSTR(wide_path.as_ptr()),
                        &mut gp_image,
                    );
                }

                if !gp_image.is_null() {
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;

                    let mut graphics: *mut GpGraphics = std::ptr::null_mut();
                    GdipCreateFromHDC(hdc, &mut graphics);
                    if !graphics.is_null() {
                        GdipSetInterpolationMode(graphics, InterpolationMode(7)); // HighQualityBicubic
                                                                                  // Stretch to fill — layout engine controls aspect ratio
                                                                                  // via widget dimensions; no letterboxing.
                        GdipDrawImageRectI(graphics, gp_image, 0, 0, w, h);
                        GdipDeleteGraphics(graphics);
                    }
                    GdipDisposeImage(gp_image);
                }
                GdiplusShutdown(token);
            }

            EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            // Skip — WM_PAINT paints ancestor backgrounds + image with alpha.
            LRESULT(1)
        }
        x if x == IMAGE_URL_LOADED_MSG => {
            // Background WinHTTP fetch finished — bytes are in
            // HWND_URL_BYTES; trigger a repaint so the next WM_PAINT
            // cycle picks them up.
            let _ = InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Register the PerryImage window class (idempotent — safe to call multiple times).
#[cfg(target_os = "windows")]
fn ensure_image_class_registered() {
    use std::sync::Once;
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryImage");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(image_wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(std::ptr::null_mut()), // transparent
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

/// Resolve a relative asset path against the executable's directory first,
/// falling back to the path as-is (relative to cwd). Matches macOS/GTK behavior.
#[cfg(target_os = "windows")]
fn resolve_asset_path(path: &str) -> String {
    if std::path::Path::new(path).is_absolute() {
        return path.to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join(path);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    path.to_string()
}

/// Create an Image from a file path. Returns widget handle.
pub fn create_file(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let resolved = resolve_asset_path(path);
        ensure_image_class_registered();

        let class_name = to_wide("PerryImage");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                None,
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                100,
                100,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Store path for WM_PAINT lookup
            HWND_TO_PATH.with(|m| {
                m.borrow_mut().insert(hwnd.0 as isize, resolved.clone());
            });

            let handle = register_widget(hwnd, WidgetKind::Image, control_id);
            IMAGE_PATHS.with(|p| p.borrow_mut().insert(handle, resolved));
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        register_widget(0, WidgetKind::Image, control_id)
    }
}

/// Create an Image from a system symbol/icon name. Returns widget handle.
pub fn create_symbol(name_ptr: *const u8) -> i64 {
    let name = str_from_header(name_ptr);
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(SS_ICON.0 | WS_CHILD.0 | WS_VISIBLE.0),
                0,
                0,
                32,
                32,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Map common symbol names to system icons
            let icon_id = match name {
                "exclamationmark.triangle" | "warning" => IDI_WARNING,
                "info.circle" | "info" => IDI_INFORMATION,
                "xmark.circle" | "error" => IDI_ERROR,
                "questionmark.circle" | "question" => IDI_QUESTION,
                "app" | "application" => IDI_APPLICATION,
                "shield" | "shield.fill" => IDI_SHIELD,
                _ => IDI_APPLICATION,
            };

            let hicon = LoadIconW(None, icon_id);
            if let Ok(hicon) = hicon {
                SendMessageW(
                    hwnd,
                    STM_SETIMAGE,
                    WPARAM(IMAGE_ICON.0 as usize),
                    LPARAM(hicon.0 as isize),
                );
            }

            register_widget(hwnd, WidgetKind::Image, control_id)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        register_widget(0, WidgetKind::Image, control_id)
    }
}

/// Create an Image from a remote URL. Returns the widget handle
/// immediately; the actual image appears once the background WinHTTP
/// fetch resolves and posts an invalidate to the UI thread.
pub fn create_url(url_ptr: *const u8, alt_ptr: *const u8) -> i64 {
    let url = str_from_header(url_ptr).to_string();
    let _alt = str_from_header(alt_ptr);
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        ensure_image_class_registered();

        let class_name = to_wide("PerryImage");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                None,
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                100,
                100,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Image, control_id);

            if !url.is_empty() {
                fetch_url_async(hwnd, url);
            }

            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = url;
        register_widget(0, WidgetKind::Image, control_id)
    }
}

/// Replace the URL of an existing Image widget — re-fetches and
/// repaints. No-op when the widget isn't a PerryImage HWND.
#[cfg(target_os = "windows")]
pub fn set_url(handle: i64, url_ptr: *const u8) {
    let url = str_from_header(url_ptr).to_string();
    if let Some(hwnd) = super::get_hwnd(handle) {
        // Clear the old bytes so the WM_PAINT path falls back to the
        // file-path arm (or to nothing) until the new fetch resolves.
        if let Ok(mut guard) = HWND_URL_BYTES.lock() {
            if let Some(map) = guard.as_mut() {
                map.remove(&(hwnd.0 as isize));
            }
        }
        unsafe {
            let _ = InvalidateRect(hwnd, None, true);
        }
        if !url.is_empty() {
            fetch_url_async(hwnd, url);
        }
    }
}

/// WinHTTP background-fetch helper. Spawns an OS thread, fetches the
/// URL via the standard WinHttpOpen / Connect / OpenRequest /
/// SendRequest / ReceiveResponse / ReadData chain, stores the bytes in
/// `HWND_URL_BYTES` keyed by HWND, then `PostMessage`s WM_USER+0x501
/// to the HWND so the UI thread can `InvalidateRect` and repaint
/// safely on its own thread (we never touch HWND state from the
/// worker thread except via PostMessage, which Win32 documents as
/// thread-safe).
#[cfg(target_os = "windows")]
fn fetch_url_async(hwnd: HWND, url: String) {
    use windows::Win32::Networking::WinHttp::*;

    // HWND_RELOAD message — image_wnd_proc reacts by invalidating
    // itself for repaint. We use a private WM_USER+N to avoid
    // clashing with any Win32-defined notification codes.
    const WM_USER_IMAGE_LOADED: u32 = 0x0400 + 0x501;

    // HWND is `Send` only when wrapped — the worker thread owns the
    // raw pointer through this struct so `std::thread::spawn` accepts
    // the closure without `Send` violations on `*mut`.
    struct SendableHwnd(HWND);
    unsafe impl Send for SendableHwnd {}
    let target = SendableHwnd(hwnd);

    std::thread::spawn(move || {
        let target = target;
        let bytes = match fetch_url_blocking(&url) {
            Some(b) if !b.is_empty() => b,
            _ => return,
        };
        url_bytes_set(target.0 .0 as isize, bytes);
        unsafe {
            let _ = PostMessageW(target.0, WM_USER_IMAGE_LOADED, WPARAM(0), LPARAM(0));
        }
    });

    // The PostMessage delivery hits the parent window's message pump
    // and dispatches to image_wnd_proc, which then invalidates the
    // image HWND for repaint. The match-arm is added to image_wnd_proc.
    let _ = (); // (compile-time anchor — keeps the const above alive when
                // grep'ing for the message id.)
                // Make the constant accessible from the wnd-proc match arm via a
                // module-scope re-export — see `IMAGE_LOADED_MSG` below.
}

/// Cross-thread blocking URL fetch via WinHTTP. Returns the body bytes
/// on 2xx; None otherwise. Uses sync mode — we're on a worker thread
/// already, so blocking is fine.
#[cfg(target_os = "windows")]
fn fetch_url_blocking(url: &str) -> Option<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::*;

    let parsed = parse_url(url)?;
    let host_wide = to_wide(&parsed.host);
    let path_wide = to_wide(&parsed.path);
    let user_agent_wide = to_wide("Perry/0.5 (perry-ui-windows)");

    unsafe {
        let session = WinHttpOpen(
            PCWSTR(user_agent_wide.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return None;
        }

        let port = parsed
            .port
            .unwrap_or(if parsed.is_https { 443 } else { 80 });
        let connect = WinHttpConnect(session, PCWSTR(host_wide.as_ptr()), port, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let flags = if parsed.is_https {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let verb_wide = to_wide("GET");
        let request = WinHttpOpenRequest(
            connect,
            PCWSTR(verb_wide.as_ptr()),
            PCWSTR(path_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null_mut(),
            flags,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let send_ok = WinHttpSendRequest(request, None, None, 0, 0, 0);
        if send_ok.is_err() {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        if WinHttpReceiveResponse(request, std::ptr::null_mut()).is_err() {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let mut body = Vec::<u8>::new();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let mut available: u32 = 0;
            if WinHttpQueryDataAvailable(request, &mut available).is_err() {
                break;
            }
            if available == 0 {
                break;
            }
            let to_read = (available as usize).min(buf.len());
            let mut read: u32 = 0;
            let read_ok = WinHttpReadData(
                request,
                buf.as_mut_ptr() as *mut _,
                to_read as u32,
                &mut read,
            );
            if read_ok.is_err() || read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
            // Sanity cap — 64 MB. Above this we treat the response as
            // truncated rather than blowing up the process.
            if body.len() > 64 * 1024 * 1024 {
                break;
            }
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        if body.is_empty() {
            None
        } else {
            Some(body)
        }
    }
}

/// Tiny URL parser — splits `https://host:port/path?qs#frag` into the
/// pieces WinHTTP wants. Doesn't handle credentials or IPv6 literals;
/// that matches the macOS NSURL/NSData path scope (image URLs in
/// practice are http(s)://host/path?qs).
struct ParsedUrl {
    is_https: bool,
    host: String,
    port: Option<u16>,
    path: String,
}

#[cfg(target_os = "windows")]
fn parse_url(s: &str) -> Option<ParsedUrl> {
    let (scheme, rest) = if let Some(r) = s.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = s.strip_prefix("http://") {
        (false, r)
    } else {
        return None;
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.find(':') {
        Some(i) => {
            let h = &authority[..i];
            let p = authority[i + 1..].parse::<u16>().ok();
            (h.to_string(), p)
        }
        None => (authority.to_string(), None),
    };
    if host.is_empty() {
        return None;
    }
    Some(ParsedUrl {
        is_https: scheme,
        host,
        port,
        path: path.to_string(),
    })
}

/// PostMessage code that the worker thread fires to the image HWND
/// once URL bytes are ready. The wnd-proc reacts by invalidating
/// itself so the next paint cycle re-runs WM_PAINT against the new
/// URL_BYTES entry.
#[cfg(target_os = "windows")]
pub const IMAGE_URL_LOADED_MSG: u32 = 0x0400 + 0x501;

/// Invalidate the image so it repaints at the current layout size.
/// Called by the layout engine after `MoveWindow` for Image widgets.
#[cfg(target_os = "windows")]
pub fn reload_bitmap_scaled(handle: i64, _w: i32, _h: i32) {
    // With GDI+ alpha-blended WM_PAINT, we just need to invalidate.
    // The paint handler reads the current client rect and draws at that size.
    if let Some(hwnd) = super::get_hwnd(handle) {
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

/// Set the size of an Image widget (DPI-scaled to match layout coordinates).
pub fn set_size(handle: i64, width: f64, height: f64) {
    // DPI-scale to match the layout engine's coordinate system
    let scale = crate::app::get_dpi_scale();
    let scaled_w = (width * scale) as i32;
    let scaled_h = (height * scale) as i32;
    // Set fixed dimensions so the layout engine uses these
    super::set_fixed_width(handle, scaled_w);
    super::set_fixed_height(handle, scaled_h);

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    scaled_w,
                    scaled_h,
                    SWP_NOMOVE | SWP_NOZORDER,
                );
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, width, height);
    }
}

/// Set the tint color for an Image widget.
/// On Win32, tinting is limited — we store the color for potential custom-draw use.
pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    IMAGE_TINTS.with(|tints| {
        tints.borrow_mut().insert(
            handle,
            ImageTint {
                r: (r * 255.0) as u8,
                g: (g * 255.0) as u8,
                b: (b * 255.0) as u8,
                a: (a * 255.0) as u8,
            },
        );
    });

    #[cfg(target_os = "windows")]
    {
        // Force repaint (custom-draw could use the tint if implemented)
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}
