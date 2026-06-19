//! Plugin system for Perry-compiled applications
//!
//! Provides a native plugin architecture where plugins are compiled to shared libraries
//! (.dylib on macOS, .so on Linux) and loaded at runtime via dlopen.
//!
//! Plugin .dylibs leave perry-runtime symbols unresolved — they bind to the host
//! executable's copies at dlopen time. One GC, one arena, one runtime.
//!
//! ## Hook Modes
//! - **filter** (0): Chain context through handlers. Each handler receives the previous
//!   handler's return value. Default mode.
//! - **action** (1): Fire-and-forget. Handlers are called but return values are ignored.
//!   The original context is always returned.
//! - **waterfall** (2): Stop at the first handler that returns a non-undefined value.
//!   Returns that value immediately.
//!
//! ## Priority
//! Lower numbers run first (default = 10). Handlers at the same priority run in
//! registration order.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::Mutex;

use lazy_static::lazy_static;

use crate::value::JSValue;

/// ABI version — plugins must match this to load
const PLUGIN_ABI_VERSION: u64 = 2;

/// Hook execution modes
const HOOK_MODE_FILTER: u8 = 0;
const HOOK_MODE_ACTION: u8 = 1;
const HOOK_MODE_WATERFALL: u8 = 2;

/// Default hook priority (lower = runs first)
const DEFAULT_PRIORITY: i32 = 10;

/// Wrapper for raw library handle that implements Send
/// Safety: library handles are only used on the main thread via Mutex-protected access
struct LibHandle(*mut libc::c_void);
unsafe impl Send for LibHandle {}

// ============================================================================
// Platform-specific raw library load/sym/close — implemented per OS.
//
// The plugin ABI is the same on every platform (perry_plugin_abi_version,
// plugin_activate, plugin_deactivate + the per-handler FFI surface in
// `register_hook` / `register_tool` / etc. above), so the only divergence
// between Unix and Windows is the dynamic-loader API:
//   - Unix:    `libc::dlopen` / `libc::dlsym` / `libc::dlclose` (POSIX)
//   - Windows: `LoadLibraryW` / `GetProcAddress` / `FreeLibrary` (Win32)
//
// The helpers below collapse those to a small uniform shape so the
// `perry_plugin_load` / `perry_plugin_unload` bodies don't have to fork.
// ============================================================================

#[cfg(unix)]
type RawLibHandle = *mut libc::c_void;

#[cfg(windows)]
type RawLibHandle = windows_sys::Win32::Foundation::HMODULE;

#[cfg(unix)]
unsafe fn open_library(path: &str) -> Option<RawLibHandle> {
    let c_path = CString::new(path).ok()?;
    let h = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
    if h.is_null() {
        None
    } else {
        Some(h)
    }
}

#[cfg(windows)]
unsafe fn open_library(path: &str) -> Option<RawLibHandle> {
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    // LoadLibraryW takes a NUL-terminated UTF-16 path. Encode once, free on return.
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let h = LoadLibraryW(wide.as_ptr());
    if h.is_null() {
        None
    } else {
        Some(h)
    }
}

#[cfg(unix)]
unsafe fn lookup_symbol(handle: RawLibHandle, name: &str) -> Option<*mut libc::c_void> {
    let c_name = CString::new(name).ok()?;
    let p = libc::dlsym(handle, c_name.as_ptr());
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

#[cfg(windows)]
unsafe fn lookup_symbol(handle: RawLibHandle, name: &str) -> Option<*mut libc::c_void> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    // GetProcAddress's `lpProcName` is a NUL-terminated *ANSI* string. The
    // plugin ABI surface (perry_plugin_*, plugin_activate, plugin_deactivate)
    // is all ASCII, so a CString is the right shape. GetProcAddress returns
    // `Option<unsafe extern "system" fn() -> isize>` — None means "symbol
    // not found", which is what we want to forward up the call chain.
    let c_name = CString::new(name).ok()?;
    let p = GetProcAddress(handle, c_name.as_ptr() as *const u8);
    match p {
        Some(f) => Some(f as *mut libc::c_void),
        None => None,
    }
}

#[cfg(unix)]
unsafe fn close_library(_handle: RawLibHandle) {
    libc::dlclose(_handle);
}

#[cfg(windows)]
unsafe fn close_library(handle: RawLibHandle) {
    use windows_sys::Win32::Foundation::FreeLibrary;
    let _ = FreeLibrary(handle);
}

#[cfg(unix)]
fn last_load_error(path: &str) -> String {
    unsafe {
        let err = libc::dlerror();
        if err.is_null() {
            format!("dlopen failed for {}", path)
        } else {
            format!(
                "dlopen failed for {}: {}",
                path,
                CStr::from_ptr(err).to_string_lossy()
            )
        }
    }
}

#[cfg(windows)]
fn last_load_error(path: &str) -> String {
    use windows_sys::Win32::Foundation::GetLastError;
    let err = unsafe { GetLastError() };
    format!("LoadLibraryW failed for {}: Win32 error {}", path, err)
}

struct PluginMetadata {
    name: String,
    version: String,
    description: String,
}

struct PluginEntry {
    id: u64,
    path_name: String,
    // `*mut c_void` because the concrete handle type differs per OS
    // (`*mut c_void` on Unix = `libc::dlopen` return; `HMODULE` on Windows
    // = `*mut c_void` per windows-sys). Stored as a raw void pointer so the
    // field's shape is platform-agnostic.
    lib_handle: LibHandle,
    activate_called: bool,
    metadata: Option<PluginMetadata>,
}

struct HookRegistration {
    plugin_id: u64,
    /// NaN-boxed closure pointer (f64 bits)
    handler_closure: u64,
    /// Execution priority (lower = runs first)
    priority: i32,
    /// Hook mode: 0=filter, 1=action, 2=waterfall
    mode: u8,
}

struct ToolRegistration {
    plugin_id: u64,
    name: String,
    description: String,
    handler_closure: u64,
}

struct ServiceRegistration {
    plugin_id: u64,
    // #854: plugin manifest metadata retained for the plugin subsystem
    #[allow(dead_code)]
    name: String,
    start_fn: u64,
    stop_fn: u64,
}

struct RouteRegistration {
    plugin_id: u64,
    // #854: plugin manifest metadata retained for the plugin subsystem
    #[allow(dead_code)]
    path: String,
    handler_closure: u64,
}

struct EventRegistration {
    plugin_id: u64,
    handler_closure: u64,
}

struct PluginRegistry {
    plugins: Vec<PluginEntry>,
    hooks: HashMap<String, Vec<HookRegistration>>,
    tools: Vec<ToolRegistration>,
    services: Vec<ServiceRegistration>,
    routes: Vec<RouteRegistration>,
    /// Event bus: event_name -> list of handlers
    events: HashMap<String, Vec<EventRegistration>>,
    /// Host-provided config: key -> NaN-boxed f64 value
    config: HashMap<String, u64>,
    next_plugin_id: u64,
    /// Maps api_handle -> plugin_id for active plugin_activate calls
    active_api_handles: HashMap<i64, u64>,
    next_api_handle: i64,
}

impl PluginRegistry {
    fn new() -> Self {
        Self {
            plugins: Vec::new(),
            hooks: HashMap::new(),
            tools: Vec::new(),
            services: Vec::new(),
            routes: Vec::new(),
            events: HashMap::new(),
            config: HashMap::new(),
            next_plugin_id: 1,
            active_api_handles: HashMap::new(),
            next_api_handle: 1,
        }
    }

    fn alloc_plugin_id(&mut self) -> u64 {
        let id = self.next_plugin_id;
        self.next_plugin_id += 1;
        id
    }

    fn alloc_api_handle(&mut self, plugin_id: u64) -> i64 {
        let handle = self.next_api_handle;
        self.next_api_handle += 1;
        self.active_api_handles.insert(handle, plugin_id);
        handle
    }

    fn plugin_id_for_handle(&self, handle: i64) -> Option<u64> {
        self.active_api_handles.get(&handle).copied()
    }

    fn remove_plugin_registrations(&mut self, plugin_id: u64) {
        for hooks in self.hooks.values_mut() {
            hooks.retain(|h| h.plugin_id != plugin_id);
        }
        self.hooks.retain(|_, v| !v.is_empty());
        self.tools.retain(|t| t.plugin_id != plugin_id);
        self.services.retain(|s| s.plugin_id != plugin_id);
        self.routes.retain(|r| r.plugin_id != plugin_id);
        for handlers in self.events.values_mut() {
            handlers.retain(|e| e.plugin_id != plugin_id);
        }
        self.events.retain(|_, v| !v.is_empty());
    }
}

lazy_static! {
    static ref REGISTRY: Mutex<PluginRegistry> = Mutex::new(PluginRegistry::new());
}

#[cfg(test)]
pub(crate) static PLUGIN_REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

/// GC root scanner for closures and JS values held by the process-wide
/// plugin registry. These slots live outside the managed heap, so copied-minor
/// GC must see and rewrite them explicitly.
pub fn scan_plugin_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_plugin_roots_mut(&mut visitor);
}

pub fn scan_plugin_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut reg = crate::gc::lock_gc_root_registry(&*REGISTRY);
    for hooks in reg.hooks.values_mut() {
        for hook in hooks.iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut hook.handler_closure);
        }
    }
    for tool in reg.tools.iter_mut() {
        visitor.visit_nanbox_u64_slot(&mut tool.handler_closure);
    }
    for service in reg.services.iter_mut() {
        visitor.visit_nanbox_u64_slot(&mut service.start_fn);
        visitor.visit_nanbox_u64_slot(&mut service.stop_fn);
    }
    for route in reg.routes.iter_mut() {
        visitor.visit_nanbox_u64_slot(&mut route.handler_closure);
    }
    for handlers in reg.events.values_mut() {
        for handler in handlers.iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut handler.handler_closure);
        }
    }
    for value in reg.config.values_mut() {
        visitor.visit_nanbox_u64_slot(value);
    }
}

// ============================================================================
// Helper: extract a Rust string from a NaN-boxed f64 string value
// ============================================================================

unsafe fn extract_string(nanboxed: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(nanboxed);
    if ptr == 0 {
        return String::new();
    }
    let header = ptr as *const crate::string::StringHeader;
    let len = (*header).byte_len as usize;
    let data = (header as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    let slice = std::slice::from_raw_parts(data, len);
    String::from_utf8_lossy(slice).into_owned()
}

/// Create a NaN-boxed string from a Rust &str
unsafe fn make_nanboxed_string(s: &str) -> f64 {
    let header = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(header).bits())
}

#[inline]
unsafe fn store_plugin_key_slot(
    keys_handle: &crate::gc::RuntimeHandle<'_>,
    index: usize,
    key: &[u8],
) {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let keys_arr = keys_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    crate::array::store_array_slot(keys_arr, index, JSValue::string_ptr(key_ptr).bits());
    (*keys_arr).length = (index + 1) as u32;
}

#[inline]
unsafe fn push_rooted_array_value(arr_handle: &crate::gc::RuntimeHandle<'_>, value: f64) {
    let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    let pushed = crate::array::js_array_push_f64(arr, value);
    arr_handle.set_raw_mut_ptr(pushed);
}

#[inline]
unsafe fn store_plugin_metadata_fields(
    obj: *mut crate::object::ObjectHeader,
    id_value: f64,
    name_value: f64,
    version_value: f64,
    description_value: f64,
) {
    crate::object::store_object_field_slot(obj, 0, id_value.to_bits());
    crate::object::store_object_field_slot(obj, 1, name_value.to_bits());
    crate::object::store_object_field_slot(obj, 2, version_value.to_bits());
    crate::object::store_object_field_slot(obj, 3, description_value.to_bits());
}

#[inline]
unsafe fn store_tool_metadata_fields(
    obj: *mut crate::object::ObjectHeader,
    name_value: f64,
    description_value: f64,
    plugin_id_value: f64,
) {
    crate::object::store_object_field_slot(obj, 0, name_value.to_bits());
    crate::object::store_object_field_slot(obj, 1, description_value.to_bits());
    crate::object::store_object_field_slot(obj, 2, plugin_id_value.to_bits());
}

#[cfg(test)]
pub(crate) unsafe fn test_store_plugin_metadata_fields(
    obj: *mut crate::object::ObjectHeader,
    id_value: f64,
    name_value: f64,
    version_value: f64,
    description_value: f64,
) {
    store_plugin_metadata_fields(obj, id_value, name_value, version_value, description_value);
}

#[cfg(test)]
pub(crate) unsafe fn test_store_tool_metadata_fields(
    obj: *mut crate::object::ObjectHeader,
    name_value: f64,
    description_value: f64,
    plugin_id_value: f64,
) {
    store_tool_metadata_fields(obj, name_value, description_value, plugin_id_value);
}

/// Call a closure pointer with one f64 argument
unsafe fn call_closure_1(handler_bits: u64, arg: f64) -> f64 {
    let ptr_mask: u64 = 0x0000_FFFF_FFFF_FFFF;
    let closure_ptr = (handler_bits & ptr_mask) as *const crate::closure::ClosureHeader;
    if closure_ptr.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    crate::closure::js_closure_call1(closure_ptr, arg)
}

// ============================================================================
// Plugin FFI — called by plugin code via handle dispatch
// ============================================================================

/// Register a hook handler (default: priority=10, mode=filter)
#[no_mangle]
pub extern "C" fn perry_plugin_register_hook(api_handle: i64, hook_name: f64, handler: f64) -> f64 {
    perry_plugin_register_hook_ex(
        api_handle,
        hook_name,
        handler,
        DEFAULT_PRIORITY as i64,
        HOOK_MODE_FILTER as i64,
    )
}

/// Register a hook handler with explicit priority and mode
/// priority: lower number = runs first (default 10)
/// mode: 0=filter (chain), 1=action (fire-and-forget), 2=waterfall (first result wins)
#[no_mangle]
pub extern "C" fn perry_plugin_register_hook_ex(
    api_handle: i64,
    hook_name: f64,
    handler: f64,
    priority: i64,
    mode: i64,
) -> f64 {
    let name = unsafe { extract_string(hook_name) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        let entry = HookRegistration {
            plugin_id,
            handler_closure: handler.to_bits(),
            priority: priority as i32,
            mode: mode as u8,
        };
        let hooks = reg.hooks.entry(name).or_default();
        hooks.push(entry);
        // Keep sorted by priority (stable sort preserves registration order for equal priorities)
        hooks.sort_by_key(|h| h.priority);
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Register a tool with name, description, and handler closure
#[no_mangle]
pub extern "C" fn perry_plugin_register_tool(
    api_handle: i64,
    name: f64,
    desc: f64,
    handler: f64,
) -> f64 {
    let tool_name = unsafe { extract_string(name) };
    let tool_desc = unsafe { extract_string(desc) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        reg.tools.push(ToolRegistration {
            plugin_id,
            name: tool_name,
            description: tool_desc,
            handler_closure: handler.to_bits(),
        });
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Register a service with start/stop functions
#[no_mangle]
pub extern "C" fn perry_plugin_register_service(
    api_handle: i64,
    name: f64,
    start_fn: f64,
    stop_fn: f64,
) -> f64 {
    let svc_name = unsafe { extract_string(name) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        reg.services.push(ServiceRegistration {
            plugin_id,
            name: svc_name,
            start_fn: start_fn.to_bits(),
            stop_fn: stop_fn.to_bits(),
        });
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Register an HTTP route handler
#[no_mangle]
pub extern "C" fn perry_plugin_register_route(api_handle: i64, path: f64, handler: f64) -> f64 {
    let route_path = unsafe { extract_string(path) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        reg.routes.push(RouteRegistration {
            plugin_id,
            path: route_path,
            handler_closure: handler.to_bits(),
        });
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Get a config value by key
#[no_mangle]
pub extern "C" fn perry_plugin_get_config(_api_handle: i64, key: f64) -> f64 {
    let key_str = unsafe { extract_string(key) };
    let reg = REGISTRY.lock().unwrap();
    match reg.config.get(&key_str) {
        Some(bits) => f64::from_bits(*bits),
        None => f64::from_bits(JSValue::undefined().bits()),
    }
}

/// Log a message from a plugin
#[no_mangle]
pub extern "C" fn perry_plugin_log(_api_handle: i64, level: i64, message: f64) -> f64 {
    let msg = unsafe { extract_string(message) };
    let level_str = match level {
        0 => "DEBUG",
        1 => "INFO",
        2 => "WARN",
        3 => "ERROR",
        _ => "LOG",
    };
    eprintln!("[plugin:{}] {}", level_str, msg);
    f64::from_bits(JSValue::undefined().bits())
}

/// Set plugin metadata (name, version, description)
#[no_mangle]
pub extern "C" fn perry_plugin_set_metadata(
    api_handle: i64,
    name: f64,
    version: f64,
    description: f64,
) -> f64 {
    let meta_name = unsafe { extract_string(name) };
    let meta_version = unsafe { extract_string(version) };
    let meta_desc = unsafe { extract_string(description) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        if let Some(entry) = reg.plugins.iter_mut().find(|p| p.id == plugin_id) {
            entry.metadata = Some(PluginMetadata {
                name: meta_name,
                version: meta_version,
                description: meta_desc,
            });
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Subscribe to an event (plugin-side event bus)
#[no_mangle]
pub extern "C" fn perry_plugin_on(api_handle: i64, event: f64, handler: f64) -> f64 {
    let event_name = unsafe { extract_string(event) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        reg.events
            .entry(event_name)
            .or_default()
            .push(EventRegistration {
                plugin_id,
                handler_closure: handler.to_bits(),
            });
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Emit an event from a plugin (dispatches to all subscribers)
#[no_mangle]
pub extern "C" fn perry_plugin_emit(api_handle: i64, event: f64, data: f64) -> f64 {
    let _ = api_handle; // plugin_id available if needed for filtering
    perry_plugin_emit_event(event, data)
}

/// Unregister a previously-registered hook handler. Removes the entry whose
/// closure bits match `handler` (closure-identity match). No-op if the
/// caller is not the registering plugin or no such entry exists.
#[no_mangle]
pub extern "C" fn perry_plugin_unregister_hook(
    api_handle: i64,
    hook_name: f64,
    handler: f64,
) -> f64 {
    let name = unsafe { extract_string(hook_name) };
    let target_bits = handler.to_bits();
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        if let Some(hooks) = reg.hooks.get_mut(&name) {
            if let Some(pos) = hooks
                .iter()
                .position(|h| h.plugin_id == plugin_id && h.handler_closure == target_bits)
            {
                hooks.remove(pos);
            }
        }
        reg.hooks.retain(|_, v| !v.is_empty());
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Unregister a tool by name. No-op if the caller didn't register it.
#[no_mangle]
pub extern "C" fn perry_plugin_unregister_tool(api_handle: i64, name: f64) -> f64 {
    let tool_name = unsafe { extract_string(name) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        reg.tools
            .retain(|t| !(t.plugin_id == plugin_id && t.name == tool_name));
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Unregister a service by name. The service's `stopFn` is invoked before
/// the entry is removed (matches the lifecycle contract of `registerService`).
#[no_mangle]
pub extern "C" fn perry_plugin_unregister_service(api_handle: i64, name: f64) -> f64 {
    let svc_name = unsafe { extract_string(name) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        if let Some(idx) = reg
            .services
            .iter()
            .position(|s| s.plugin_id == plugin_id && s.name == svc_name)
        {
            let ptr_mask: u64 = 0x0000_FFFF_FFFF_FFFF;
            let stop_bits = reg.services[idx].stop_fn;
            let stop_ptr = (stop_bits & ptr_mask) as *const crate::closure::ClosureHeader;
            if stop_ptr.is_null() {
                reg.services.remove(idx);
            } else {
                drop(reg);
                unsafe {
                    crate::closure::js_closure_call0(stop_ptr);
                }
                let mut reg = REGISTRY.lock().unwrap();
                if let Some(idx2) = reg
                    .services
                    .iter()
                    .position(|s| s.plugin_id == plugin_id && s.name == svc_name)
                {
                    reg.services.remove(idx2);
                }
            }
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Unregister a route by path. No-op if the caller didn't register it.
#[no_mangle]
pub extern "C" fn perry_plugin_unregister_route(api_handle: i64, path: f64) -> f64 {
    let route_path = unsafe { extract_string(path) };
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        reg.routes
            .retain(|r| !(r.plugin_id == plugin_id && r.path == route_path));
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Unsubscribe a previously-registered event handler. Removes the entry
/// whose closure bits match `handler`. No-op otherwise.
#[no_mangle]
pub extern "C" fn perry_plugin_off(api_handle: i64, event: f64, handler: f64) -> f64 {
    let event_name = unsafe { extract_string(event) };
    let target_bits = handler.to_bits();
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(plugin_id) = reg.plugin_id_for_handle(api_handle) {
        if let Some(handlers) = reg.events.get_mut(&event_name) {
            if let Some(pos) = handlers
                .iter()
                .position(|e| e.plugin_id == plugin_id && e.handler_closure == target_bits)
            {
                handlers.remove(pos);
            }
        }
        reg.events.retain(|_, v| !v.is_empty());
    }
    f64::from_bits(JSValue::undefined().bits())
}

// ============================================================================
// Host-side functions — called by the host application
// ============================================================================

/// Load a plugin from a shared library path.
/// Returns the plugin ID (> 0) on success, 0 on failure.
///
/// Cross-platform: Unix uses `libc::dlopen` (POSIX), Windows uses
/// `LoadLibraryW` (Win32). Both paths converge through the
/// `open_library` / `lookup_symbol` / `close_library` / `last_load_error`
/// helpers above, so the activate/deactivate orchestration is shared.
#[no_mangle]
pub extern "C" fn perry_plugin_load(path_val: f64) -> i64 {
    let path_str = unsafe { extract_string(path_val) };

    let handle = match unsafe { open_library(&path_str) } {
        Some(h) => h,
        None => {
            eprintln!("[plugin] {}", last_load_error(&path_str));
            return 0;
        }
    };

    unsafe {
        // Check ABI version if the plugin exports it. A plugin that omits
        // this symbol still loads — version checks are an opt-in safety net
        // for plugin authors, not a hard requirement (matches the Unix
        // behavior pre-Windows-support).
        if let Some(abi_fn_ptr) = lookup_symbol(handle, "perry_plugin_abi_version") {
            let abi_fn: extern "C" fn() -> u64 = std::mem::transmute(abi_fn_ptr);
            let version = abi_fn();
            if version != PLUGIN_ABI_VERSION {
                eprintln!(
                    "[plugin] ABI version mismatch for {}: plugin={}, host={}",
                    path_str, version, PLUGIN_ABI_VERSION
                );
                close_library(handle);
                return 0;
            }
        }

        // plugin_activate is mandatory — every plugin must export it.
        let activate_ptr = match lookup_symbol(handle, "plugin_activate") {
            Some(p) => p,
            None => {
                eprintln!("[plugin] No plugin_activate symbol in {}", path_str);
                close_library(handle);
                return 0;
            }
        };

        let mut reg = REGISTRY.lock().unwrap();
        let plugin_id = reg.alloc_plugin_id();
        let api_handle = reg.alloc_api_handle(plugin_id);

        let name = std::path::Path::new(&path_str)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        reg.plugins.push(PluginEntry {
            id: plugin_id,
            path_name: name.clone(),
            lib_handle: LibHandle(handle as *mut libc::c_void),
            activate_called: false,
            metadata: None,
        });

        // Release lock before calling activate (plugin code may call register_*)
        drop(reg);

        let activate_fn: extern "C" fn(i64) -> i64 = std::mem::transmute(activate_ptr);
        let result = activate_fn(api_handle);

        let mut reg = REGISTRY.lock().unwrap();
        if let Some(entry) = reg.plugins.iter_mut().find(|p| p.id == plugin_id) {
            entry.activate_called = true;
        }
        reg.active_api_handles.remove(&api_handle);

        if result != 0 {
            let display_name = reg
                .plugins
                .iter()
                .find(|p| p.id == plugin_id)
                .and_then(|p| p.metadata.as_ref())
                .map(|m| format!("{} v{}", m.name, m.version))
                .unwrap_or(name);
            eprintln!("[plugin] Loaded: {} (id={})", display_name, plugin_id);
        }
        plugin_id as i64
    }
}

/// Unload a plugin by its ID. Cross-platform: see `perry_plugin_load`.
#[no_mangle]
pub extern "C" fn perry_plugin_unload(plugin_id_val: i64) {
    let plugin_id = plugin_id_val as u64;

    let mut reg = REGISTRY.lock().unwrap();

    let entry_idx = reg.plugins.iter().position(|p| p.id == plugin_id);
    let entry = match entry_idx {
        Some(idx) => reg.plugins.remove(idx),
        None => {
            eprintln!("[plugin] No plugin with id={}", plugin_id);
            return;
        }
    };

    // Remove all registrations BEFORE calling deactivate (prevents dangling pointers)
    reg.remove_plugin_registrations(plugin_id);

    let handle = entry.lib_handle.0;
    let name = entry
        .metadata
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or(entry.path_name.clone());
    drop(reg);

    unsafe {
        // plugin_deactivate is optional — a plugin that doesn't export it
        // still unloads cleanly (the registry teardown already cleared
        // its hooks/tools/services/routes/events).
        if let Some(deactivate_ptr) = lookup_symbol(handle as RawLibHandle, "plugin_deactivate") {
            let deactivate_fn: extern "C" fn() = std::mem::transmute(deactivate_ptr);
            deactivate_fn();
        }
        close_library(handle as RawLibHandle);
    }

    eprintln!("[plugin] Unloaded: {} (id={})", name, plugin_id);
}

/// Emit a hook — calls all registered handlers for the given hook name
/// Respects priority ordering and hook modes (filter/action/waterfall)
#[no_mangle]
pub extern "C" fn perry_plugin_emit_hook(hook_name: f64, context: f64) -> f64 {
    let name = unsafe { extract_string(hook_name) };

    // Collect handler info while holding the lock, then release
    let handlers: Vec<(u64, u8)> = {
        let reg = REGISTRY.lock().unwrap();
        match reg.hooks.get(&name) {
            Some(hooks) => hooks.iter().map(|h| (h.handler_closure, h.mode)).collect(),
            None => return context,
        }
    };

    // Determine effective mode from first handler (all handlers for a hook should use same mode,
    // but if mixed, use the mode of each individual handler)
    let mut current_ctx = context;
    for (handler_bits, mode) in handlers {
        let result = unsafe { call_closure_1(handler_bits, current_ctx) };
        let result_bits = result.to_bits();

        match mode {
            HOOK_MODE_FILTER => {
                // Chain: pass modified context to next handler
                if result_bits != JSValue::undefined().bits() {
                    current_ctx = result;
                }
            }
            HOOK_MODE_ACTION => {
                // Fire-and-forget: ignore return value, keep original context
            }
            HOOK_MODE_WATERFALL => {
                // Stop at first non-undefined result
                if result_bits != JSValue::undefined().bits() {
                    return result;
                }
            }
            _ => {
                // Unknown mode — treat as filter
                if result_bits != JSValue::undefined().bits() {
                    current_ctx = result;
                }
            }
        }
    }
    current_ctx
}

/// Emit an event on the event bus (host-side)
/// Calls all subscribers for the event, returns undefined
#[no_mangle]
pub extern "C" fn perry_plugin_emit_event(event: f64, data: f64) -> f64 {
    let event_name = unsafe { extract_string(event) };

    let handlers: Vec<u64> = {
        let reg = REGISTRY.lock().unwrap();
        match reg.events.get(&event_name) {
            Some(subs) => subs.iter().map(|e| e.handler_closure).collect(),
            None => return f64::from_bits(JSValue::undefined().bits()),
        }
    };

    for handler_bits in handlers {
        unsafe {
            call_closure_1(handler_bits, data);
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Invoke a registered tool by name
/// Returns the tool handler's return value, or undefined if not found
#[no_mangle]
pub extern "C" fn perry_plugin_invoke_tool(name: f64, args: f64) -> f64 {
    let tool_name = unsafe { extract_string(name) };

    let handler_bits: Option<u64> = {
        let reg = REGISTRY.lock().unwrap();
        reg.tools
            .iter()
            .find(|t| t.name == tool_name)
            .map(|t| t.handler_closure)
    };

    match handler_bits {
        Some(bits) => unsafe { call_closure_1(bits, args) },
        None => {
            eprintln!("[plugin] Tool not found: {}", tool_name);
            f64::from_bits(JSValue::undefined().bits())
        }
    }
}

/// Set a config value (host-side, before or after loading plugins)
#[no_mangle]
pub extern "C" fn perry_plugin_set_config(key: f64, value: f64) -> f64 {
    let key_str = unsafe { extract_string(key) };
    let mut reg = REGISTRY.lock().unwrap();
    reg.config.insert(key_str, value.to_bits());
    f64::from_bits(JSValue::undefined().bits())
}

/// Discover plugin files in a directory
/// Returns a NaN-boxed array of string paths
#[no_mangle]
pub extern "C" fn perry_plugin_discover(dir_path: f64) -> f64 {
    let dir = unsafe { extract_string(dir_path) };

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("[plugin] Cannot read directory {}: {}", dir, err);
            let arr = crate::array::js_array_alloc(8);
            return f64::from_bits(JSValue::pointer(arr as *const u8).bits());
        }
    };

    let arr = crate::array::js_array_alloc(8);
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let is_plugin = matches!(ext, "dylib" | "so" | "dll");

        if is_plugin {
            if let Some(path_str) = path.to_str() {
                let s =
                    crate::string::js_string_from_bytes(path_str.as_ptr(), path_str.len() as u32);
                let nanboxed = JSValue::string_ptr(s);
                unsafe {
                    push_rooted_array_value(&arr_handle, f64::from_bits(nanboxed.bits()));
                }
            }
        }
    }

    let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// List loaded plugins — returns array of objects with {id, name, version, description}
#[no_mangle]
pub extern "C" fn perry_plugin_list_plugins() -> f64 {
    let plugins: Vec<(u64, String, String, String)> = {
        let reg = crate::gc::lock_gc_root_registry(&*REGISTRY);
        reg.plugins
            .iter()
            .map(|plugin| {
                let name = plugin
                    .metadata
                    .as_ref()
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| plugin.path_name.clone());
                let version = plugin
                    .metadata
                    .as_ref()
                    .map(|m| m.version.clone())
                    .unwrap_or_else(|| "0.0.0".to_string());
                let description = plugin
                    .metadata
                    .as_ref()
                    .map(|m| m.description.clone())
                    .unwrap_or_default();
                (plugin.id, name, version, description)
            })
            .collect()
    };
    let arr = crate::array::js_array_alloc(plugins.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);

    for (index, (plugin_id, name, version, desc)) in plugins.into_iter().enumerate() {
        let item_scope = crate::gc::RuntimeHandleScope::new();
        unsafe {
            // Create object with 4 fields: id, name, version, description
            let obj = crate::object::js_object_alloc(0, 4);
            let obj_handle = item_scope.root_raw_mut_ptr(obj);
            let keys_arr = crate::array::js_array_alloc(4);
            let keys_handle = item_scope.root_raw_mut_ptr(keys_arr);

            store_plugin_key_slot(&keys_handle, 0, b"id");
            store_plugin_key_slot(&keys_handle, 1, b"name");
            store_plugin_key_slot(&keys_handle, 2, b"version");
            store_plugin_key_slot(&keys_handle, 3, b"description");

            let id_value = plugin_id as f64;
            let name_value = make_nanboxed_string(&name);
            let name_handle = item_scope.root_nanbox_f64(name_value);
            let version_value = make_nanboxed_string(&version);
            let version_handle = item_scope.root_nanbox_f64(version_value);
            let desc_value = make_nanboxed_string(&desc);
            let desc_handle = item_scope.root_nanbox_f64(desc_value);

            let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
            store_plugin_metadata_fields(
                obj,
                id_value,
                name_handle.get_nanbox_f64(),
                version_handle.get_nanbox_f64(),
                desc_handle.get_nanbox_f64(),
            );

            let keys_arr = keys_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            crate::object::js_object_set_keys(obj, keys_arr);

            let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            crate::array::store_array_slot(arr, index, JSValue::pointer(obj as *const u8).bits());
            (*arr).length = (index + 1) as u32;
        }
    }

    let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// List registered hook names — returns array of strings
#[no_mangle]
pub extern "C" fn perry_plugin_list_hooks() -> f64 {
    let hook_names: Vec<String> = {
        let reg = crate::gc::lock_gc_root_registry(&*REGISTRY);
        reg.hooks.keys().cloned().collect()
    };
    let arr = crate::array::js_array_alloc(hook_names.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);

    for (index, hook_name) in hook_names.into_iter().enumerate() {
        unsafe {
            let s = make_nanboxed_string(&hook_name);
            let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            crate::array::store_array_slot(arr, index, s.to_bits());
            (*arr).length = (index + 1) as u32;
        }
    }

    let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// List registered tools — returns array of objects with {name, description, pluginId}
#[no_mangle]
pub extern "C" fn perry_plugin_list_tools() -> f64 {
    let tools: Vec<(String, String, u64)> = {
        let reg = crate::gc::lock_gc_root_registry(&*REGISTRY);
        reg.tools
            .iter()
            .map(|tool| (tool.name.clone(), tool.description.clone(), tool.plugin_id))
            .collect()
    };
    let arr = crate::array::js_array_alloc(tools.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);

    for (index, (tool_name, tool_desc, plugin_id)) in tools.into_iter().enumerate() {
        let item_scope = crate::gc::RuntimeHandleScope::new();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 3);
            let obj_handle = item_scope.root_raw_mut_ptr(obj);
            let keys_arr = crate::array::js_array_alloc(3);
            let keys_handle = item_scope.root_raw_mut_ptr(keys_arr);

            store_plugin_key_slot(&keys_handle, 0, b"name");
            store_plugin_key_slot(&keys_handle, 1, b"description");
            store_plugin_key_slot(&keys_handle, 2, b"pluginId");

            let name_value = make_nanboxed_string(&tool_name);
            let name_handle = item_scope.root_nanbox_f64(name_value);
            let desc_value = make_nanboxed_string(&tool_desc);
            let desc_handle = item_scope.root_nanbox_f64(desc_value);
            let plugin_id_value = plugin_id as f64;
            let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
            store_tool_metadata_fields(
                obj,
                name_handle.get_nanbox_f64(),
                desc_handle.get_nanbox_f64(),
                plugin_id_value,
            );

            let keys_arr = keys_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            crate::object::js_object_set_keys(obj, keys_arr);

            let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            crate::array::store_array_slot(arr, index, JSValue::pointer(obj as *const u8).bits());
            (*arr).length = (index + 1) as u32;
        }
    }

    let arr = arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// Get number of loaded plugins
#[no_mangle]
pub extern "C" fn perry_plugin_count() -> i64 {
    let reg = REGISTRY.lock().unwrap();
    reg.plugins.len() as i64
}

/// Initialize the plugin system (called from host's main)
#[no_mangle]
pub extern "C" fn perry_plugin_init() {
    let _reg = REGISTRY.lock().unwrap();
}

#[cfg(test)]
pub(crate) fn test_clear_plugin_roots() {
    *crate::gc::lock_gc_root_registry(&*REGISTRY) = PluginRegistry::new();
}

#[cfg(test)]
pub(crate) fn test_seed_plugin_roots(bits: u64) {
    let mut reg = crate::gc::lock_gc_root_registry(&*REGISTRY);
    *reg = PluginRegistry::new();
    reg.hooks.insert(
        "hook".to_string(),
        vec![HookRegistration {
            plugin_id: 1,
            handler_closure: bits,
            priority: DEFAULT_PRIORITY,
            mode: HOOK_MODE_FILTER,
        }],
    );
    reg.tools.push(ToolRegistration {
        plugin_id: 1,
        name: "tool".to_string(),
        description: "tool".to_string(),
        handler_closure: bits,
    });
    reg.services.push(ServiceRegistration {
        plugin_id: 1,
        name: "service".to_string(),
        start_fn: bits,
        stop_fn: bits,
    });
    reg.routes.push(RouteRegistration {
        plugin_id: 1,
        path: "/route".to_string(),
        handler_closure: bits,
    });
    reg.events.insert(
        "event".to_string(),
        vec![EventRegistration {
            plugin_id: 1,
            handler_closure: bits,
        }],
    );
    reg.config.insert("config".to_string(), bits);
}

#[cfg(test)]
pub(crate) fn test_plugin_roots_snapshot() -> (u64, u64, u64, u64, u64, u64, u64) {
    let reg = crate::gc::lock_gc_root_registry(&*REGISTRY);
    (
        reg.hooks
            .get("hook")
            .and_then(|hooks| hooks.first())
            .map(|hook| hook.handler_closure)
            .unwrap_or(0),
        reg.tools
            .first()
            .map(|tool| tool.handler_closure)
            .unwrap_or(0),
        reg.services
            .first()
            .map(|service| service.start_fn)
            .unwrap_or(0),
        reg.services
            .first()
            .map(|service| service.stop_fn)
            .unwrap_or(0),
        reg.routes
            .first()
            .map(|route| route.handler_closure)
            .unwrap_or(0),
        reg.events
            .get("event")
            .and_then(|events| events.first())
            .map(|event| event.handler_closure)
            .unwrap_or(0),
        reg.config.get("config").copied().unwrap_or(0),
    )
}

// ============================================================================
// Unit tests — exercise the 5 selective-cleanup FFI functions added for
// issue #5270 (PluginApi.unregister* / off).
//
// Each test seeds a small PluginRegistry directly (bypassing dlopen +
// plugin_activate), then calls the unregister FFI with synthetic NaN-boxed
// args. The assertions read the registry state back to confirm the targeted
// row was removed without disturbing unrelated entries.
// ============================================================================

#[cfg(test)]
mod unregister_tests {
    use super::*;

    /// Allocate a fresh api_handle in the test registry bound to plugin_id 1.
    /// Mirrors what `perry_plugin_load` does for a real plugin: increments
    /// `next_api_handle` and inserts the binding so `plugin_id_for_handle`
    /// resolves. The handle is owned by no real shared library so there's
    /// no dlclose on drop — `test_clear_plugin_roots` reclaims everything
    /// at the end of the test.
    fn fresh_api_handle() -> i64 {
        let mut reg = REGISTRY.lock().unwrap();
        reg.alloc_api_handle(1)
    }

    /// Seed the registry with the canonical 6-entry fixture (one of each
    /// registration type) so each unregister test has well-defined state.
    /// Closure bits are arbitrary f64-bit patterns; identity compare uses
    /// `to_bits()`, so values just need to be distinct across entries.
    fn seed() {
        let mut reg = REGISTRY.lock().unwrap();
        *reg = PluginRegistry::new();
        // Reset the active_api_handles by reconstructing the registry, then
        // re-allocate the test's preferred handle (1) below.
        reg.hooks.insert(
            "beforeSave".to_string(),
            vec![
                HookRegistration {
                    plugin_id: 1,
                    handler_closure: 0xAAAA_0001_0001_u64,
                    priority: 10,
                    mode: HOOK_MODE_FILTER,
                },
                HookRegistration {
                    plugin_id: 1,
                    handler_closure: 0xAAAA_0001_0002_u64,
                    priority: 20,
                    mode: HOOK_MODE_ACTION,
                },
            ],
        );
        reg.tools.push(ToolRegistration {
            plugin_id: 1,
            name: "formatCode".to_string(),
            description: "format".to_string(),
            handler_closure: 0xBBBB_0001_0001_u64,
        });
        reg.tools.push(ToolRegistration {
            plugin_id: 2,
            name: "otherPluginTool".to_string(),
            description: "owned by plugin 2".to_string(),
            handler_closure: 0xBBBB_0002_0001_u64,
        });
        reg.services.push(ServiceRegistration {
            plugin_id: 1,
            name: "worker".to_string(),
            start_fn: 0xCCCC_0001_0001_u64,
            stop_fn: 0xCCCC_0001_0002_u64,
        });
        reg.routes.push(RouteRegistration {
            plugin_id: 1,
            path: "/api/foo".to_string(),
            handler_closure: 0xDDDD_0001_0001_u64,
        });
        reg.events.insert(
            "dataUpdated".to_string(),
            vec![EventRegistration {
                plugin_id: 1,
                handler_closure: 0xEEEE_0001_0001_u64,
            }],
        );
        reg.active_api_handles.insert(1, 1);
        reg.next_api_handle = 2;
    }

    fn teardown() {
        *REGISTRY.lock().unwrap() = PluginRegistry::new();
    }

    fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
        PLUGIN_REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn unregister_hook_removes_only_matching_entry() {
        let _test_guard = lock_tests();
        seed();
        let handle = fresh_api_handle();
        let hook_name = unsafe { make_nanboxed_string("beforeSave") };
        let target_handler = f64::from_bits(0xAAAA_0001_0001_u64);
        let _ = unsafe { perry_plugin_unregister_hook(handle, hook_name, target_handler) };

        let reg = REGISTRY.lock().unwrap();
        let hooks = reg
            .hooks
            .get("beforeSave")
            .expect("beforeSave still present");
        assert_eq!(hooks.len(), 1, "one matching handler should remain");
        assert_eq!(hooks[0].handler_closure, 0xAAAA_0001_0002_u64);
        drop(reg);

        // Second call with the remaining handler — bucket should now be empty
        // and the parent map entry pruned.
        let other_handler = f64::from_bits(0xAAAA_0001_0002_u64);
        let _ = unsafe { perry_plugin_unregister_hook(handle, hook_name, other_handler) };
        let reg = REGISTRY.lock().unwrap();
        assert!(
            reg.hooks.get("beforeSave").is_none(),
            "empty hook bucket should be removed"
        );
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_hook_with_wrong_bits_is_noop() {
        let _test_guard = lock_tests();
        seed();
        let handle = fresh_api_handle();
        let hook_name = unsafe { make_nanboxed_string("beforeSave") };
        let bogus_handler = f64::from_bits(0xDEAD_BEEF_DEAD_BEEF_u64);

        let _ = unsafe { perry_plugin_unregister_hook(handle, hook_name, bogus_handler) };

        let reg = REGISTRY.lock().unwrap();
        let hooks = reg.hooks.get("beforeSave").unwrap();
        assert_eq!(hooks.len(), 2, "no matching bits → both handlers stay");
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_hook_from_other_plugin_is_noop() {
        let _test_guard = lock_tests();
        seed();
        // Forge an api_handle bound to plugin 2 (the owner of otherPluginTool).
        // Plugin 2 shouldn't be able to remove plugin 1's hooks.
        let mut reg = REGISTRY.lock().unwrap();
        let handle_2 = reg.alloc_api_handle(2);
        drop(reg);

        let hook_name = unsafe { make_nanboxed_string("beforeSave") };
        let target = f64::from_bits(0xAAAA_0001_0001_u64);
        let _ = unsafe { perry_plugin_unregister_hook(handle_2, hook_name, target) };

        let reg = REGISTRY.lock().unwrap();
        assert_eq!(reg.hooks.get("beforeSave").unwrap().len(), 2);
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_tool_removes_only_matching_row() {
        let _test_guard = lock_tests();
        seed();
        let handle = fresh_api_handle();
        let name = unsafe { make_nanboxed_string("formatCode") };
        let _ = unsafe { perry_plugin_unregister_tool(handle, name) };

        let reg = REGISTRY.lock().unwrap();
        assert_eq!(reg.tools.len(), 1, "only the matching tool is removed");
        assert_eq!(reg.tools[0].name, "otherPluginTool");
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_tool_unknown_name_is_noop() {
        let _test_guard = lock_tests();
        seed();
        let handle = fresh_api_handle();
        let bogus = unsafe { make_nanboxed_string("doesNotExist") };
        let _ = unsafe { perry_plugin_unregister_tool(handle, bogus) };

        let reg = REGISTRY.lock().unwrap();
        assert_eq!(reg.tools.len(), 2);
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_route_removes_matching_path() {
        let _test_guard = lock_tests();
        seed();
        let handle = fresh_api_handle();
        let path = unsafe { make_nanboxed_string("/api/foo") };
        let _ = unsafe { perry_plugin_unregister_route(handle, path) };

        let reg = REGISTRY.lock().unwrap();
        assert_eq!(reg.routes.len(), 0);
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_service_removes_matching_row() {
        let _test_guard = lock_tests();
        seed();
        let handle = fresh_api_handle();
        let name = unsafe { make_nanboxed_string("worker") };
        // `stop_fn` is a closure pointer stored as bits — calling
        // `js_closure_call0` on it would dereference arbitrary memory. The
        // null-pointer guard inside unregister_service checks the masked pointer,
        // so stubbing stop_fn to zero exercises the removal path without crashing.
        {
            let mut reg = REGISTRY.lock().unwrap();
            for svc in reg.services.iter_mut() {
                svc.stop_fn = 0;
            }
        }
        let _ = unsafe { perry_plugin_unregister_service(handle, name) };

        let reg = REGISTRY.lock().unwrap();
        assert_eq!(reg.services.len(), 0, "worker service removed");
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_off_removes_matching_event_handler() {
        let _test_guard = lock_tests();
        seed();
        let handle = fresh_api_handle();
        let event = unsafe { make_nanboxed_string("dataUpdated") };
        let handler = f64::from_bits(0xEEEE_0001_0001_u64);
        let _ = unsafe { perry_plugin_off(handle, event, handler) };

        let reg = REGISTRY.lock().unwrap();
        assert!(
            reg.events.get("dataUpdated").is_none(),
            "empty event bucket should be removed"
        );
        drop(reg);
        teardown();
    }

    #[test]
    fn unregister_with_unknown_api_handle_is_noop() {
        let _test_guard = lock_tests();
        seed();
        // An api_handle that was never allocated returns None from
        // plugin_id_for_handle → every unregister path's body is skipped.
        let ghost_handle: i64 = 9999;
        let hook_name = unsafe { make_nanboxed_string("beforeSave") };
        let _ = unsafe { perry_plugin_unregister_hook(ghost_handle, hook_name, 0.0) };
        let name = unsafe { make_nanboxed_string("formatCode") };
        let _ = unsafe { perry_plugin_unregister_tool(ghost_handle, name) };
        let path = unsafe { make_nanboxed_string("/api/foo") };
        let _ = unsafe { perry_plugin_unregister_route(ghost_handle, path) };
        let _ = unsafe { perry_plugin_unregister_service(ghost_handle, name) };
        let event = unsafe { make_nanboxed_string("dataUpdated") };
        let _ = unsafe { perry_plugin_off(ghost_handle, event, 0.0) };

        let reg = REGISTRY.lock().unwrap();
        assert_eq!(reg.hooks.get("beforeSave").unwrap().len(), 2);
        assert_eq!(reg.tools.len(), 2);
        assert_eq!(reg.routes.len(), 1);
        assert_eq!(reg.services.len(), 1);
        assert_eq!(reg.events.get("dataUpdated").unwrap().len(), 1);
        drop(reg);
        teardown();
    }
}
