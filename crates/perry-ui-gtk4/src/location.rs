// Location service for Linux — IP-based geolocation via ip-api.com.
//
// Runs the HTTP request on a background thread and dispatches the callback
// on the GTK main thread via glib::MainContext::invoke().
// City-level accuracy — sufficient for weather, maps, etc.

use gtk4::glib;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
}

/// Request the user's location. Calls callback(lat, lon) on success,
/// or callback(NaN, NaN) on failure.
pub fn request_location(callback: f64) {
    std::thread::spawn(move || {
        let result = try_ip_geolocation();

        glib::MainContext::default().invoke(move || {
            unsafe {
                js_run_stdlib_pump();
                js_promise_run_microtasks();
            }
            let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
            match result {
                Some((lat, lon)) => unsafe {
                    js_closure_call2(ptr, lat, lon);
                },
                None => unsafe {
                    js_closure_call2(ptr, f64::NAN, f64::NAN);
                },
            }
        });
    });
}

/// Get location via IP-based geolocation (ip-api.com, plain HTTP).
fn try_ip_geolocation() -> Option<(f64, f64)> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let addr = ("ip-api.com", 80u16).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    stream.write_all(
        b"GET /json/?fields=lat,lon,city HTTP/1.1\r\nHost: ip-api.com\r\nConnection: close\r\n\r\n"
    ).ok()?;

    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;

    // Find JSON body after HTTP headers
    let body = response.split("\r\n\r\n").nth(1)?;
    eprintln!("[location] ip-api response: {}", body.trim());

    let lat = extract_json_f64(body, "lat")?;
    let lon = extract_json_f64(body, "lon")?;
    eprintln!("[location] resolved coordinates: lat={}, lon={}", lat, lon);
    Some((lat, lon))
}

/// Extract a floating-point number from a JSON string by key.
fn extract_json_f64(json: &str, key: &str) -> Option<f64> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| {
            c != '.' && c != '-' && c != '+' && c != 'e' && c != 'E' && !c.is_ascii_digit()
        })
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}
