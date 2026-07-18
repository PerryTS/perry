//! WebView widget — `CoreWebView2` (Microsoft Edge WebView2) on Windows.
//!
//! Issue #658 Phase 2. Mirrors the WKWebView impl on macOS / iOS / visionOS.
//!
//! ## Architecture
//!
//! Perry's widget system is HWND-based. WebView2's `ICoreWebView2Controller`
//! doesn't itself produce an HWND — it operates as a child surface inside a
//! parent HWND, with explicit bounds set via `put_Bounds`. So we:
//!
//! 1. Create a host STATIC child HWND. This becomes the registered widget
//!    handle.
//! 2. Synchronously kick off the WebView2 async init (env → controller),
//!    pumping the message loop until completion. WebView2 init is fast
//!    (~tens of ms first time, near-zero on warm cache); blocking is OK
//!    given the bounded scope.
//! 3. Subscribe to `NavigationStarting` (sync intercept), `NavigationCompleted`
//!    (`onLoaded` + error path), and `WebResourceRequested` (not used in v1).
//! 4. Forward layout-engine resize events (`WM_SIZE` on the host HWND) to
//!    the controller via a small subclass.
//!
//! ## Cookie isolation
//!
//! `ephemeral: true` (the default) maps to a per-handle temporary
//! `userDataFolder` under `%TEMP%\PerryWebView\<pid>-<handle>`. The folder
//! is created on init and best-effort deleted via `clear_cookies`. Persistent
//! mode shares the default Edge profile location.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};

#[cfg(target_os = "windows")]
use webview2_com::Microsoft::Web::WebView2::Win32::*;
#[cfg(target_os = "windows")]
use webview2_com::*;
#[cfg(target_os = "windows")]
use windows::core::{Interface, BOOL, PCWSTR, PWSTR};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::CoTaskMemFree;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_is_truthy(value: f64) -> i32;
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

#[cfg(target_os = "windows")]
fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let p = perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    unsafe { js_nanbox_string(p as i64) }
}

#[cfg(target_os = "windows")]
fn pcwstr_to_string(ptr: PWSTR) -> String {
    if ptr.0.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        while *ptr.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr.0, len);
        String::from_utf16_lossy(slice)
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Initialize COM as a single-threaded apartment for the current (UI) thread.
/// WebView2 environment creation requires an STA with a running message pump;
/// without it the async completion handler never fires. Idempotent — a thread
/// already in an apartment returns `S_FALSE` / `RPC_E_CHANGED_MODE`, both of
/// which are fine for our purposes (we just need an STA to exist).
#[cfg(target_os = "windows")]
fn ensure_com_sta() {
    use std::sync::Once;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    static COM_INIT: Once = Once::new();
    COM_INIT.call_once(|| unsafe {
        // Result intentionally ignored: S_OK on first init, S_FALSE if already
        // initialized STA, RPC_E_CHANGED_MODE if the thread is already MTA
        // (in which case WebView2 still works via the existing apartment).
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    });
}

#[cfg(target_os = "windows")]
fn host_of_url_string(s: &str) -> String {
    let after_scheme = match s.find("://") {
        Some(i) => &s[i + 3..],
        None => return String::new(),
    };
    let host_end = after_scheme
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_scheme.len());
    let host_with_port = &after_scheme[..host_end];
    match host_with_port.find(':') {
        Some(i) => host_with_port[..i].to_string(),
        None => host_with_port.to_string(),
    }
}

fn catch_panic<F: FnOnce() + std::panic::UnwindSafe>(label: &str, f: F) {
    if let Err(e) = std::panic::catch_unwind(f) {
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", e)
        };
        eprintln!("[perry] panic in {} (caught): {}", label, msg);
    }
}

fn host_in_allowlist(host: &str, allowlist: &[String]) -> bool {
    if allowlist.is_empty() {
        return true;
    }
    allowlist
        .iter()
        .any(|d| host == d || host.ends_with(&format!(".{}", d)))
}

#[cfg(target_os = "windows")]
struct WebViewState {
    host_hwnd: HWND,
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    on_should_navigate: f64,
    on_loaded: f64,
    on_error: f64,
    allowed_domains: Vec<String>,
    user_data_dir: Option<PathBuf>,
    /// Pending URL captured by `create()`'s url arg — applied once the
    /// controller resolves.
    pending_url: Option<String>,
}

#[cfg(target_os = "windows")]
thread_local! {
    static WEBVIEW_STATES: RefCell<HashMap<i64, WebViewState>> = RefCell::new(HashMap::new());
    /// HWND.0 (as isize) → widget handle reverse-lookup for the WM_SIZE
    /// subclass which only sees the HWND.
    static HWND_TO_HANDLE: RefCell<HashMap<isize, i64>> = RefCell::new(HashMap::new());
    static SUBCLASSED: RefCell<std::collections::HashSet<isize>> =
        RefCell::new(std::collections::HashSet::new());
}

#[cfg(target_os = "windows")]
const WEBVIEW_SUBCLASS_ID: usize = 0x77_76_69_77; // 'w','v','i','w'

/// True when `handle` belongs to a WebView. WebViews register their host
/// HWND as `WidgetKind::Image`, so kind checks can't identify them — the
/// message pump uses this to keep dialog-style Tab navigation away from
/// WebView2's Chromium children (they own their keyboard handling).
#[cfg(target_os = "windows")]
pub fn is_webview(handle: i64) -> bool {
    WEBVIEW_STATES.with(|m| m.borrow().contains_key(&handle))
}

#[cfg(not(target_os = "windows"))]
pub fn is_webview(_handle: i64) -> bool {
    false
}

static NEXT_DATA_DIR_TAG: AtomicI64 = AtomicI64::new(1);

// =============================================================================
// Public API
// =============================================================================

pub fn create(url_ptr: *const u8, width: f64, height: f64, ephemeral_hint: f64) -> i64 {
    let url = str_from_header(url_ptr).to_string();
    let control_id = alloc_control_id();
    let _ = ephemeral_hint;

    #[cfg(target_os = "windows")]
    {
        // WebView2's `CreateCoreWebView2EnvironmentWithOptions` requires the
        // calling thread to be a COM single-threaded apartment (STA) with a
        // running message loop — otherwise the async environment-creation
        // completion handler is never posted back to our pump and init times
        // out with HRESULT(0x80070005) "no result", leaving the pane blank
        // and firing no navigation events (issue #4835). The dialog / file
        // picker paths already call `CoInitializeEx(APARTMENTTHREADED)` for
        // the same reason; WebView never did. This call is idempotent and
        // tolerates a prior init (`S_FALSE` / `RPC_E_CHANGED_MODE`).
        ensure_com_sta();

        let class_name = to_wide("STATIC");
        let window_text = to_wide("");
        let host = unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(window_text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                if width > 0.0 { width as i32 } else { 600 },
                if height > 0.0 { height as i32 } else { 400 },
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            )
        }
        .unwrap();

        let handle = register_widget(host, WidgetKind::Image, control_id);
        HWND_TO_HANDLE.with(|m| {
            m.borrow_mut().insert(host.0 as isize, handle);
        });

        // v2-B: ephemeral_hint controls userDataFolder choice at env-creation
        // time. ephemeral=true (default) → per-handle temp dir; false →
        // shared persistent dir under %LOCALAPPDATA%\PerryWebView so cookies
        // survive across sessions / WebView instances in the same app.
        let user_data_dir = if ephemeral_hint > 0.5 {
            ephemeral_user_data_dir()
        } else {
            persistent_user_data_dir()
        };

        WEBVIEW_STATES.with(|s| {
            s.borrow_mut().insert(
                handle,
                WebViewState {
                    host_hwnd: host,
                    controller: None,
                    webview: None,
                    on_should_navigate: 0.0,
                    on_loaded: 0.0,
                    on_error: 0.0,
                    allowed_domains: Vec::new(),
                    user_data_dir: Some(user_data_dir.clone()),
                    pending_url: if url.is_empty() {
                        None
                    } else {
                        Some(url.clone())
                    },
                },
            );
        });

        ensure_size_subclass(host);

        // Kick off async WebView2 init. Pumps messages until both env +
        // controller are ready, then applies the pending URL.
        if init_webview2_sync(handle, &user_data_dir).is_ok() {
            apply_pending(handle);
        }

        handle
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (url, width, height);
        register_widget(0, WidgetKind::Image, control_id)
    }
}

#[cfg(target_os = "windows")]
fn ephemeral_user_data_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("PerryWebView");
    let pid = std::process::id();
    let tag = NEXT_DATA_DIR_TAG.fetch_add(1, Ordering::Relaxed);
    p.push(format!("{}-{}", pid, tag));
    let _ = std::fs::create_dir_all(&p);
    p
}

/// Persistent userDataFolder for `ephemeral: false`. Located under
/// `%LOCALAPPDATA%\PerryWebView\persistent` so cookies survive across
/// app launches, the same way Edge / Chrome's profile dir works.
#[cfg(target_os = "windows")]
fn persistent_user_data_dir() -> PathBuf {
    let mut p = std::env::var("LOCALAPPDATA")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir());
    p.push("PerryWebView");
    p.push("persistent");
    let _ = std::fs::create_dir_all(&p);
    p
}

#[cfg(target_os = "windows")]
fn init_webview2_sync(handle: i64, user_data_dir: &PathBuf) -> windows::core::Result<()> {
    use std::cell::Cell;
    use std::rc::Rc;

    let host_hwnd = WEBVIEW_STATES.with(|s| s.borrow().get(&handle).map(|st| st.host_hwnd));
    let host_hwnd = match host_hwnd {
        Some(h) => h,
        None => return Ok(()),
    };

    // `env_done` holds the (non-Copy) result; `env_ready` is a separate Copy
    // flag the message-pump predicate can poll *without* consuming the result.
    // The previous code polled `env_done.take().is_some()`, which removed the
    // value inside the predicate — so the subsequent `env_done.take()` always
    // saw `None` and reported a spurious "no result" error even on a fully
    // successful init (issue #4835). Keep the result intact; only `take()` it
    // once, after the loop exits.
    let env_done: Rc<Cell<Option<windows::core::Result<ICoreWebView2Environment>>>> =
        Rc::new(Cell::new(None));
    let env_ready: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let env_done_clone = env_done.clone();
    let env_ready_clone = env_ready.clone();

    let user_data_wide = to_wide(user_data_dir.to_string_lossy().as_ref());

    let env_handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |error_code, environment| {
            if error_code.is_err() {
                env_done_clone.set(Some(Err(windows::core::Error::from(
                    error_code.unwrap_err(),
                ))));
            } else if let Some(env) = environment {
                env_done_clone.set(Some(Ok(env)));
            } else {
                env_done_clone.set(Some(Err(windows::core::Error::from(
                    windows::core::HRESULT::from_win32(unsafe {
                        windows::Win32::Foundation::GetLastError().0
                    }),
                ))));
            }
            env_ready_clone.set(true);
            Ok(())
        },
    ));

    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(user_data_wide.as_ptr()),
            None,
            &env_handler,
        )?;
    }
    pump_messages_until(|| env_ready.get());
    let env_result = env_done.take().unwrap_or_else(|| {
        Err(windows::core::Error::new(
            windows::core::HRESULT(0x8007_0005_u32 as i32),
            "WebView2 env init: no result",
        ))
    });
    let env = env_result?;

    // Step 2: create controller bound to host_hwnd.
    let ctrl_done: Rc<Cell<Option<windows::core::Result<ICoreWebView2Controller>>>> =
        Rc::new(Cell::new(None));
    let ctrl_ready: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let ctrl_done_clone = ctrl_done.clone();
    let ctrl_ready_clone = ctrl_ready.clone();

    let ctrl_handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |error_code, controller| {
            if error_code.is_err() {
                ctrl_done_clone.set(Some(Err(windows::core::Error::from(
                    error_code.unwrap_err(),
                ))));
            } else if let Some(c) = controller {
                ctrl_done_clone.set(Some(Ok(c)));
            } else {
                ctrl_done_clone.set(Some(Err(windows::core::Error::from(
                    windows::core::HRESULT::from_win32(unsafe {
                        windows::Win32::Foundation::GetLastError().0
                    }),
                ))));
            }
            ctrl_ready_clone.set(true);
            Ok(())
        },
    ));

    unsafe {
        env.CreateCoreWebView2Controller(host_hwnd, &ctrl_handler)?;
    }
    // Same peek-don't-consume pattern as the environment handler above.
    pump_messages_until(|| ctrl_ready.get());
    let ctrl_result = ctrl_done.take().unwrap_or_else(|| {
        Err(windows::core::Error::new(
            windows::core::HRESULT(0x8007_0005_u32 as i32),
            "WebView2 controller init: no result",
        ))
    });
    let controller = ctrl_result?;

    // Wire up the WebView2: bounds + nav events.
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(host_hwnd, &mut rect);
        controller.SetBounds(rect)?;
        controller.SetIsVisible(true)?;
    }

    let webview = unsafe { controller.CoreWebView2()? };
    install_navigation_handlers(handle, &webview);

    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow_mut().get_mut(&handle) {
            st.controller = Some(controller);
            st.webview = Some(webview);
        }
    });

    Ok(())
}

#[cfg(target_os = "windows")]
fn install_navigation_handlers(handle: i64, webview: &ICoreWebView2) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;

    // NavigationStarting — sync intercept. Allows the user's
    // onShouldNavigate closure to cancel via `args.put_Cancel(true)`.
    let nav_starting = NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
        let args = match args {
            Some(a) => a,
            None => return Ok(()),
        };
        let url = unsafe {
            let mut uri = PWSTR::null();
            let _ = args.Uri(&mut uri);
            let s = pcwstr_to_string(uri);
            if !uri.0.is_null() {
                CoTaskMemFree(Some(uri.0 as *const _));
            }
            s
        };

        let (on_should, allowed) = WEBVIEW_STATES.with(|s| {
            s.borrow()
                .get(&handle)
                .map(|st| (st.on_should_navigate, st.allowed_domains.clone()))
                .unwrap_or((0.0, Vec::new()))
        });

        // Allowlist gate first.
        if !allowed.is_empty() {
            let host = host_of_url_string(&url);
            if !host_in_allowlist(&host, &allowed) {
                unsafe {
                    let _ = args.SetCancel(true);
                }
                return Ok(());
            }
        }

        if on_should != 0.0 {
            let url_nb = nanbox_str(&url);
            let closure_ptr = unsafe { js_nanbox_get_pointer(on_should) } as *const u8;
            let result_cell = Cell::new(f64::from_bits(0x7FFC_0000_0000_0001));
            if !closure_ptr.is_null() {
                let result_cell_ref = &result_cell;
                catch_panic(
                    "webview onShouldNavigate",
                    std::panic::AssertUnwindSafe(|| {
                        let r = unsafe { js_closure_call1(closure_ptr, url_nb) };
                        result_cell_ref.set(r);
                    }),
                );
            }
            let result = result_cell.get();
            let bits = result.to_bits();
            let is_undefined = bits == 0x7FFC_0000_0000_0001;
            let allow = is_undefined || unsafe { js_is_truthy(result) != 0 };
            if !allow {
                unsafe {
                    let _ = args.SetCancel(true);
                }
            }
        }

        Ok(())
    }));

    // NavigationCompleted — onLoaded on success, onError on failure.
    let nav_completed = NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
        let webview = match sender {
            Some(s) => s,
            None => return Ok(()),
        };
        let args = match args {
            Some(a) => a,
            None => return Ok(()),
        };
        let success = unsafe {
            let mut s: BOOL = BOOL(0);
            let _ = args.IsSuccess(&mut s);
            s.as_bool()
        };
        if success {
            let url = unsafe {
                let mut uri = PWSTR::null();
                let _ = webview.Source(&mut uri);
                let s = pcwstr_to_string(uri);
                if !uri.0.is_null() {
                    CoTaskMemFree(Some(uri.0 as *const _));
                }
                s
            };
            let on_loaded = WEBVIEW_STATES.with(|s| {
                s.borrow()
                    .get(&handle)
                    .map(|st| st.on_loaded)
                    .unwrap_or(0.0)
            });
            if on_loaded != 0.0 {
                let url_nb = nanbox_str(&url);
                let closure_ptr = unsafe { js_nanbox_get_pointer(on_loaded) } as *const u8;
                if !closure_ptr.is_null() {
                    catch_panic(
                        "webview onLoaded",
                        std::panic::AssertUnwindSafe(|| unsafe {
                            js_closure_call1(closure_ptr, url_nb);
                        }),
                    );
                }
            }
        } else {
            // Pull the error code.
            let status = unsafe {
                let mut st = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                let _ = args.WebErrorStatus(&mut st);
                st.0 as i64
            };
            let on_error = WEBVIEW_STATES
                .with(|s| s.borrow().get(&handle).map(|st| st.on_error).unwrap_or(0.0));
            if on_error != 0.0 {
                let msg_nb = nanbox_str(&format!("WebView2 error status {}", status));
                let closure_ptr = unsafe { js_nanbox_get_pointer(on_error) } as *const u8;
                if !closure_ptr.is_null() {
                    catch_panic(
                        "webview onError",
                        std::panic::AssertUnwindSafe(|| unsafe {
                            js_closure_call2(closure_ptr, status as f64, msg_nb);
                        }),
                    );
                }
            }
        }
        Ok(())
    }));

    let mut token: i64 = 0;
    unsafe {
        let _ = webview.add_NavigationStarting(&nav_starting, &mut token);
        let _ = webview.add_NavigationCompleted(&nav_completed, &mut token);
    }
}

#[cfg(target_os = "windows")]
fn pump_messages_until<F: FnMut() -> bool>(mut done: F) {
    unsafe {
        let mut msg = MSG::default();
        let start = std::time::Instant::now();
        while !done() {
            // Bail out after 10 s — WebView2 init typically completes in
            // tens of ms; longer than this means the runtime is missing.
            if start.elapsed().as_secs() >= 10 {
                break;
            }
            // PM_REMOVE is non-zero so an absent message returns FALSE
            // immediately. Mix in WaitMessage when the queue's empty so
            // we don't spin a CPU core while the WebView2 worker thread
            // does its work.
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                let _ = WaitMessage();
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_pending(handle: i64) {
    let url = WEBVIEW_STATES.with(|s| {
        s.borrow_mut()
            .get_mut(&handle)
            .and_then(|st| st.pending_url.take())
    });
    if let Some(url) = url {
        navigate(handle, &url);
    }
}

#[cfg(target_os = "windows")]
fn navigate(handle: i64, url: &str) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            if let Some(wv) = &st.webview {
                let wide = to_wide(url);
                unsafe {
                    let _ = wv.Navigate(PCWSTR(wide.as_ptr()));
                }
            } else {
                // Controller not ready yet — re-queue.
                drop(st);
            }
        }
    });
    // If controller wasn't ready, set pending_url for apply_pending.
    let needs_queue = WEBVIEW_STATES.with(|s| {
        s.borrow()
            .get(&handle)
            .map(|st| st.webview.is_none())
            .unwrap_or(false)
    });
    if needs_queue {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&handle) {
                st.pending_url = Some(url.to_string());
            }
        });
    }
}

// -----------------------------------------------------------------------------
// Imperative ops
// -----------------------------------------------------------------------------

pub fn load_url(handle: i64, url_ptr: *const u8) {
    let url = str_from_header(url_ptr).to_string();
    if url.is_empty() {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        navigate(handle, &url);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, url);
    }
}

pub fn reload(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow().get(&handle) {
                if let Some(wv) = &st.webview {
                    unsafe {
                        let _ = wv.Reload();
                    }
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn go_back(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow().get(&handle) {
                if let Some(wv) = &st.webview {
                    unsafe {
                        let _ = wv.GoBack();
                    }
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn go_forward(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow().get(&handle) {
                if let Some(wv) = &st.webview {
                    unsafe {
                        let _ = wv.GoForward();
                    }
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn can_go_back(handle: i64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        let v = WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow().get(&handle) {
                if let Some(wv) = &st.webview {
                    let mut can = BOOL(0);
                    unsafe {
                        let _ = wv.CanGoBack(&mut can);
                    }
                    return if can.as_bool() { 1 } else { 0 };
                }
            }
            0
        });
        return v;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
        0
    }
}

pub fn evaluate_js(handle: i64, js_ptr: *const u8, callback: f64) {
    let js = str_from_header(js_ptr).to_string();
    #[cfg(target_os = "windows")]
    {
        let webview =
            WEBVIEW_STATES.with(|s| s.borrow().get(&handle).and_then(|st| st.webview.clone()));
        let webview = match webview {
            Some(w) => w,
            None => return,
        };

        let cb_handler =
            ExecuteScriptCompletedHandler::create(Box::new(move |error_code, result_json| {
                let s = if error_code.is_err() {
                    String::new()
                } else {
                    // result_json is a JSON-encoded string. For simple results
                    // (e.g. document.cookie returns a JS string), the result is
                    // a JSON-quoted string. Strip outer quotes for ergonomics
                    // when it's a plain string; otherwise pass through.
                    let raw = result_json;
                    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                        let inner = &raw[1..raw.len() - 1];
                        inner.replace("\\\"", "\"").replace("\\\\", "\\")
                    } else if raw == "null" {
                        String::new()
                    } else {
                        raw
                    }
                };
                let nb = nanbox_str(&s);
                let closure_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
                if !closure_ptr.is_null() {
                    catch_panic(
                        "webview evaluateJs callback",
                        std::panic::AssertUnwindSafe(|| unsafe {
                            js_closure_call1(closure_ptr, nb);
                        }),
                    );
                }
                Ok(())
            }));

        let js_wide = to_wide(&js);
        unsafe {
            let _ = webview.ExecuteScript(PCWSTR(js_wide.as_ptr()), &cb_handler);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, js, callback);
    }
}

pub fn clear_cookies(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        // CoreWebView2.Profile is the modern API but requires WebView2
        // 1.0.992+. For maximum compat: use ICoreWebView2_2.CookieManager
        // and remove all. As a v1 fallback, also remove the temp
        // userDataFolder when ephemeral mode is on (gets recreated next time).
        let manager = WEBVIEW_STATES.with(|s| {
            s.borrow().get(&handle).and_then(|st| {
                let wv = st.webview.as_ref()?;
                let v2: ICoreWebView2_2 = wv.cast().ok()?;
                let mgr = unsafe { v2.CookieManager().ok()? };
                Some(mgr)
            })
        });
        if let Some(mgr) = manager {
            unsafe {
                let _ = mgr.DeleteAllCookies();
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn set_user_agent(handle: i64, ua_ptr: *const u8) {
    let ua = str_from_header(ua_ptr).to_string();
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow().get(&handle) {
                if let Some(wv) = &st.webview {
                    if let Ok(v2) = wv.cast::<ICoreWebView2_2>() {
                        if let Ok(settings) = unsafe { v2.Settings() } {
                            if let Ok(s2) = settings.cast::<ICoreWebView2Settings2>() {
                                let wide = to_wide(&ua);
                                unsafe {
                                    let _ = s2.SetUserAgent(PCWSTR(wide.as_ptr()));
                                }
                            }
                        }
                    }
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, ua);
    }
}

pub fn set_allowed_domains(handle: i64, domains_arr_handle: i64) {
    extern "C" {
        fn js_array_get_length(arr: i64) -> i64;
        fn js_array_get_element_f64(arr: i64, index: i64) -> f64;
        fn js_get_string_pointer_unified(value: f64) -> *const u8;
    }
    let mut domains = Vec::new();
    unsafe {
        let len = js_array_get_length(domains_arr_handle);
        for i in 0..len {
            let elem = js_array_get_element_f64(domains_arr_handle, i);
            let str_ptr = js_get_string_pointer_unified(elem);
            if !str_ptr.is_null() {
                domains.push(str_from_header(str_ptr).to_string());
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&handle) {
                st.allowed_domains = domains;
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, domains);
    }
}

pub fn set_ephemeral(_handle: i64, _ephemeral: i64) {
    // WebView2's userDataFolder is set at env-creation time and can't be
    // changed mid-flight. We default to ephemeral (a per-handle temp dir)
    // in `create()`. To opt out, the user would need a "deferred
    // initialization" entry point we haven't added yet — track as a
    // follow-up if a real use case lands.
}

pub fn set_on_should_navigate(handle: i64, closure: f64) {
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&handle) {
                st.on_should_navigate = closure;
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, closure);
    }
}

pub fn set_on_loaded(handle: i64, closure: f64) {
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&handle) {
                st.on_loaded = closure;
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, closure);
    }
}

pub fn set_on_error(handle: i64, closure: f64) {
    #[cfg(target_os = "windows")]
    {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&handle) {
                st.on_error = closure;
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, closure);
    }
}

// -----------------------------------------------------------------------------
// Layout integration — WM_SIZE on the host HWND forwards to controller bounds.
// -----------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn ensure_size_subclass(host: HWND) {
    use windows::Win32::UI::Shell::SetWindowSubclass;
    let key = host.0 as isize;
    let installed = SUBCLASSED.with(|s| s.borrow().contains(&key));
    if !installed {
        unsafe {
            let _ = SetWindowSubclass(host, Some(size_subclass_proc), WEBVIEW_SUBCLASS_ID, 0);
        }
        SUBCLASSED.with(|s| {
            s.borrow_mut().insert(key);
        });
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn size_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    if msg == WM_SIZE {
        let handle = HWND_TO_HANDLE.with(|m| m.borrow().get(&(hwnd.0 as isize)).copied());
        if let Some(handle) = handle {
            let controller = WEBVIEW_STATES
                .with(|s| s.borrow().get(&handle).and_then(|st| st.controller.clone()));
            if let Some(controller) = controller {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = controller.SetBounds(rect);
            }
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
