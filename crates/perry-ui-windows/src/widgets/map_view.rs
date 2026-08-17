//! Native Windows MapView (#559).
//!
//! The registered Perry widget remains an ordinary child HWND so it participates
//! in the existing Win32 layout tree. A `DesktopWindowXamlSource` attaches a
//! XAML Island to that host and renders `Windows.UI.Xaml.Controls.Maps.MapControl`
//! inside it. The source, map, and native interop interface are retained for the
//! widget lifetime; a host-window subclass keeps the island sized to its parent.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::SS_CENTER;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

#[cfg(target_os = "windows")]
use windows_xaml::core::{Interface, HSTRING};
#[cfg(target_os = "windows")]
use windows_xaml::Devices::Geolocation::{BasicGeoposition, Geopoint};
#[cfg(target_os = "windows")]
use windows_xaml::Win32::System::WinRT::Xaml::IDesktopWindowXamlSourceNative2;
#[cfg(target_os = "windows")]
use windows_xaml::UI::Xaml::Controls::Maps::{MapControl, MapElement, MapIcon, MapStyle};
#[cfg(target_os = "windows")]
use windows_xaml::UI::Xaml::Hosting::{DesktopWindowXamlSource, WindowsXamlManager};

use super::{alloc_control_id, register_widget, WidgetKind};

#[cfg(target_os = "windows")]
const MAP_SUBCLASS_ID: usize = 0x5045_5252_595F_4D41;

struct MapState {
    lat: f64,
    lon: f64,
    lat_span: f64,
    lon_span: f64,
    map_type: i64,
    pin_count: usize,
    init_error: Option<String>,
    #[cfg(target_os = "windows")]
    backend: Option<XamlMapBackend>,
}

#[cfg(target_os = "windows")]
struct XamlMapBackend {
    // Retaining the source owns the island and its content.
    source: DesktopWindowXamlSource,
    native: IDesktopWindowXamlSourceNative2,
    map: MapControl,
    island_hwnd: HWND,
}

#[cfg(target_os = "windows")]
impl Drop for XamlMapBackend {
    fn drop(&mut self) {
        let _ = self.source.Close();
    }
}

thread_local! {
    static MAPS: RefCell<HashMap<i64, MapState>> = RefCell::new(HashMap::new());
    #[cfg(target_os = "windows")]
    static HWND_TO_HANDLE: RefCell<HashMap<isize, i64>> = RefCell::new(HashMap::new());
    // Every manager holds a reference to the per-thread XAML framework. Keep
    // one alive for as long as Perry's UI thread can own MapViews.
    #[cfg(target_os = "windows")]
    static XAML_MANAGER: RefCell<Option<WindowsXamlManager>> = const { RefCell::new(None) };
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

/// Convert a MapKit-style latitude/longitude span to the corresponding map
/// zoom. This matches Perry's GTK4 backend and clamps to MapControl's range.
fn zoom_from_span(lat_span: f64, lon_span: f64) -> f64 {
    if !lat_span.is_finite() || !lon_span.is_finite() {
        return 20.0;
    }
    let span = lat_span.abs().max(lon_span.abs());
    if span <= 0.0 {
        return 20.0;
    }
    (360.0 / span).log2().clamp(1.0, 20.0)
}

#[cfg(target_os = "windows")]
fn xaml_map_style(style: i64) -> MapStyle {
    match style {
        1 => MapStyle::Aerial,
        2 => MapStyle::AerialWithRoads,
        _ => MapStyle::Road,
    }
}

#[cfg(target_os = "windows")]
fn geopoint(lat: f64, lon: f64) -> windows_xaml::core::Result<Geopoint> {
    Geopoint::Create(BasicGeoposition {
        Latitude: lat.clamp(-90.0, 90.0),
        Longitude: lon.clamp(-180.0, 180.0),
        Altitude: 0.0,
    })
}

#[cfg(target_os = "windows")]
fn ensure_xaml_initialized() -> Result<(), String> {
    // XAML Islands require an STA. CoInitializeEx is idempotent for a thread
    // already initialized in the same apartment model.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|err| format!("COM STA initialization failed: {err}"))?;

    XAML_MANAGER.with(|slot| {
        if slot.borrow().is_some() {
            return Ok(());
        }
        let manager = WindowsXamlManager::InitializeForCurrentThread()
            .map_err(|err| format!("XAML initialization failed: {err}"))?;
        *slot.borrow_mut() = Some(manager);
        Ok(())
    })
}

#[cfg(target_os = "windows")]
fn configured_map_token() -> Option<String> {
    // PERRY_MAP_SERVICE_TOKEN is the cross-version name. Keep the explicit
    // Bing alias for existing Windows/UWP deployments.
    std::env::var("PERRY_MAP_SERVICE_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("PERRY_BING_MAPS_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

#[cfg(target_os = "windows")]
fn create_xaml_backend(host: HWND, width: i32, height: i32) -> Result<XamlMapBackend, String> {
    ensure_xaml_initialized()?;

    // Create/attach the source before constructing UIElement-derived content;
    // this is the ordering required by the XAML hosting API.
    let source = DesktopWindowXamlSource::new()
        .map_err(|err| format!("DesktopWindowXamlSource creation failed: {err}"))?;
    let native: IDesktopWindowXamlSourceNative2 = source
        .cast()
        .map_err(|err| format!("XAML Island interop unavailable: {err}"))?;
    let xaml_host = windows_xaml::Win32::Foundation::HWND(host.0 as isize);
    unsafe { native.AttachToWindow(xaml_host) }
        .map_err(|err| format!("XAML Island attachment failed: {err}"))?;

    let island = unsafe { native.WindowHandle() }
        .map_err(|err| format!("XAML Island HWND lookup failed: {err}"))?;
    let island_hwnd = HWND(island.0 as *mut _);

    let map = MapControl::new().map_err(|err| format!("MapControl creation failed: {err}"))?;
    if let Some(token) = configured_map_token() {
        map.SetMapServiceToken(HSTRING::from(token))
            .map_err(|err| format!("MapControl token setup failed: {err}"))?;
    }
    source
        .SetContent(&map)
        .map_err(|err| format!("MapControl hosting failed: {err}"))?;

    unsafe {
        SetWindowPos(
            island_hwnd,
            None,
            0,
            0,
            width.max(1),
            height.max(1),
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
        )
    }
    .map_err(|err| format!("XAML Island sizing failed: {err}"))?;

    Ok(XamlMapBackend {
        source,
        native,
        map,
        island_hwnd,
    })
}

pub fn create(width: f64, height: f64) -> i64 {
    let control_id = alloc_control_id();
    let w = if width > 0.0 { width as i32 } else { 400 };
    let h = if height > 0.0 { height as i32 } else { 300 };

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("[Map - initializing native control]");
        let host = unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(
                    WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0 | SS_CENTER.0,
                ),
                0,
                0,
                w,
                h,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            )
        }
        .unwrap();

        let handle = register_widget(host, WidgetKind::Image, control_id);
        let backend = create_xaml_backend(host, w, h);
        let init_error = backend.as_ref().err().cloned();
        MAPS.with(|maps| {
            maps.borrow_mut().insert(
                handle,
                MapState {
                    lat: 0.0,
                    lon: 0.0,
                    lat_span: 0.0,
                    lon_span: 0.0,
                    map_type: 0,
                    pin_count: 0,
                    init_error,
                    backend: backend.ok(),
                },
            );
        });
        HWND_TO_HANDLE.with(|map| {
            map.borrow_mut().insert(host.0 as isize, handle);
        });
        unsafe {
            let _ = SetWindowSubclass(host, Some(map_host_subclass_proc), MAP_SUBCLASS_ID, 0);
        }
        refresh_placeholder(handle);
        handle
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (w, h);
        let handle = register_widget(0, WidgetKind::Image, control_id);
        MAPS.with(|maps| {
            maps.borrow_mut().insert(
                handle,
                MapState {
                    lat: 0.0,
                    lon: 0.0,
                    lat_span: 0.0,
                    lon_span: 0.0,
                    map_type: 0,
                    pin_count: 0,
                    init_error: Some("native MapControl is only available on Windows".to_string()),
                },
            );
        });
        handle
    }
}

fn refresh_placeholder(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        let display = MAPS.with(|maps| {
            let maps = maps.borrow();
            let state = maps.get(&handle)?;
            if state.backend.is_some() {
                return None;
            }
            if let Some(error) = &state.init_error {
                return Some(format!("[Map unavailable: {error}]"));
            }
            let map_type = match state.map_type {
                1 => "Aerial",
                2 => "Aerial with roads",
                _ => "Road",
            };
            Some(format!(
                "[Map ({map_type}) @ {:.4},{:.4} - span {:.3}x{:.3} - {} pins]",
                state.lat, state.lon, state.lat_span, state.lon_span, state.pin_count
            ))
        });
        if let (Some(text), Some(hwnd)) = (display, super::get_hwnd(handle)) {
            let wide = to_wide(&text);
            unsafe {
                let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

pub fn set_region(handle: i64, lat: f64, lon: f64, lat_span: f64, lon_span: f64) {
    #[cfg(target_os = "windows")]
    let map = MAPS.with(|maps| {
        let mut maps = maps.borrow_mut();
        let state = maps.get_mut(&handle)?;
        state.lat = lat;
        state.lon = lon;
        state.lat_span = lat_span;
        state.lon_span = lon_span;
        state.backend.as_ref().map(|backend| backend.map.clone())
    });
    #[cfg(not(target_os = "windows"))]
    MAPS.with(|maps| {
        if let Some(state) = maps.borrow_mut().get_mut(&handle) {
            state.lat = lat;
            state.lon = lon;
            state.lat_span = lat_span;
            state.lon_span = lon_span;
        }
    });

    #[cfg(target_os = "windows")]
    if let Some(map) = map {
        if let Ok(center) = geopoint(lat, lon) {
            let _ = map.SetCenter(center);
            let _ = map.SetZoomLevel(zoom_from_span(lat_span, lon_span));
        }
    }
    refresh_placeholder(handle);
}

pub fn add_pin(handle: i64, lat: f64, lon: f64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    #[cfg(target_os = "windows")]
    let map = MAPS.with(|maps| {
        let mut maps = maps.borrow_mut();
        let state = maps.get_mut(&handle)?;
        state.pin_count += 1;
        state.backend.as_ref().map(|backend| backend.map.clone())
    });
    #[cfg(not(target_os = "windows"))]
    MAPS.with(|maps| {
        if let Some(state) = maps.borrow_mut().get_mut(&handle) {
            state.pin_count += 1;
        }
    });

    #[cfg(target_os = "windows")]
    if let Some(map) = map {
        if let (Ok(location), Ok(icon), Ok(elements)) =
            (geopoint(lat, lon), MapIcon::new(), map.MapElements())
        {
            let _ = icon.SetLocation(location);
            let _ = icon.SetTitle(HSTRING::from(title));
            let element = MapElement::from(&icon);
            let _ = elements.Append(element);
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (lat, lon, title);
    refresh_placeholder(handle);
}

pub fn clear_pins(handle: i64) {
    #[cfg(target_os = "windows")]
    let map = MAPS.with(|maps| {
        let mut maps = maps.borrow_mut();
        let state = maps.get_mut(&handle)?;
        state.pin_count = 0;
        state.backend.as_ref().map(|backend| backend.map.clone())
    });
    #[cfg(not(target_os = "windows"))]
    MAPS.with(|maps| {
        if let Some(state) = maps.borrow_mut().get_mut(&handle) {
            state.pin_count = 0;
        }
    });

    #[cfg(target_os = "windows")]
    if let Some(map) = map {
        if let Ok(elements) = map.MapElements() {
            let _ = elements.Clear();
        }
    }
    refresh_placeholder(handle);
}

pub fn set_map_type(handle: i64, style: i64) {
    #[cfg(target_os = "windows")]
    let map = MAPS.with(|maps| {
        let mut maps = maps.borrow_mut();
        let state = maps.get_mut(&handle)?;
        state.map_type = style;
        state.backend.as_ref().map(|backend| backend.map.clone())
    });
    #[cfg(not(target_os = "windows"))]
    MAPS.with(|maps| {
        if let Some(state) = maps.borrow_mut().get_mut(&handle) {
            state.map_type = style;
        }
    });

    #[cfg(target_os = "windows")]
    if let Some(map) = map {
        let _ = map.SetStyle(xaml_map_style(style));
    }
    refresh_placeholder(handle);
}

/// Let every live XAML Island inspect a message before the Win32 dispatcher.
/// This is required for keyboard and pointer interaction hosted through
/// `DesktopWindowXamlSource`.
#[cfg(target_os = "windows")]
pub fn pre_translate_message(msg: &MSG) -> bool {
    let sources = MAPS.with(|maps| {
        maps.borrow()
            .values()
            .filter_map(|state| state.backend.as_ref().map(|backend| backend.native.clone()))
            .collect::<Vec<_>>()
    });
    let old_msg: windows_xaml::Win32::UI::WindowsAndMessaging::MSG =
        unsafe { std::mem::transmute_copy(msg) };
    for source in sources {
        let mut handled = windows_xaml::Win32::Foundation::BOOL(0);
        if unsafe { source.PreTranslateMessage(&old_msg, &mut handled) }.is_ok()
            && handled.as_bool()
        {
            return true;
        }
    }
    false
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn map_host_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _refdata: usize,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            let handle = HWND_TO_HANDLE.with(|map| map.borrow().get(&(hwnd.0 as isize)).copied());
            if let Some(handle) = handle {
                let island = MAPS.with(|maps| {
                    maps.borrow()
                        .get(&handle)
                        .and_then(|state| state.backend.as_ref().map(|backend| backend.island_hwnd))
                });
                if let Some(island) = island {
                    let mut rect = RECT::default();
                    if GetClientRect(hwnd, &mut rect).is_ok() {
                        let _ = SetWindowPos(
                            island,
                            None,
                            0,
                            0,
                            (rect.right - rect.left).max(1),
                            (rect.bottom - rect.top).max(1),
                            SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
                        );
                    }
                }
            }
        }
        WM_NCDESTROY => {
            let handle = HWND_TO_HANDLE.with(|map| map.borrow_mut().remove(&(hwnd.0 as isize)));
            if let Some(handle) = handle {
                MAPS.with(|maps| {
                    maps.borrow_mut().remove(&handle);
                });
            }
            let _ = RemoveWindowSubclass(hwnd, Some(map_host_subclass_proc), MAP_SUBCLASS_ID);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::zoom_from_span;

    #[test]
    fn zoom_conversion_matches_cross_platform_map_contract() {
        assert_eq!(zoom_from_span(360.0, 360.0), 1.0);
        assert_eq!(zoom_from_span(0.0, 0.0), 20.0);
        assert!((zoom_from_span(0.05, 0.05) - 12.813_781).abs() < 0.000_01);
        assert_eq!(zoom_from_span(f64::NAN, 10.0), 20.0);
    }
}
