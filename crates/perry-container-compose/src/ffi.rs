//! FFI exports for Perry TypeScript integration.
//!
//! Each function follows the Perry FFI convention:
//! - String arguments arrive as `*const StringHeader` (Perry runtime layout)
//! - Results are serialised to JSON strings before being handed back to JS

use crate::compose::ComposeEngine;
use std::path::PathBuf;
use std::sync::Arc;

// ──────────────────────────────────────────────────────────────
// Minimal re-implementation of the Perry runtime string types
// ──────────────────────────────────────────────────────────────

#[repr(C)]
pub struct StringHeader {
    pub length: u32,
}

unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).length as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

// ──────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────

fn json_ok(value: &str) -> *const StringHeader {
    let payload = format!("{{\"ok\":true,\"result\":{}}}", value);
    heap_string(payload)
}

fn json_err(message: &str) -> *const StringHeader {
    let escaped = message.replace('"', "\\\"");
    let payload = format!("{{\"ok\":false,\"error\":\"{}\"}}", escaped);
    heap_string(payload)
}

fn heap_string(s: String) -> *const StringHeader {
    let bytes = s.into_bytes();
    let total = std::mem::size_of::<StringHeader>() + bytes.len();
    let layout = std::alloc::Layout::from_size_align(total, std::mem::align_of::<StringHeader>())
        .expect("layout");
    unsafe {
        let ptr = std::alloc::alloc(layout) as *mut StringHeader;
        (*ptr).length = bytes.len() as u32;
        let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<StringHeader>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data_ptr, bytes.len());
        ptr as *const StringHeader
    }
}

fn block<F: std::future::Future<Output = T>, T>(fut: F) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(fut)
}

fn parse_compose_file(file_ptr: *const StringHeader) -> Option<PathBuf> {
    unsafe { string_from_header(file_ptr) }.map(PathBuf::from)
}

fn parse_services_filter(raw: Option<String>) -> Result<Vec<String>, &'static str> {
    match raw {
        None => Ok(Vec::new()),
        Some(raw) if raw.trim().is_empty() => Ok(Vec::new()),
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "services must be a JSON array"),
    }
}

fn make_engine(files: Vec<PathBuf>) -> Result<Arc<ComposeEngine>, String> {
    let config = crate::config::ProjectConfig::new(files, None, Vec::new());
    let proj = crate::project::ComposeProject::load(&config).map_err(|e| e.to_string())?;
    let backend: Arc<dyn crate::backend::ContainerBackend> =
        match block(crate::backend::detect_backend()) {
            Ok(b) => Arc::from(b),
            Err(e) => return Err(format!("{:?}", e)),
        };
    Ok(Arc::new(ComposeEngine::new(
        proj.spec,
        proj.project_name,
        backend,
    )))
}

// ──────────────────────────────────────────────────────────────
// Exported FFI functions
// ──────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_compose_start(file_ptr: *const StringHeader) -> *const StringHeader {
    let files: Vec<PathBuf> = parse_compose_file(file_ptr).into_iter().collect();
    match make_engine(files) {
        Err(e) => json_err(&e),
        Ok(engine) => match block(engine.up(&[], true, false, false)) {
            Ok(_) => json_ok("null"),
            Err(e) => json_err(&e.to_string()),
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_stop(file_ptr: *const StringHeader) -> *const StringHeader {
    let files: Vec<PathBuf> = parse_compose_file(file_ptr).into_iter().collect();
    match make_engine(files) {
        Err(e) => json_err(&e),
        Ok(engine) => match block(engine.down(&[], false, false)) {
            Ok(_) => json_ok("null"),
            Err(e) => json_err(&e.to_string()),
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_ps(file_ptr: *const StringHeader) -> *const StringHeader {
    let files: Vec<PathBuf> = parse_compose_file(file_ptr).into_iter().collect();
    match make_engine(files) {
        Err(e) => json_err(&e),
        Ok(engine) => match block(engine.ps()) {
            Err(e) => json_err(&e.to_string()),
            Ok(infos) => {
                let items: Vec<String> = infos
                    .iter()
                    .map(|i| {
                        format!(
                            "{{\"service\":\"{}\",\"container\":\"{}\",\"status\":\"{}\"}}",
                            i.name, i.id, i.status
                        )
                    })
                    .collect();
                let array = format!("[{}]", items.join(","));
                json_ok(&array)
            }
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_logs(
    file_ptr: *const StringHeader,
    services_ptr: *const StringHeader,
    _follow: bool,
) -> *const StringHeader {
    let files: Vec<PathBuf> = parse_compose_file(file_ptr).into_iter().collect();
    let services = match parse_services_filter(string_from_header(services_ptr)) {
        Ok(services) => services,
        Err(message) => return json_err(message),
    };

    match make_engine(files) {
        Err(e) => json_err(&e),
        Ok(engine) => match block(engine.logs(&services, None)) {
            Err(e) => json_err(&e.to_string()),
            Ok(logs) => match serde_json::to_string(&logs) {
                Ok(payload) => json_ok(&payload),
                Err(e) => json_err(&e.to_string()),
            },
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_exec(
    file_ptr: *const StringHeader,
    service_ptr: *const StringHeader,
    cmd_ptr: *const StringHeader,
) -> *const StringHeader {
    let files: Vec<PathBuf> = parse_compose_file(file_ptr).into_iter().collect();
    let service = match string_from_header(service_ptr) {
        Some(s) => s,
        None => return json_err("service name is required"),
    };
    let cmd: Vec<String> = string_from_header(cmd_ptr)
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default();

    match make_engine(files) {
        Err(e) => json_err(&e),
        Ok(engine) => match block(engine.exec(&service, &cmd, None, None)) {
            Err(e) => json_err(&e.to_string()),
            Ok(result) => {
                let stdout = result.stdout.replace('"', "\\\"").replace('\n', "\\n");
                let stderr = result.stderr.replace('"', "\\\"").replace('\n', "\\n");
                let payload = format!("{{\"stdout\":\"{}\",\"stderr\":\"{}\"}}", stdout, stderr);
                json_ok(&payload)
            }
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_config(file_ptr: *const StringHeader) -> *const StringHeader {
    let files: Vec<PathBuf> = parse_compose_file(file_ptr).into_iter().collect();
    let config = crate::config::ProjectConfig::new(files, None, Vec::new());
    match crate::project::ComposeProject::load(&config) {
        Err(e) => json_err(&e.to_string()),
        Ok(proj) => {
            let yaml = proj.spec.to_yaml().unwrap_or_default();
            let escaped = yaml.replace('"', "\\\"").replace('\n', "\\n");
            json_ok(&format!("\"{}\"", escaped))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_services_filter;

    #[test]
    fn services_filter_accepts_absent_and_empty_values() {
        assert_eq!(parse_services_filter(None), Ok(Vec::new()));
        assert_eq!(
            parse_services_filter(Some("  ".to_string())),
            Ok(Vec::new())
        );
    }

    #[test]
    fn services_filter_accepts_a_string_array() {
        assert_eq!(
            parse_services_filter(Some(r#"["api","db"]"#.to_string())),
            Ok(vec!["api".to_string(), "db".to_string()])
        );
    }

    #[test]
    fn services_filter_rejects_invalid_or_non_array_json() {
        assert_eq!(
            parse_services_filter(Some("not-json".to_string())),
            Err("services must be a JSON array")
        );
        assert_eq!(
            parse_services_filter(Some(r#"{"service":"api"}"#.to_string())),
            Err("services must be a JSON array")
        );
    }
}
