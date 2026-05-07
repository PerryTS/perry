//! GTK4 MapView (issue #517) — backed by libshumate, GNOME's GTK4-native
//! vector-tile map widget. OpenStreetMap tiles by default; no API key.
//!
//! API parity with the Apple `widgets/map_view.rs` impls:
//!  - `create(width, height)` → `ShumateSimpleMap` upcast to `gtk::Widget`
//!  - `set_region(lat, lon, lat_span, lon_span)` → derive a tile-zoom
//!    level from the requested span (z = log2(360 / lat_span)) and seed
//!    the viewport center
//!  - `add_pin(lat, lon, title)` → push a `Marker` into a lazily-allocated
//!    `MarkerLayer` overlay
//!  - `clear_pins()` → drop the marker layer (and recreate next add_pin)
//!  - `set_map_type(style)` → 0=street (default OSM), 1/2 keep OSM since
//!    libshumate's standard `MapSourceRegistry` only ships one source.
//!    Apps wanting satellite/hybrid layers can register their own
//!    MapSource via libshumate's API later.

use gtk4::prelude::*;
use libshumate::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Per-map marker layer; lazily created on first `add_pin`, dropped on
    /// `clear_pins`. Keyed by widget handle.
    static MARKER_LAYERS: RefCell<HashMap<i64, libshumate::MarkerLayer>> =
        RefCell::new(HashMap::new());
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

fn simple_map_for(handle: i64) -> Option<libshumate::SimpleMap> {
    super::get_widget(handle).and_then(|w| w.downcast::<libshumate::SimpleMap>().ok())
}

fn ensure_marker_layer(
    handle: i64,
    map: &libshumate::SimpleMap,
) -> libshumate::MarkerLayer {
    if let Some(existing) = MARKER_LAYERS.with(|m| m.borrow().get(&handle).cloned()) {
        return existing;
    }
    let viewport = map.viewport().expect("ShumateSimpleMap has a viewport");
    let layer = libshumate::MarkerLayer::new(&viewport);
    map.add_overlay_layer(&layer);
    MARKER_LAYERS.with(|m| m.borrow_mut().insert(handle, layer.clone()));
    layer
}

pub fn create(width: f64, height: f64) -> i64 {
    crate::app::ensure_gtk_init();

    let map = libshumate::SimpleMap::new();
    let w = width.max(40.0) as i32;
    let h = height.max(40.0) as i32;
    map.set_size_request(w, h);

    // Default OSM tile source — keeps users key-free out of the box.
    let registry = libshumate::MapSourceRegistry::with_defaults();
    if let Some(source) = registry.by_id(libshumate::MAP_SOURCE_OSM_MAPNIK) {
        map.set_map_source(Some(&source));
    }

    super::register_widget(map.upcast())
}

/// Convert MapKit-style (lat, lon, lat_span, lon_span) to a libshumate
/// (latitude, longitude, zoom_level) viewport setup. Tile zoom = log2(360
/// / span_deg); 360° = whole world (zoom 0), 0.01° ≈ city block (zoom 15).
pub fn set_region(handle: i64, lat: f64, lon: f64, lat_span: f64, lon_span: f64) {
    let span = lat_span.max(lon_span).max(0.0001);
    let zoom = (360.0 / span).log2().clamp(0.0, 20.0);
    if let Some(map) = simple_map_for(handle) {
        if let Some(viewport) = map.viewport() {
            viewport.set_zoom_level(zoom);
            viewport.set_latitude(lat);
            viewport.set_longitude(lon);
        }
    }
}

pub fn add_pin(handle: i64, lat: f64, lon: f64, title_ptr: *const u8) {
    if let Some(map) = simple_map_for(handle) {
        let title = str_from_header(title_ptr);
        let layer = ensure_marker_layer(handle, &map);
        let marker = libshumate::Marker::new();
        marker.set_location(lat, lon);
        if !title.is_empty() {
            // libshumate's Marker is just a positioned container; the
            // visible "pin" is whatever child widget we set. A GtkLabel
            // with the title text is the smallest acceptable thing.
            let label = gtk4::Label::new(Some(title));
            marker.set_child(Some(&label));
        }
        layer.add_marker(&marker);
    }
}

pub fn clear_pins(handle: i64) {
    if let Some(map) = simple_map_for(handle) {
        if let Some(layer) = MARKER_LAYERS.with(|m| m.borrow_mut().remove(&handle)) {
            map.remove_overlay_layer(&layer);
        }
    }
}

/// libshumate's default `MapSourceRegistry` only ships the OSM-mapnik
/// source. Honor `style == 0` (standard) and treat 1/2 as no-ops with a
/// log line so the API stays cross-platform-uniform without surprising
/// the user. Apps that need satellite imagery can register their own
/// `MapSource` (e.g. an MBTiles raster) via libshumate's API directly.
pub fn set_map_type(_handle: i64, style: i64) {
    if style != 0 {
        eprintln!(
            "[perry/ui] mapViewSetMapType({}) on GTK4: libshumate's bundled \
             registry only ships the OSM-mapnik source; satellite/hybrid \
             requires a custom MapSource — leaving style at standard.",
            style
        );
    }
}
