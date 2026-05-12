//! macOS Map widget (issue #517).
//!
//! Wraps `MKMapView` from MapKit. The framework is linked at the
//! linker step (see `crates/perry/src/commands/compile/link.rs::1442`)
//! so we instantiate via raw `objc_msgSend` against the `MKMapView`
//! class without adding an `objc2-map-kit` dependency.
//!
//! Out of scope this iteration: user-location tracking, custom
//! annotation views (we only support the default red pin via
//! `MKPointAnnotation`), polylines / polygons, route directions,
//! gesture-driven region-change callbacks, satellite/hybrid map type
//! switching API. Filed as #517 follow-ups.

use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::NSView;
use objc2_foundation::NSString;

#[repr(C)]
#[derive(Copy, Clone)]
struct CLLocationCoordinate2D {
    latitude: f64,
    longitude: f64,
}

unsafe impl Encode for CLLocationCoordinate2D {
    const ENCODING: Encoding = Encoding::Struct(
        "CLLocationCoordinate2D",
        &[Encoding::Double, Encoding::Double],
    );
}

#[repr(C)]
#[derive(Copy, Clone)]
struct MKCoordinateSpan {
    latitude_delta: f64,
    longitude_delta: f64,
}

unsafe impl Encode for MKCoordinateSpan {
    const ENCODING: Encoding =
        Encoding::Struct("MKCoordinateSpan", &[Encoding::Double, Encoding::Double]);
}

#[repr(C)]
#[derive(Copy, Clone)]
struct MKCoordinateRegion {
    center: CLLocationCoordinate2D,
    span: MKCoordinateSpan,
}

unsafe impl Encode for MKCoordinateRegion {
    const ENCODING: Encoding = Encoding::Struct(
        "MKCoordinateRegion",
        &[CLLocationCoordinate2D::ENCODING, MKCoordinateSpan::ENCODING],
    );
}

/// Create an `MKMapView` of the requested size. Returns 1-based widget
/// handle, or 0 if MapKit isn't available at runtime.
pub fn create(width: f64, height: f64) -> i64 {
    unsafe {
        let Some(cls) = AnyClass::get(c"MKMapView") else {
            // MapKit not loaded — return 0 so the caller can detect
            // and fall back to a placeholder view.
            return 0;
        };
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(width.max(40.0), height.max(40.0)),
        );
        let raw: *mut AnyObject = msg_send![alloc, initWithFrame: frame];
        let map: Retained<AnyObject> = match Retained::from_raw(raw) {
            Some(r) => r,
            None => return 0,
        };
        // Show user-location button + zoom controls as available
        // (these are no-ops on older targets).
        let _: () = msg_send![&*map, setShowsZoomControls: true];
        let _: () = msg_send![&*map, setShowsCompass: true];
        let view: Retained<NSView> = Retained::cast_unchecked(map);
        super::register_widget(view)
    }
}

/// Center the map on (lat, lon) with a span (degrees).
pub fn set_region(handle: i64, lat: f64, lon: f64, lat_span: f64, lon_span: f64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    let region = MKCoordinateRegion {
        center: CLLocationCoordinate2D {
            latitude: lat,
            longitude: lon,
        },
        span: MKCoordinateSpan {
            latitude_delta: lat_span.max(0.001),
            longitude_delta: lon_span.max(0.001),
        },
    };
    unsafe {
        let _: () = msg_send![&*view, setRegion: region, animated: true];
    }
}

/// Drop a pin at (lat, lon) with the given title. Uses the default red
/// `MKPointAnnotation` styling — custom annotation views are out of
/// scope this iteration.
pub fn add_pin(handle: i64, lat: f64, lon: f64, title_ptr: *const u8) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    let title = if title_ptr.is_null() {
        String::new()
    } else {
        unsafe {
            let header = title_ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = title_ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
        }
    };
    unsafe {
        let Some(cls) = AnyClass::get(c"MKPointAnnotation") else {
            return;
        };
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let pin: *mut AnyObject = msg_send![alloc, init];
        let coord = CLLocationCoordinate2D {
            latitude: lat,
            longitude: lon,
        };
        let _: () = msg_send![pin, setCoordinate: coord];
        if !title.is_empty() {
            let ns = NSString::from_str(&title);
            let _: () = msg_send![pin, setTitle: &*ns];
        }
        let _: () = msg_send![&*view, addAnnotation: pin];
    }
}

/// Remove every annotation from the map.
pub fn clear_pins(handle: i64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    unsafe {
        let annotations: *mut AnyObject = msg_send![&*view, annotations];
        if annotations.is_null() {
            return;
        }
        let _: () = msg_send![&*view, removeAnnotations: annotations];
    }
}

/// Swap the visual map style. `style` is 0=standard, 1=satellite,
/// 2=hybrid (matches `MKMapType`'s enum order).
pub fn set_map_type(handle: i64, style: i64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    let map_type = match style {
        1 => 1u64,
        2 => 2u64,
        _ => 0u64,
    };
    unsafe {
        let _: () = msg_send![&*view, setMapType: map_type];
    }
}
