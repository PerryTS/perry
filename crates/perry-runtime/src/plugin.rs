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

struct PluginMetadata {
    name: String,
    version: String,
    description: String,
}

struct PluginEntry {
    id: u64,
    path_name: String,
    #[cfg(unix)]
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
    name: String,
    start_fn: u64,
    stop_fn: u64,
}

struct RouteRegistration {
    plugin_id: u64,
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

// ============================================================================
// Host-side functions — called by the host application
// ============================================================================

/// Load a plugin from a shared library path
/// Returns the plugin ID (> 0) on success, 0 on failure
#[cfg(unix)]
#[no_mangle]
pub extern "C" fn perry_plugin_load(path_val: f64) -> i64 {
    let path_str = unsafe { extract_string(path_val) };

    let c_path = match CString::new(path_str.clone()) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[plugin] Invalid path: {}", path_str);
            return 0;
        }
    };

    unsafe {
        let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            let err = libc::dlerror();
            if !err.is_null() {
                let err_str = CStr::from_ptr(err).to_string_lossy();
                eprintln!("[plugin] dlopen failed for {}: {}", path_str, err_str);
            }
            return 0;
        }

        // Check ABI version if available
        let abi_sym = CString::new("perry_plugin_abi_version").unwrap();
        let abi_fn_ptr = libc::dlsym(handle, abi_sym.as_ptr());
        if !abi_fn_ptr.is_null() {
            let abi_fn: extern "C" fn() -> u64 = std::mem::transmute(abi_fn_ptr);
            let version = abi_fn();
            if version != PLUGIN_ABI_VERSION {
                eprintln!(
                    "[plugin] ABI version mismatch for {}: plugin={}, host={}",
                    path_str, version, PLUGIN_ABI_VERSION
                );
                libc::dlclose(handle);
                return 0;
            }
        }

        // Look up plugin_activate
        let activate_sym = CString::new("plugin_activate").unwrap();
        let activate_ptr = libc::dlsym(handle, activate_sym.as_ptr());
        if activate_ptr.is_null() {
            eprintln!("[plugin] No plugin_activate symbol in {}", path_str);
            libc::dlclose(handle);
            return 0;
        }

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
            lib_handle: LibHandle(handle),
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

/// Unload a plugin by its ID
#[cfg(unix)]
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
        let deactivate_sym = CString::new("plugin_deactivate").unwrap();
        let deactivate_ptr = libc::dlsym(handle, deactivate_sym.as_ptr());
        if !deactivate_ptr.is_null() {
            let deactivate_fn: extern "C" fn() = std::mem::transmute(deactivate_ptr);
            deactivate_fn();
        }
        libc::dlclose(handle);
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
            let arr = unsafe { crate::array::js_array_alloc(8) };
            return f64::from_bits(JSValue::pointer(arr as *const u8).bits());
        }
    };

    let arr = unsafe { crate::array::js_array_alloc(8) };

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
                    crate::array::js_array_push_f64(arr, f64::from_bits(nanboxed.bits()));
                }
            }
        }
    }

    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// List loaded plugins — returns array of objects with {id, name, version, description}
#[no_mangle]
pub extern "C" fn perry_plugin_list_plugins() -> f64 {
    let reg = REGISTRY.lock().unwrap();
    let arr = unsafe { crate::array::js_array_alloc(reg.plugins.len() as u32) };

    for plugin in &reg.plugins {
        unsafe {
            // Create object with 4 fields: id, name, version, description
            let obj = crate::object::js_object_alloc(0, 4);
            let keys_arr = crate::array::js_array_alloc(4);

            // id
            let id_key = crate::string::js_string_from_bytes("id\0".as_ptr(), 2);
            crate::array::js_array_push(keys_arr, JSValue::string_ptr(id_key));
            let fields = (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *mut f64;
            *fields = plugin.id as f64;

            // name
            let name_key = crate::string::js_string_from_bytes("name\0".as_ptr(), 4);
            crate::array::js_array_push(keys_arr, JSValue::string_ptr(name_key));
            let name_str = plugin
                .metadata
                .as_ref()
                .map(|m| m.name.as_str())
                .unwrap_or(&plugin.path_name);
            *fields.add(1) = make_nanboxed_string(name_str);

            // version
            let ver_key = crate::string::js_string_from_bytes("version\0".as_ptr(), 7);
            crate::array::js_array_push(keys_arr, JSValue::string_ptr(ver_key));
            let version = plugin
                .metadata
                .as_ref()
                .map(|m| m.version.as_str())
                .unwrap_or("0.0.0");
            *fields.add(2) = make_nanboxed_string(version);

            // description
            let desc_key = crate::string::js_string_from_bytes("description\0".as_ptr(), 11);
            crate::array::js_array_push(keys_arr, JSValue::string_ptr(desc_key));
            let desc = plugin
                .metadata
                .as_ref()
                .map(|m| m.description.as_str())
                .unwrap_or("");
            *fields.add(3) = make_nanboxed_string(desc);

            (*obj).keys_array = keys_arr;

            crate::array::js_array_push_f64(
                arr,
                f64::from_bits(JSValue::pointer(obj as *const u8).bits()),
            );
        }
    }

    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// List registered hook names — returns array of strings
#[no_mangle]
pub extern "C" fn perry_plugin_list_hooks() -> f64 {
    let reg = REGISTRY.lock().unwrap();
    let arr = unsafe { crate::array::js_array_alloc(reg.hooks.len() as u32) };

    for hook_name in reg.hooks.keys() {
        unsafe {
            let s = make_nanboxed_string(hook_name);
            crate::array::js_array_push_f64(arr, s);
        }
    }

    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

/// List registered tools — returns array of objects with {name, description, pluginId}
#[no_mangle]
pub extern "C" fn perry_plugin_list_tools() -> f64 {
    let reg = REGISTRY.lock().unwrap();
    let arr = unsafe { crate::array::js_array_alloc(reg.tools.len() as u32) };

    for tool in &reg.tools {
        unsafe {
            let obj = crate::object::js_object_alloc(0, 3);
            let keys_arr = crate::array::js_array_alloc(3);

            let name_key = crate::string::js_string_from_bytes("name\0".as_ptr(), 4);
            crate::array::js_array_push(keys_arr, JSValue::string_ptr(name_key));
            let fields = (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *mut f64;
            *fields = make_nanboxed_string(&tool.name);

            let desc_key = crate::string::js_string_from_bytes("description\0".as_ptr(), 11);
            crate::array::js_array_push(keys_arr, JSValue::string_ptr(desc_key));
            *fields.add(1) = make_nanboxed_string(&tool.description);

            let pid_key = crate::string::js_string_from_bytes("pluginId\0".as_ptr(), 8);
            crate::array::js_array_push(keys_arr, JSValue::string_ptr(pid_key));
            *fields.add(2) = tool.plugin_id as f64;

            (*obj).keys_array = keys_arr;

            crate::array::js_array_push_f64(
                arr,
                f64::from_bits(JSValue::pointer(obj as *const u8).bits()),
            );
        }
    }

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
