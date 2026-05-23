//! HTTP server for geisterhand.
//! Routes: /widgets, /click/:handle, /type/:handle, /slide/:handle,
//! /select/:handle, /toggle/:handle, /state/:handle, /key,
//! /scroll/:handle, /chaos/start, /chaos/stop, /chaos/status

use tiny_http::{Header, Method, Response, Server};

extern "C" {
    fn perry_geisterhand_get_registry_json(out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_free_string(ptr: *mut u8, len: usize);
    fn perry_geisterhand_get_closure(handle: i64, callback_kind: u8) -> f64;
    fn perry_geisterhand_queue_action(closure_f64: f64);
    fn perry_geisterhand_queue_action1(closure_f64: f64, arg: f64);
    fn perry_geisterhand_queue_state_set(handle: i64, value: f64);
    fn perry_geisterhand_request_screenshot(out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_find_by_shortcut(shortcut_ptr: *const u8, shortcut_len: usize) -> f64;
    fn perry_geisterhand_queue_scroll(handle: i64, x: f64, y: f64);
    fn perry_geisterhand_queue_set_text(handle: i64, text_ptr: *const u8, text_len: usize);
    fn perry_geisterhand_request_value(handle: i64, out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_request_tree(out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_queue_apply_style(
        handle: i64,
        prop_id: u32,
        a0: f64,
        a1: f64,
        a2: f64,
        a3: f64,
    );
}

// Callback kind constants (must match perry-runtime/src/geisterhand_registry.rs)
const CB_ON_CLICK: u8 = 0;
const CB_ON_CHANGE: u8 = 1;
const CB_ON_SUBMIT: u8 = 2;
const CB_ON_HOVER: u8 = 3;
const CB_ON_DOUBLE_CLICK: u8 = 4;

// Style prop-id namespace — must stay in lockstep with
// `perry-runtime/src/geisterhand_registry.rs::STYLE_*` and the
// per-platform `apply_style` dispatcher (`perry-ui-macos/src/
// geisterhand_style.rs`). The inspector UI sends camelCase prop names
// over the wire; the server resolves them via `style_prop_id`.
const STYLE_BACKGROUND_COLOR: u32 = 1;
const STYLE_COLOR: u32 = 2;
const STYLE_BORDER_COLOR: u32 = 3;
const STYLE_BORDER_WIDTH: u32 = 4;
const STYLE_BORDER_RADIUS: u32 = 5;
const STYLE_OPACITY: u32 = 6;
const STYLE_PADDING_UNIFORM: u32 = 7;
const STYLE_HIDDEN: u32 = 8;
const STYLE_ENABLED: u32 = 9;

fn style_prop_id(name: &str) -> Option<u32> {
    match name {
        "backgroundColor" => Some(STYLE_BACKGROUND_COLOR),
        "color" => Some(STYLE_COLOR),
        "borderColor" => Some(STYLE_BORDER_COLOR),
        "borderWidth" => Some(STYLE_BORDER_WIDTH),
        "borderRadius" => Some(STYLE_BORDER_RADIUS),
        "opacity" => Some(STYLE_OPACITY),
        "padding" => Some(STYLE_PADDING_UNIFORM),
        "hidden" => Some(STYLE_HIDDEN),
        "enabled" => Some(STYLE_ENABLED),
        _ => None,
    }
}

fn is_color_prop(prop_id: u32) -> bool {
    matches!(
        prop_id,
        STYLE_BACKGROUND_COLOR | STYLE_COLOR | STYLE_BORDER_COLOR
    )
}
fn is_bool_prop(prop_id: u32) -> bool {
    matches!(prop_id, STYLE_HIDDEN | STYLE_ENABLED)
}

/// Parse a CSS-style color string into `(r, g, b, a)` floats in `[0, 1]`.
/// Supports the same forms as the codegen-side `parse_color_string`
/// helper (`crates/perry-codegen/src/lower_call.rs`) so live edits and
/// compile-time inline styles produce identical pixels. Returns `None`
/// for unrecognized input; callers drop the edit silently rather than
/// rendering a magenta sentinel for typos.
fn parse_color_string(s: &str) -> Option<(f64, f64, f64, f64)> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let clean: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        let to_f = |b: u8| (b as f64) / 255.0;
        return match clean.len() {
            3 => {
                let n = u32::from_str_radix(&clean, 16).ok()?;
                let r = ((n >> 8) & 0xF) as u8;
                let g = ((n >> 4) & 0xF) as u8;
                let b = (n & 0xF) as u8;
                Some((to_f(r * 17), to_f(g * 17), to_f(b * 17), 1.0))
            }
            4 => {
                let n = u32::from_str_radix(&clean, 16).ok()?;
                let r = ((n >> 12) & 0xF) as u8;
                let g = ((n >> 8) & 0xF) as u8;
                let b = ((n >> 4) & 0xF) as u8;
                let a = (n & 0xF) as u8;
                Some((to_f(r * 17), to_f(g * 17), to_f(b * 17), to_f(a * 17)))
            }
            6 => {
                let n = u32::from_str_radix(&clean, 16).ok()?;
                Some((
                    to_f(((n >> 16) & 0xFF) as u8),
                    to_f(((n >> 8) & 0xFF) as u8),
                    to_f((n & 0xFF) as u8),
                    1.0,
                ))
            }
            8 => {
                let n = u32::from_str_radix(&clean, 16).ok()?;
                Some((
                    to_f(((n >> 24) & 0xFF) as u8),
                    to_f(((n >> 16) & 0xFF) as u8),
                    to_f(((n >> 8) & 0xFF) as u8),
                    to_f((n & 0xFF) as u8),
                ))
            }
            _ => None,
        };
    }
    match s.to_lowercase().as_str() {
        "white" => Some((1.0, 1.0, 1.0, 1.0)),
        "black" => Some((0.0, 0.0, 0.0, 1.0)),
        "red" => Some((1.0, 0.0, 0.0, 1.0)),
        "green" => Some((0.0, 0.5, 0.0, 1.0)),
        "blue" => Some((0.0, 0.0, 1.0, 1.0)),
        "yellow" => Some((1.0, 1.0, 0.0, 1.0)),
        "cyan" => Some((0.0, 1.0, 1.0, 1.0)),
        "magenta" => Some((1.0, 0.0, 1.0, 1.0)),
        "gray" | "grey" => Some((0.502, 0.502, 0.502, 1.0)),
        "transparent" => Some((0.0, 0.0, 0.0, 0.0)),
        _ => None,
    }
}

/// Inspector UI — single-page HTML+JS app embedded at compile time. Served
/// at `GET /` so users can open `http://localhost:7676/` in a browser
/// and see a live tree view of their app's widgets, with per-widget
/// metadata and a button to fire onClick. (Phase D step 1.) Uses only
/// the existing JSON endpoints (`/widgets?tree=true`, `/value/:h`,
/// `/click/:h`) — no new server-side surface needed for the read-only
/// view. Live-edit (style mutation) lands in step 2 once the
/// `/style/:h` endpoint is wired.
const INSPECTOR_HTML: &str = include_str!("inspector_ui/index.html");

fn json_header() -> Header {
    Header::from_bytes("Content-Type", "application/json").unwrap()
}

fn html_header() -> Header {
    Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap()
}

fn cors_header() -> Header {
    Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap()
}

fn ok_json(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.as_bytes().to_vec())
        .with_header(json_header())
        .with_header(cors_header())
}

fn error_json(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(r#"{{"error":"{}"}}"#, msg);
    Response::from_data(body.into_bytes())
        .with_status_code(status)
        .with_header(json_header())
        .with_header(cors_header())
}

/// Parse handle from URL path segment (e.g., "/click/3" → 3)
fn parse_handle(path: &str, prefix: &str) -> Option<i64> {
    let rest = path.strip_prefix(prefix)?;
    rest.parse::<i64>().ok()
}

/// Parse a query parameter value from a URL (e.g., "/widgets?label=Save" → Some("Save"))
fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split('?').nth(1)?;
    let needle = format!("{}=", key);
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix(&needle) {
            return Some(val);
        }
    }
    None
}

/// Map widget type name to code
fn widget_type_from_name(name: &str) -> Option<u8> {
    match name {
        "button" => Some(0),
        "textfield" | "text_field" => Some(1),
        "slider" => Some(2),
        "toggle" => Some(3),
        "picker" => Some(4),
        "menu" => Some(5),
        "shortcut" => Some(6),
        "table" => Some(7),
        "scrollview" | "scroll_view" => Some(8),
        _ => name.parse::<u8>().ok(),
    }
}

/// Read request body as string
fn read_body(request: &mut tiny_http::Request) -> String {
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    body
}

pub fn run_server(port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[geisterhand] failed to start: {}", e);
            return;
        }
    };

    for mut request in server.incoming_requests() {
        let full_url = request.url().to_string();
        let path = full_url.split('?').next().unwrap_or(&full_url);
        let method = request.method().clone();

        // Handle CORS preflight
        if matches!(method, Method::Options) {
            let resp = Response::from_data(Vec::<u8>::new())
                .with_header(cors_header())
                .with_header(
                    Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
                        .unwrap(),
                )
                .with_header(
                    Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap(),
                );
            let _ = request.respond(resp);
            continue;
        }

        let response = match (method, path) {
            // GET / — inspector UI (Phase D step 1). Self-contained HTML+JS
            // page; uses the JSON endpoints below to render a live tree.
            (Method::Get, "/") | (Method::Get, "/inspector") | (Method::Get, "/index.html") => {
                Response::from_data(INSPECTOR_HTML.as_bytes().to_vec())
                    .with_header(html_header())
                    .with_header(cors_header())
            }

            // GET /widgets — list all registered widgets.
            // Supports ?label=, ?type= filters and ?tree=true for visibility/frame data.
            (Method::Get, "/widgets") if query_param(&full_url, "tree") == Some("true") => {
                let mut len: usize = 0;
                let ptr = unsafe { perry_geisterhand_request_tree(&mut len) };
                if !ptr.is_null() && len > 0 {
                    let tree_json = unsafe {
                        String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
                    };
                    unsafe {
                        perry_geisterhand_free_string(ptr, len);
                    }
                    ok_json(&tree_json)
                } else {
                    ok_json("[]")
                }
            }
            (Method::Get, "/widgets") => {
                let mut len: usize = 0;
                let ptr = unsafe { perry_geisterhand_get_registry_json(&mut len) };
                let json = if !ptr.is_null() && len > 0 {
                    let s = unsafe {
                        String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
                    };
                    unsafe {
                        perry_geisterhand_free_string(ptr, len);
                    }
                    s
                } else {
                    "[]".to_string()
                };

                let label_filter = query_param(&full_url, "label").map(|s| s.to_lowercase());
                let type_filter = query_param(&full_url, "type").and_then(widget_type_from_name);
                let body = if label_filter.is_none() && type_filter.is_none() {
                    json
                } else {
                    serde_json::from_str::<Vec<serde_json::Value>>(&json)
                        .map(|arr| {
                            let kept: Vec<&serde_json::Value> = arr
                                .iter()
                                .filter(|w| {
                                    if let Some(ref needle) = label_filter {
                                        let hit = w
                                            .get("label")
                                            .and_then(|l| l.as_str())
                                            .map(|l| l.to_lowercase().contains(needle))
                                            .unwrap_or(false);
                                        if !hit {
                                            return false;
                                        }
                                    }
                                    if let Some(wt) = type_filter {
                                        let hit = w
                                            .get("widget_type")
                                            .and_then(|t| t.as_u64())
                                            .map(|v| v == wt as u64)
                                            .unwrap_or(false);
                                        if !hit {
                                            return false;
                                        }
                                    }
                                    true
                                })
                                .collect();
                            serde_json::to_string(&kept).unwrap_or_else(|_| "[]".to_string())
                        })
                        .unwrap_or(json)
                };
                ok_json(&body)
            }

            // POST /click/:handle — fire onClick
            (Method::Post, p) if p.starts_with("/click/") => match parse_handle(p, "/click/") {
                Some(handle) => {
                    let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CLICK) };
                    if closure != 0.0 {
                        unsafe {
                            perry_geisterhand_queue_action(closure);
                        }
                        ok_json(r#"{"ok":true}"#)
                    } else {
                        error_json(404, "no onClick callback for this handle")
                    }
                }
                None => error_json(400, "invalid handle"),
            },

            // POST /type/:handle — set textfield text + fire onChange
            (Method::Post, p) if p.starts_with("/type/") => {
                match parse_handle(p, "/type/") {
                    Some(handle) => {
                        let body = read_body(&mut request);
                        let text = match serde_json::from_str::<serde_json::Value>(&body) {
                            Ok(v) => v
                                .get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string(),
                            Err(_) => body.clone(),
                        };
                        // Use SendMessageW directly from this thread (thread-safe Win32 call)
                        // to set the Edit control text, bypassing the action queue.
                        // HWND lookup uses perry-ui-geisterhand's own static (single copy,
                        // avoids the dual perry-runtime static instance issue).
                        let hwnd_val = crate::perry_geisterhand_lookup_hwnd(handle);
                        if hwnd_val != 0 {
                            #[cfg(target_os = "windows")]
                            {
                                // Use raw Win32 FFI — no windows crate dependency needed
                                extern "system" {
                                    fn SendMessageW(
                                        hwnd: usize,
                                        msg: u32,
                                        wparam: usize,
                                        lparam: isize,
                                    ) -> isize;
                                }
                                const WM_SETTEXT: u32 = 0x000C;
                                let wide: Vec<u16> =
                                    text.encode_utf16().chain(std::iter::once(0)).collect();
                                unsafe {
                                    SendMessageW(hwnd_val, WM_SETTEXT, 0, wide.as_ptr() as isize);
                                }
                            }
                            // Also fire onChange callback via the action queue
                            let closure =
                                unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                            if closure != 0.0 {
                                extern "C" {
                                    fn js_string_from_bytes(ptr: *const u8, len: usize) -> *mut u8;
                                    fn js_nanbox_string(ptr: i64) -> f64;
                                }
                                let text_bytes = text.as_bytes();
                                let str_ptr = unsafe {
                                    js_string_from_bytes(text_bytes.as_ptr(), text_bytes.len())
                                };
                                let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                                unsafe {
                                    perry_geisterhand_queue_action1(closure, nanboxed);
                                }
                            }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            // Non-Windows: queue text set via the action queue
                            let text_bytes = text.as_bytes();
                            unsafe {
                                perry_geisterhand_queue_set_text(
                                    handle,
                                    text_bytes.as_ptr(),
                                    text_bytes.len(),
                                );
                            }
                            ok_json(r#"{"ok":true}"#)
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /slide/:handle — set slider value + fire onChange
            (Method::Post, p) if p.starts_with("/slide/") => match parse_handle(p, "/slide/") {
                Some(handle) => {
                    let body = read_body(&mut request);
                    let value = match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) => v.get("value").and_then(|v| v.as_f64()).unwrap_or(0.5),
                        Err(_) => 0.5,
                    };
                    let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                    if closure != 0.0 {
                        unsafe {
                            perry_geisterhand_queue_action1(closure, value);
                        }
                        ok_json(r#"{"ok":true}"#)
                    } else {
                        error_json(404, "no onChange callback for this handle")
                    }
                }
                None => error_json(400, "invalid handle"),
            },

            // POST /select/:handle — set Picker selected index + fire onChange.
            // #1488: Picker registers as widget kind 4 (`callback_kind=1` =
            // onChange) but had no automation route. Mirrors /slide — the
            // index is queued as the onChange arg, and the JS-side state
            // setter (`onChange={(idx) => setSelected(idx)}` pattern) drives
            // the re-render. Body shape: `{"index": N}`. (Label-string lookup
            // suggested in the issue is a later step — see the issue thread.)
            (Method::Post, p) if p.starts_with("/select/") => match parse_handle(p, "/select/") {
                Some(handle) => {
                    let body = read_body(&mut request);
                    let index = match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) => v.get("index").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        Err(_) => 0.0,
                    };
                    let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                    if closure != 0.0 {
                        unsafe {
                            perry_geisterhand_queue_action1(closure, index);
                        }
                        ok_json(r#"{"ok":true}"#)
                    } else {
                        error_json(404, "no onChange callback for this handle")
                    }
                }
                None => error_json(400, "invalid handle"),
            },

            // POST /toggle/:handle — toggle + fire onChange
            (Method::Post, p) if p.starts_with("/toggle/") => {
                match parse_handle(p, "/toggle/") {
                    Some(handle) => {
                        let closure =
                            unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                        if closure != 0.0 {
                            // Toggle with TAG_TRUE (0x7FFC_0000_0000_0004)
                            let tag_true = f64::from_bits(0x7FFC_0000_0000_0004u64);
                            unsafe {
                                perry_geisterhand_queue_action1(closure, tag_true);
                            }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onChange callback for this handle")
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /state/:handle — set state value
            (Method::Post, p) if p.starts_with("/state/") => match parse_handle(p, "/state/") {
                Some(handle) => {
                    let body = read_body(&mut request);
                    let value = match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) => v.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        Err(_) => 0.0,
                    };
                    unsafe {
                        perry_geisterhand_queue_state_set(handle, value);
                    }
                    ok_json(r#"{"ok":true}"#)
                }
                None => error_json(400, "invalid handle"),
            },

            // POST /style/:handle — apply one or more StyleProps to a widget
            // (issue #185 Phase D step 2). Body shape: a JSON object whose
            // keys are camelCase prop names (`backgroundColor`, `color`,
            // `borderColor`, `borderWidth`, `borderRadius`, `opacity`,
            // `padding`, `hidden`, `enabled`) and values match each prop's
            // expected type — strings (CSS color), numbers (scalar), or
            // booleans. Unknown keys are skipped silently. Each accepted
            // prop is queued as a separate `ApplyStyle` PendingAction;
            // the main-thread pump drains and dispatches them via the
            // platform-registered `apply_style` function pointer (see
            // `perry-ui-macos/src/geisterhand_style.rs`).
            (Method::Post, p) if p.starts_with("/style/") => match parse_handle(p, "/style/") {
                Some(handle) => {
                    let body = read_body(&mut request);
                    match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(parsed) => match parsed.as_object() {
                            Some(obj) => {
                                let mut applied: Vec<String> = Vec::new();
                                for (key, value) in obj {
                                    let prop_id = match style_prop_id(key) {
                                        Some(id) => id,
                                        None => continue,
                                    };
                                    if is_color_prop(prop_id) {
                                        let color = if let Some(s) = value.as_str() {
                                            parse_color_string(s)
                                        } else {
                                            value.as_object().map(|o| {
                                                (
                                                    o.get("r")
                                                        .and_then(|v| v.as_f64())
                                                        .unwrap_or(0.0),
                                                    o.get("g")
                                                        .and_then(|v| v.as_f64())
                                                        .unwrap_or(0.0),
                                                    o.get("b")
                                                        .and_then(|v| v.as_f64())
                                                        .unwrap_or(0.0),
                                                    o.get("a")
                                                        .and_then(|v| v.as_f64())
                                                        .unwrap_or(1.0),
                                                )
                                            })
                                        };
                                        if let Some((r, g, b, a)) = color {
                                            unsafe {
                                                perry_geisterhand_queue_apply_style(
                                                    handle, prop_id, r, g, b, a,
                                                );
                                            }
                                            applied.push(key.clone());
                                        }
                                    } else if is_bool_prop(prop_id) {
                                        let v = value.as_bool().unwrap_or(false);
                                        let a0 = if v { 1.0 } else { 0.0 };
                                        unsafe {
                                            perry_geisterhand_queue_apply_style(
                                                handle, prop_id, a0, 0.0, 0.0, 0.0,
                                            );
                                        }
                                        applied.push(key.clone());
                                    } else if let Some(v) = value.as_f64() {
                                        unsafe {
                                            perry_geisterhand_queue_apply_style(
                                                handle, prop_id, v, 0.0, 0.0, 0.0,
                                            );
                                        }
                                        applied.push(key.clone());
                                    }
                                }
                                let body = format!(
                                    r#"{{"ok":true,"applied":{}}}"#,
                                    serde_json::to_string(&applied).unwrap_or_else(|_| "[]".into())
                                );
                                ok_json(&body)
                            }
                            None => error_json(400, "expected JSON object"),
                        },
                        Err(_) => error_json(400, "invalid JSON"),
                    }
                }
                None => error_json(400, "invalid handle"),
            },

            // POST /hover/:handle — fire onHover
            (Method::Post, p) if p.starts_with("/hover/") => match parse_handle(p, "/hover/") {
                Some(handle) => {
                    let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_HOVER) };
                    if closure != 0.0 {
                        unsafe {
                            perry_geisterhand_queue_action(closure);
                        }
                        ok_json(r#"{"ok":true}"#)
                    } else {
                        error_json(404, "no onHover callback for this handle")
                    }
                }
                None => error_json(400, "invalid handle"),
            },

            // POST /doubleclick/:handle — fire onDoubleClick
            (Method::Post, p) if p.starts_with("/doubleclick/") => {
                match parse_handle(p, "/doubleclick/") {
                    Some(handle) => {
                        let closure =
                            unsafe { perry_geisterhand_get_closure(handle, CB_ON_DOUBLE_CLICK) };
                        if closure != 0.0 {
                            unsafe {
                                perry_geisterhand_queue_action(closure);
                            }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onDoubleClick callback for this handle")
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /chaos/start — start chaos mode
            (Method::Post, "/chaos/start") => {
                let body = read_body(&mut request);
                let interval_ms = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => v.get("interval_ms").and_then(|v| v.as_u64()).unwrap_or(100),
                    Err(_) => 100,
                };
                let seed = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => v.get("seed").and_then(|v| v.as_u64()),
                    Err(_) => None,
                };
                crate::chaos::start(interval_ms, seed);
                ok_json(r#"{"ok":true,"chaos":"started"}"#)
            }

            // POST /chaos/stop — stop chaos mode
            (Method::Post, "/chaos/stop") => {
                crate::chaos::stop();
                ok_json(r#"{"ok":true,"chaos":"stopped"}"#)
            }

            // GET /chaos/status — chaos mode stats
            (Method::Get, "/chaos/status") => {
                let status = crate::chaos::status();
                ok_json(&status)
            }

            // GET /health
            (Method::Get, "/health") => ok_json(r#"{"status":"ok"}"#),

            // GET /screenshot — capture the app window as PNG
            (Method::Get, "/screenshot") => {
                let mut len: usize = 0;
                let ptr = unsafe { perry_geisterhand_request_screenshot(&mut len) };
                if !ptr.is_null() && len > 0 {
                    let data = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };
                    unsafe {
                        perry_geisterhand_free_string(ptr, len);
                    }
                    Response::from_data(data)
                        .with_header(Header::from_bytes("Content-Type", "image/png").unwrap())
                        .with_header(cors_header())
                } else {
                    error_json(500, "screenshot capture failed or timed out")
                }
            }

            // POST /key — fire a keyboard shortcut by matching registered menu shortcuts
            (Method::Post, "/key") => {
                let body = read_body(&mut request);
                let shortcut = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => v
                        .get("shortcut")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    Err(_) => body.trim().to_string(),
                };
                if shortcut.is_empty() {
                    error_json(400, "missing shortcut field")
                } else {
                    let closure = unsafe {
                        perry_geisterhand_find_by_shortcut(shortcut.as_ptr(), shortcut.len())
                    };
                    if closure != 0.0 {
                        unsafe {
                            perry_geisterhand_queue_action(closure);
                        }
                        ok_json(r#"{"ok":true}"#)
                    } else {
                        error_json(404, "no registered shortcut matches")
                    }
                }
            }

            // POST /scroll/:handle — scroll a scrollview
            (Method::Post, p) if p.starts_with("/scroll/") => match parse_handle(p, "/scroll/") {
                Some(handle) => {
                    let body = read_body(&mut request);
                    let (x, y) = match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) => (
                            v.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            v.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        ),
                        Err(_) => (0.0, 0.0),
                    };
                    unsafe {
                        perry_geisterhand_queue_scroll(handle, x, y);
                    }
                    ok_json(r#"{"ok":true}"#)
                }
                None => error_json(400, "invalid handle"),
            },

            // POST /wait — wait for a widget with matching label to appear
            (Method::Post, "/wait") => {
                let body = read_body(&mut request);
                let (label, timeout_ms) = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => (
                        v.get("label")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        v.get("timeout").and_then(|t| t.as_u64()).unwrap_or(5000),
                    ),
                    Err(_) => (String::new(), 5000),
                };
                if label.is_empty() {
                    error_json(400, "missing label field")
                } else {
                    let needle = label.to_lowercase();
                    let start = std::time::Instant::now();
                    let timeout = std::time::Duration::from_millis(timeout_ms);
                    loop {
                        let mut len: usize = 0;
                        let ptr = unsafe { perry_geisterhand_get_registry_json(&mut len) };
                        if !ptr.is_null() && len > 0 {
                            let json = unsafe {
                                String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))
                                    .into_owned()
                            };
                            unsafe {
                                perry_geisterhand_free_string(ptr, len);
                            }
                            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&json) {
                                if let Some(w) = arr.iter().find(|w| {
                                    w.get("label")
                                        .and_then(|l| l.as_str())
                                        .map(|l| l.to_lowercase().contains(&needle))
                                        .unwrap_or(false)
                                }) {
                                    break ok_json(
                                        &serde_json::to_string(w)
                                            .unwrap_or_else(|_| "{}".to_string()),
                                    );
                                }
                            }
                        }
                        if start.elapsed() >= timeout {
                            break error_json(408, "timeout waiting for widget");
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }

            // GET /value/:handle — read current widget value
            //
            // Issue #640: distinguish "widget not found / not readable"
            // (ptr.is_null() → `value: null`) from "widget found with
            // empty stringValue" (ptr non-null + len=0 → `value: ""`).
            // Pre-fix the truthiness check `len > 0` collapsed the two
            // cases, so a TextField that the user had just typed `""`
            // into (or that was empty after a route swap) was
            // indistinguishable from a torn-down widget — confounding
            // bug-triage of issues like the multi-route TextField
            // probe in the issue's reproducer.
            (Method::Get, p) if p.starts_with("/value/") => match parse_handle(p, "/value/") {
                Some(handle) => {
                    let mut len: usize = 0;
                    let ptr = unsafe { perry_geisterhand_request_value(handle, &mut len) };
                    if !ptr.is_null() {
                        let val = unsafe {
                            String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len))
                                .into_owned()
                        };
                        unsafe {
                            perry_geisterhand_free_string(ptr, len);
                        }
                        ok_json(&format!(
                            r#"{{"handle":{},"value":"{}"}}"#,
                            handle,
                            val.replace('\\', "\\\\").replace('"', "\\\"")
                        ))
                    } else {
                        ok_json(&format!(r#"{{"handle":{},"value":null}}"#, handle))
                    }
                }
                None => error_json(400, "invalid handle"),
            },

            _ => error_json(404, "not found"),
        };

        let _ = request.respond(response);
    }
}
