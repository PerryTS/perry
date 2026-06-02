//! System tray icon (issue #490) — Linux StatusNotifierItem (KSNI).
//!
//! Modern Linux desktops (KDE Plasma, GNOME with the AppIndicator/KStatusNotifierItem
//! extension, XFCE, Cinnamon, MATE, Budgie) speak the freedesktop
//! `org.kde.StatusNotifierItem` DBus protocol. The pure-Rust `ksni`
//! crate handles the DBus protocol surface; we model each tray icon as
//! a long-lived `ksni::Service` running on a dedicated tokio current-
//! thread runtime in a background OS thread (mirrors the mpris-server
//! pattern in `media_playback.rs`).
//!
//! Architecture
//! ============
//! - `TRAYS: Mutex<Vec<Option<TrayHandle>>>` — process-wide registry,
//!   1-based indices match `menu.rs` / `widgets/`. We use a Mutex (not
//!   thread_local) because tray creation initiates the spawn from a
//!   tokio worker, and we need cross-thread access from `set_icon` /
//!   `set_tooltip` / `attach_menu` / `on_click` after the fact.
//! - `TrayState` carries the dynamic tray-icon state (icon path, tooltip,
//!   attached menu handle, click callback). Wrapped in `Arc<Mutex>` so
//!   the `impl ksni::Tray for PerryTray` can read from it.
//! - `PerryTray` is a thin wrapper that holds the shared state. KSNI's
//!   trait methods (`icon_pixmap`, `tool_tip`, `menu`, `activate`)
//!   read from the state on each invocation.
//! - Updates trigger a `Handle::update(|_| ())` async call so KSNI
//!   re-fetches everything and pushes property changes over DBus.
//!
//! Callback marshalling
//! ====================
//! KSNI invokes `activate()` and menu-item `activate` callbacks from
//! its tokio runtime (background thread). Perry's runtime is largely
//! thread-local — `js_closure_call0` must run on the GTK main thread
//! where the JS heap lives. We marshal via
//! `glib::MainContext::default().invoke(move || ...)`, identical to
//! the location.rs pattern.
//!
//! Tested DEs
//! ==========
//! KSNI works on KDE Plasma (native), GNOME 3.x+/40+ with the
//! `gnome-shell-extension-appindicator` extension installed, XFCE,
//! Cinnamon, MATE, Budgie, LXQt. On vanilla GNOME without the extension
//! the tray simply doesn't appear (KSNI's `Error::WontShow` path); we
//! log a one-line warning and keep the handle live so `setIcon` /
//! `attachMenu` / etc. don't crash, matching the macOS no-display
//! behavior on the menu-bar overflow.

#![cfg(target_os = "linux")]

use crate::menu::{snapshot_menu, MenuItemSnapshot};
use ksni::menu::{StandardItem, SubMenu};
use ksni::{Handle, MenuItem, ToolTip, TrayMethods};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::{Builder, Runtime};

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Extract a &str from a *const StringHeader pointer. Mirrors menu.rs.
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

/// Mutable per-tray state — read by `impl ksni::Tray for PerryTray` on
/// each property fetch. Updates require a `Handle::update` call to
/// trigger DBus property change broadcasts.
struct TrayState {
    /// Stable id for this tray service (StatusNotifierItem requires a
    /// session-stable name). We use `perry-tray-<pid>-<idx>`.
    id: String,
    /// Path to the icon file (PNG). Empty = no icon (we fall back to a
    /// freedesktop-named icon "applications-system" so the tray still
    /// shows up).
    icon_path: String,
    /// Tooltip text shown on hover.
    tooltip: String,
    /// Handle of the attached menu (in `menu.rs` MENUS storage), or 0
    /// for "no menu — just use onClick".
    menu_handle: i64,
    /// JS click callback (NaN-boxed f64), or 0.0 for "no callback".
    on_click: f64,
}

/// The KSNI tray implementation. Reads from `state` on every property
/// fetch; mutations to `state` (followed by `Handle::update`) propagate
/// over DBus.
struct PerryTray {
    state: Arc<Mutex<TrayState>>,
}

impl ksni::Tray for PerryTray {
    fn id(&self) -> String {
        self.state.lock().map(|s| s.id.clone()).unwrap_or_default()
    }

    fn title(&self) -> String {
        // Title in KSNI = the human-readable application name. Fall
        // back to the program name; tooltip carries the per-tray text.
        std::env::args()
            .next()
            .and_then(|p| {
                std::path::PathBuf::from(p)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "Perry".into())
    }

    fn icon_name(&self) -> String {
        // If the user supplied a full path, KSNI prefers `icon_pixmap`
        // (raw ARGB32 bytes). icon_name is for freedesktop-named icons
        // — fall back to a generic system icon when no path is set so
        // the tray surface still has a visible icon.
        let st = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        if st.icon_path.is_empty() {
            "applications-system".to_string()
        } else {
            String::new()
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let st = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        if st.icon_path.is_empty() {
            return Vec::new();
        }
        load_icon_argb32(&st.icon_path).into_iter().collect()
    }

    fn tool_tip(&self) -> ToolTip {
        let st = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return ToolTip::default(),
        };
        ToolTip {
            title: st.tooltip.clone(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let menu_handle = match self.state.lock() {
            Ok(s) => s.menu_handle,
            Err(_) => return Vec::new(),
        };
        if menu_handle <= 0 {
            return Vec::new();
        }
        build_ksni_menu(menu_handle)
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click: dispatch the JS callback on the GTK main thread.
        let cb = match self.state.lock() {
            Ok(s) => s.on_click,
            Err(_) => return,
        };
        if cb == 0.0 {
            return;
        }
        invoke_callback_on_main(cb);
    }
}

/// Schedule `js_closure_call0(callback_ptr)` on the GTK main loop. Safe
/// to call from any thread; the call site is the KSNI tokio worker.
fn invoke_callback_on_main(callback: f64) {
    use gtk4::glib;
    glib::MainContext::default().invoke(move || {
        let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
        if !ptr.is_null() {
            unsafe {
                js_closure_call0(ptr);
            }
        }
    });
}

/// Convert an on-disk PNG/JPEG/etc. to KSNI's ARGB32-network-order
/// representation. Uses `gtk4::gdk_pixbuf::Pixbuf` (already a transitive
/// dep — no extra build cost) which handles every format gdk-pixbuf
/// supports (PNG, JPEG, BMP, GIF, ICO, TIFF, …).
///
/// KSNI / SNI wants ARGB32 in network byte order: bytes laid out as
/// `[A, R, G, B, A, R, G, B, …]` — most-significant byte first.
fn load_icon_argb32(path: &str) -> Option<ksni::Icon> {
    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file(path).ok()?;
    let width = pixbuf.width();
    let height = pixbuf.height();
    if width <= 0 || height <= 0 {
        return None;
    }
    // Force RGBA (gdk-pixbuf can also be RGB after JPEG decode; add_alpha
    // converts both shapes to 4-channel RGBA so the byte walk below is
    // uniform). add_alpha returns a fresh Pixbuf via the GLib FFI, so
    // it can fail under OOM — fall back to the original Pixbuf and let
    // the channel-count branch below handle 3-channel RGB if needed.
    let pixbuf = pixbuf.add_alpha(false, 0, 0, 0).unwrap_or(pixbuf);
    let pixel_bytes = pixbuf.read_pixel_bytes();
    let pixels: &[u8] = pixel_bytes.as_ref();
    if pixels.is_empty() {
        return None;
    }
    let n_channels = pixbuf.n_channels() as usize;
    let rowstride = pixbuf.rowstride() as usize;
    let w = width as usize;
    let h = height as usize;

    // Convert from RGBA (gdk-pixbuf native after add_alpha — 4 channels)
    // to ARGB32 network byte order. The branch on n_channels handles the
    // rare case where add_alpha failed and we kept the 3-channel original.
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row_start = y * rowstride;
        for x in 0..w {
            let pixel_start = row_start + x * n_channels;
            if pixel_start + n_channels > pixels.len() {
                return None;
            }
            let r = pixels[pixel_start];
            let g = pixels[pixel_start + 1];
            let b = pixels[pixel_start + 2];
            let a = if n_channels >= 4 {
                pixels[pixel_start + 3]
            } else {
                0xff
            };
            // ARGB network byte order: A first, then R, G, B.
            out.push(a);
            out.push(r);
            out.push(g);
            out.push(b);
        }
    }
    Some(ksni::Icon {
        width,
        height,
        data: out,
    })
}

/// Recursively convert Perry's stored menu (`menu.rs` MENUS) into a
/// KSNI menu tree. Each item's `activate` Box closure captures the
/// JS callback pointer and dispatches on the GTK main thread.
fn build_ksni_menu(menu_handle: i64) -> Vec<MenuItem<PerryTray>> {
    let entries = snapshot_menu(menu_handle);
    let mut out: Vec<MenuItem<PerryTray>> = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            MenuItemSnapshot::Item {
                title,
                callback,
                shortcut,
            } => {
                let shortcut_vec = shortcut
                    .as_deref()
                    .map(parse_shortcut_for_ksni)
                    .unwrap_or_default();
                out.push(
                    StandardItem {
                        label: title,
                        shortcut: shortcut_vec,
                        activate: Box::new(move |_tray: &mut PerryTray| {
                            invoke_callback_on_main(callback);
                        }),
                        ..Default::default()
                    }
                    .into(),
                );
            }
            MenuItemSnapshot::Separator => {
                out.push(MenuItem::Separator);
            }
            MenuItemSnapshot::Submenu {
                title,
                submenu_handle,
            } => {
                let sub = build_ksni_menu(submenu_handle);
                out.push(
                    SubMenu {
                        label: title,
                        submenu: sub,
                        ..Default::default()
                    }
                    .into(),
                );
            }
        }
    }
    out
}

/// Convert a Perry shortcut string ("Cmd+S", "Shift+Alt+P") into KSNI's
/// `Vec<Vec<String>>` shape. KSNI wants modifier names "Control" /
/// "Alt" / "Shift" / "Super". We map "cmd"/"command"/"ctrl" → "Control"
/// (matches `shortcut_to_gtk_accel` semantics in menu.rs).
fn parse_shortcut_for_ksni(s: &str) -> Vec<Vec<String>> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut chord: Vec<String> = Vec::new();
    for part in s.split('+') {
        let trimmed = part.trim();
        let lower = trimmed.to_lowercase();
        let mapped = match lower.as_str() {
            "cmd" | "command" | "ctrl" | "control" => "Control".to_string(),
            "shift" => "Shift".to_string(),
            "option" | "alt" => "Alt".to_string(),
            "super" | "meta" | "win" => "Super".to_string(),
            _ => trimmed.to_uppercase(),
        };
        chord.push(mapped);
    }
    vec![chord]
}

/// Per-tray live handle — the ksni::Handle drives DBus updates, and
/// the shared state is what the impl reads.
struct TrayHandle {
    state: Arc<Mutex<TrayState>>,
    ksni_handle: Handle<PerryTray>,
}

/// Process-wide tray registry. Mutex (not thread_local) because tray
/// creation runs on a tokio worker; subsequent set_icon / attach_menu
/// calls run on the GTK main thread.
fn trays() -> &'static Mutex<Vec<Option<TrayHandle>>> {
    static TRAYS: OnceLock<Mutex<Vec<Option<TrayHandle>>>> = OnceLock::new();
    TRAYS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Dedicated tokio runtime for the tray's KSNI service — re-used
/// across all tray icons (KSNI services share a single DBus connection
/// via zbus's shared session bus, so one runtime is sufficient).
fn tray_runtime() -> Option<&'static Runtime> {
    static RT: OnceLock<Option<Runtime>> = OnceLock::new();
    RT.get_or_init(|| {
        // Spawn a dedicated thread to host the runtime so its Handle
        // outlives any individual entry-point call.
        match Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("perry-tray")
            .enable_all()
            .build()
        {
            Ok(rt) => Some(rt),
            Err(e) => {
                eprintln!(
                    "[perry] warning: tray icon: failed to start tokio runtime: {} \
                    (#490 — KSNI requires an async runtime)",
                    e
                );
                None
            }
        }
    })
    .as_ref()
}

/// `trayCreate(iconPath)` — start a KSNI service on the background
/// runtime, return a 1-based handle index. Returns 0 on failure.
pub fn create(icon_path_ptr: *const u8) -> i64 {
    let icon_path = str_from_header(icon_path_ptr).to_string();

    let rt = match tray_runtime() {
        Some(r) => r,
        None => return 0,
    };

    // Reserve the handle slot first so `id` is stable.
    let idx = {
        let mut t = trays().lock().expect("tray registry poisoned");
        t.push(None);
        t.len()
    };
    let id = format!("perry-tray-{}-{}", std::process::id(), idx);

    let state = Arc::new(Mutex::new(TrayState {
        id,
        icon_path,
        tooltip: String::new(),
        menu_handle: 0,
        on_click: 0.0,
    }));

    let tray = PerryTray {
        state: state.clone(),
    };

    let handle = match rt.block_on(async move {
        // assume_sni_available routes "no SNI host" to watcher_offline
        // instead of an immediate Err, matching the macOS no-tray-bar
        // graceful fallback.
        tray.assume_sni_available(true).spawn().await
    }) {
        Ok(h) => h,
        Err(e) => {
            eprintln!(
                "[perry] warning: tray icon could not be registered: {} \
                — make sure your desktop supports StatusNotifierItem \
                (KDE / GNOME with AppIndicator extension / XFCE / etc.) (#490)",
                e
            );
            // Roll back the slot reservation.
            let mut t = trays().lock().expect("tray registry poisoned");
            if t.len() == idx {
                t.pop();
            }
            return 0;
        }
    };

    let mut t = trays().lock().expect("tray registry poisoned");
    t[idx - 1] = Some(TrayHandle {
        state,
        ksni_handle: handle,
    });
    idx as i64
}

/// Trigger a property refresh — KSNI broadcasts changed DBus properties
/// to the system tray host. Cheap; safe to call from the GTK main thread.
fn refresh(handle: &Handle<PerryTray>) {
    if let Some(rt) = tray_runtime() {
        let h = handle.clone();
        // Fire-and-forget — we don't need to await the DBus round-trip.
        rt.spawn(async move {
            let _ = h.update(|_| ()).await;
        });
    }
}

fn with_tray<F>(handle: i64, f: F)
where
    F: FnOnce(&TrayHandle),
{
    let idx = (handle - 1) as usize;
    let t = trays().lock().expect("tray registry poisoned");
    if let Some(Some(tray)) = t.get(idx) {
        f(tray);
    }
}

pub fn set_icon(handle: i64, icon_path_ptr: *const u8) {
    let path = str_from_header(icon_path_ptr).to_string();
    if path.is_empty() {
        return;
    }
    with_tray(handle, |tray| {
        if let Ok(mut s) = tray.state.lock() {
            s.icon_path = path.clone();
        }
        refresh(&tray.ksni_handle);
    });
}

pub fn set_tooltip(handle: i64, tooltip_ptr: *const u8) {
    let tooltip = str_from_header(tooltip_ptr).to_string();
    with_tray(handle, |tray| {
        if let Ok(mut s) = tray.state.lock() {
            s.tooltip = tooltip.clone();
        }
        refresh(&tray.ksni_handle);
    });
}

pub fn attach_menu(tray_handle: i64, menu_handle: i64) {
    with_tray(tray_handle, |tray| {
        if let Ok(mut s) = tray.state.lock() {
            s.menu_handle = menu_handle;
        }
        refresh(&tray.ksni_handle);
    });
}

pub fn on_click(tray_handle: i64, callback: f64) {
    with_tray(tray_handle, |tray| {
        if let Ok(mut s) = tray.state.lock() {
            s.on_click = callback;
        }
        // No DBus refresh needed — `activate` callback is internal.
    });
}

pub fn destroy(handle: i64) {
    let idx = (handle - 1) as usize;
    let removed = {
        let mut t = trays().lock().expect("tray registry poisoned");
        if idx < t.len() {
            t[idx].take()
        } else {
            None
        }
    };
    if let Some(tray) = removed {
        // ksni::Handle::shutdown returns an awaiter; fire-and-forget on
        // the tray runtime. The slot is left as None so subsequent
        // index-based operations on this handle silently no-op (matches
        // the macOS pattern in tray.rs).
        if let Some(rt) = tray_runtime() {
            let h = tray.ksni_handle;
            rt.spawn(async move {
                let _ = h.shutdown().await;
            });
        }
    }
}
