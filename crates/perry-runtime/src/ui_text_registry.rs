//! Cross-platform `showToast` / `setText` runtime backbone (Phase 2 v3.3).
//!
//! Phase 2 v3 (v0.5.405) shipped HarmonyOS-only `perry_arkts_show_toast` and
//! `perry_arkts_set_text` symbols guarded behind `feature = "ohos-napi"`. On
//! every other backend (macOS / iOS / Linux GTK4 / Windows / Android / tvOS /
//! visionOS / watchOS) the codegen at `lower_call/native.rs` would emit calls
//! to those symbols regardless of target — and the link would fail because
//! the symbols only exist when `ohos-napi` is on.
//!
//! This module fills the gap: when `ohos-napi` is OFF, it provides
//! cross-platform stub definitions of `perry_arkts_show_toast` /
//! `perry_arkts_set_text` / `perry_arkts_register_text_id` that route to a
//! per-process **handler registry**. The platform-specific UI crate
//! (perry-ui-macos, perry-ui-gtk4, etc.) registers its own handlers at
//! startup via `js_register_show_toast_handler` / `js_register_set_text_handler`
//! / `js_register_text_id_handler`. Backends that haven't wired anything yet
//! get a hilog/eprintln "not yet implemented on <platform>" line so missing
//! coverage is discoverable.
//!
//! When `ohos-napi` is ON, `arkts_callbacks.rs` provides the canonical
//! drain-queue implementations of `perry_arkts_show_toast` /
//! `perry_arkts_set_text`, and this module's stubs are gated out via
//! `#[cfg(not(feature = "ohos-napi"))]` so there's no symbol collision.
//!
//! ## Symbol shape
//!
//! All three functions take **NaN-boxed JS value** arguments (raw `f64` bits
//! per Perry's tagging convention, `STRING_TAG=0x7FFF` for heap strings,
//! `SHORT_STRING_TAG=0x7FF9` for SSO). The handler-callback signature
//! receives plain Rust `&str` so the platform UI code doesn't need to know
//! about Perry's value representation.
//!
//! ## Registration model
//!
//! Each handler slot is an `AtomicPtr<()>` storing a function pointer. UI
//! crates register at `app_run` startup (or whenever they initialize),
//! before any user TS code calls `showToast`. Calls before registration
//! emit a one-time "no handler registered" warning and silently no-op.
//!
//! Mirrors the existing `js_register_stdlib_pump` pattern in `lib.rs`
//! (the v0.5.x cross-crate callback wiring that lets perry-ui-macos's
//! pump timer drive `js_stdlib_process_pending` without a hard link
//! dep on perry-stdlib).

#[cfg(not(feature = "ohos-napi"))]
use std::ptr::null_mut;
#[cfg(not(feature = "ohos-napi"))]
use std::sync::atomic::{AtomicPtr, Ordering};

use std::sync::Mutex;

/// Decode a NaN-boxed JS value to a Rust `String`. Matches the
/// `arkts_callbacks::decode_jsvalue_string` helper exactly so harmonyos
/// and non-harmonyos builds see identical string semantics. Falls back
/// to empty string on null header (defensive — should never happen with
/// codegen-emitted shape).
pub(crate) fn decode_jsvalue_string(handle: f64) -> String {
    let header = crate::value::js_jsvalue_to_string(handle);
    if header.is_null() {
        return String::new();
    }
    unsafe {
        let blen = (*header).byte_len as usize;
        let data_ptr =
            (header as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, blen);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Cross-platform handler signature. Receives a UTF-8 string view of the
/// already-decoded JS value. UI crates implement this on the main thread
/// (AppKit / UIKit / GTK4 / Win32 / etc).
pub type ShowToastHandler = extern "C" fn(msg_ptr: *const u8, msg_len: usize);

/// Cross-platform setText handler signature.
pub type SetTextHandler =
    extern "C" fn(id_ptr: *const u8, id_len: usize, val_ptr: *const u8, val_len: usize);

/// Cross-platform register-text-id handler signature. Called when a
/// `Text("content", "id")` is created so the platform UI code can map
/// the id → widget handle for later `setText` lookups.
pub type RegisterTextIdHandler =
    extern "C" fn(widget_handle: i64, id_ptr: *const u8, id_len: usize);

#[cfg(not(feature = "ohos-napi"))]
static SHOW_TOAST_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());
#[cfg(not(feature = "ohos-napi"))]
static SET_TEXT_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());
#[cfg(not(feature = "ohos-napi"))]
static REGISTER_TEXT_ID_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());

// --- Pending-call buffers ---
//
// Widget construction in user code happens at module-init time (before
// `app_run` calls our `js_register_*_handler` functions): every
// `Text("Count: 0", "counter")` immediately fires
// `perry_arkts_register_text_id(handle, id)`. If we discarded those
// calls when no handler was registered, the macOS-side id → handle map
// would never get populated and later `setText("counter", ...)` calls
// would silently no-op.
//
// Solution: queue each call when the handler slot is null. When the UI
// crate registers its handler at startup, the registration FFI drains
// the queue immediately, replaying every buffered call against the
// fresh handler. After drain, future calls go straight through.

static PENDING_TOASTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PENDING_SET_TEXTS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static PENDING_REGISTER_IDS: Mutex<Vec<(i64, String)>> = Mutex::new(Vec::new());

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_show_toast_handler(f: ShowToastHandler) {
    SHOW_TOAST_HANDLER.store(f as *mut (), Ordering::Release);
    // Drain any toasts queued before the UI lib finished initialising.
    let drained: Vec<String> = match PENDING_TOASTS.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for s in drained {
        let bytes = s.as_bytes();
        f(bytes.as_ptr(), bytes.len());
    }
}

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_set_text_handler(f: SetTextHandler) {
    SET_TEXT_HANDLER.store(f as *mut (), Ordering::Release);
    let drained: Vec<(String, String)> = match PENDING_SET_TEXTS.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for (id, val) in drained {
        let id_b = id.as_bytes();
        let val_b = val.as_bytes();
        f(id_b.as_ptr(), id_b.len(), val_b.as_ptr(), val_b.len());
    }
}

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_text_id_handler(f: RegisterTextIdHandler) {
    REGISTER_TEXT_ID_HANDLER.store(f as *mut (), Ordering::Release);
    let drained: Vec<(i64, String)> = match PENDING_REGISTER_IDS.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for (handle, id) in drained {
        let id_b = id.as_bytes();
        f(handle, id_b.as_ptr(), id_b.len());
    }
}

// On harmonyos, `arkts_callbacks::perry_arkts_show_toast` and
// `arkts_callbacks::perry_arkts_set_text` provide the canonical
// drain-queue implementations. We stub the registration FFIs so cross-
// platform UI crates that try to register handlers compile cleanly even
// on harmonyos builds (the ArkUI path doesn't need them — but leaving
// them undefined would break the link if a UI crate tried to register).
#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_show_toast_handler(_f: ShowToastHandler) {
    // No-op on harmonyos — drain-queue path in arkts_callbacks owns it.
}

#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_set_text_handler(_f: SetTextHandler) {}

#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_text_id_handler(_f: RegisterTextIdHandler) {}

/// Cross-platform `perry_arkts_show_toast` stub. Only compiled when the
/// `ohos-napi` feature is OFF — when it's ON, `arkts_callbacks.rs`
/// provides the canonical drain-queue implementation and this stub is
/// gated out so there's no symbol collision.
///
/// Calls before a handler is registered (i.e. during widget-tree
/// construction at module-init time, before `app_run` runs the UI
/// crate's `js_register_show_toast_handler` call) are buffered into
/// `PENDING_TOASTS`. The handler-registration FFI drains the buffer.
#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn perry_arkts_show_toast(msg_handle: f64) {
    let s = decode_jsvalue_string(msg_handle);
    let raw = SHOW_TOAST_HANDLER.load(Ordering::Acquire);
    if raw.is_null() {
        if let Ok(mut q) = PENDING_TOASTS.lock() {
            q.push(s);
        }
        return;
    }
    unsafe {
        let func: ShowToastHandler = std::mem::transmute(raw);
        let bytes = s.as_bytes();
        func(bytes.as_ptr(), bytes.len());
    }
}

/// Cross-platform `perry_arkts_set_text` stub. Same `ohos-napi` gating
/// + buffering shape as `perry_arkts_show_toast`.
#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn perry_arkts_set_text(id_handle: f64, val_handle: f64) {
    let id = decode_jsvalue_string(id_handle);
    let val = decode_jsvalue_string(val_handle);
    let raw = SET_TEXT_HANDLER.load(Ordering::Acquire);
    if raw.is_null() {
        if let Ok(mut q) = PENDING_SET_TEXTS.lock() {
            q.push((id, val));
        }
        return;
    }
    unsafe {
        let func: SetTextHandler = std::mem::transmute(raw);
        let id_bytes = id.as_bytes();
        let val_bytes = val.as_bytes();
        func(
            id_bytes.as_ptr(),
            id_bytes.len(),
            val_bytes.as_ptr(),
            val_bytes.len(),
        );
    }
}

// =============================================================================
// Issue #535 — `perry/ui` `state<T>` runtime registry.
// =============================================================================

static STATE_VALUES: Mutex<Option<std::collections::HashMap<String, f64>>> = Mutex::new(None);

fn with_state_values<F, R>(f: F) -> R
where
    F: FnOnce(&mut std::collections::HashMap<String, f64>) -> R,
{
    let mut guard = crate::gc::lock_gc_root_registry(&STATE_VALUES);
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    f(map)
}

#[no_mangle]
pub extern "C" fn js_state_init(id_handle: f64, initial: f64) {
    let id = decode_jsvalue_string(id_handle);
    with_state_values(|m| {
        m.insert(id, initial);
    });
}

#[no_mangle]
pub extern "C" fn js_state_get(id_handle: f64) -> f64 {
    let id = decode_jsvalue_string(id_handle);
    let undefined_bits: u64 = 0x7FFC_0000_0000_0001;
    with_state_values(|m| m.get(&id).copied()).unwrap_or_else(|| f64::from_bits(undefined_bits))
}

#[no_mangle]
pub extern "C" fn js_state_set(id_handle: f64, value: f64) {
    let id = decode_jsvalue_string(id_handle);
    with_state_values(|m| {
        m.insert(id.clone(), value);
    });
    #[cfg(not(feature = "ohos-napi"))]
    perry_arkts_set_text(id_handle, value);
    #[cfg(feature = "ohos-napi")]
    crate::arkts_callbacks::perry_arkts_set_text(id_handle, value);
    navstack_dispatch_state_change(&id, value);
    foreach_dispatch_state_change(&id, value);
}

// =============================================================================
// Issue #535 Layer 2 — `NavStack(state, routes)` runtime registry.
//
// Each `NavStack(state, [{name, body}, ...])` registers one entry per route
// here at App-build time. `js_state_set` walks the entry on every state
// change and fires the registered "set widget hidden" handler for each
// route — only the route whose name matches the new state value stays
// visible. The handler itself lives in the platform UI crate
// (perry-ui-macos / perry-ui-gtk4 / etc.) and is set via
// `js_register_widget_hidden_handler` at app startup; before registration
// the handler stays null and dispatch silently no-ops (the routes are
// still recorded so a later-registered handler picks up subsequent
// changes — same shape as `PENDING_*` queues for setText).
// =============================================================================

#[derive(Clone)]
struct NavRoute {
    name: String,
    handle: i64,
}

static NAVSTACK_REGISTRY: Mutex<Option<std::collections::HashMap<String, Vec<NavRoute>>>> =
    Mutex::new(None);

/// Set-widget-hidden handler signature. UI crates implement this on the
/// main thread (calls AppKit's `NSView.isHidden = ...` etc.).
pub type SetWidgetHiddenHandler = extern "C" fn(widget_handle: i64, hidden: i32);

#[cfg(not(feature = "ohos-napi"))]
static SET_WIDGET_HIDDEN_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_widget_hidden_handler(f: SetWidgetHiddenHandler) {
    SET_WIDGET_HIDDEN_HANDLER.store(f as *mut (), Ordering::Release);
    // Drain any NavStack routes that registered before the platform UI
    // crate hooked up its handler. `js_navstack_register_route` runs at
    // App-build time (inside the codegen-emitted IIFE); the platform
    // handler only registers later, inside `app_run`. Without this
    // drain, every "hide non-active route" call from the build-time
    // pass silently no-op'd against a null handler, leaving every route
    // body visible and overlapping (#612).
    let routes_snapshot: Option<Vec<(String, Vec<NavRoute>)>> = {
        let guard = match NAVSTACK_REGISTRY.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.as_ref().map(|m| {
            m.iter()
                .map(|(synth_id, rs)| (synth_id.clone(), rs.clone()))
                .collect()
        })
    };
    if let Some(routes_by_id) = routes_snapshot {
        for (synth_id, routes) in routes_by_id {
            let current_value = with_state_values(|m| m.get(&synth_id).copied());
            let Some(value_f64) = current_value else {
                continue;
            };
            let current_str = decode_jsvalue_string(value_f64);
            for route in routes {
                let hidden = if route.name == current_str { 0 } else { 1 };
                f(route.handle, hidden);
            }
        }
    }
}

#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_widget_hidden_handler(_f: SetWidgetHiddenHandler) {
    // No-op on harmonyos — ArkUI's `@State` decorator owns visibility
    // through the harvest's `setVisibility` NAPI bridge instead.
}

/// `__navstack_register_route("synth_id", "route_name", widget_handle)` —
/// records one route entry for the NavStack bound to `synth_id`. Called
/// once per route at App-build time (state_desugar's NavStack lowering
/// emits one call per route after evaluating the route body). Also sets
/// the route's initial visibility (hides the widget if its name doesn't
/// match the current value of `state(synth_id)`) so only the active
/// route is visible at first paint.
#[no_mangle]
pub extern "C" fn js_navstack_register_route(
    synth_id_handle: f64,
    route_name_handle: f64,
    widget_handle: i64,
) {
    let synth_id = decode_jsvalue_string(synth_id_handle);
    let route_name = decode_jsvalue_string(route_name_handle);
    {
        let mut guard = NAVSTACK_REGISTRY
            .lock()
            .expect("NAVSTACK_REGISTRY poisoned");
        let map = guard.get_or_insert_with(std::collections::HashMap::new);
        map.entry(synth_id.clone()).or_default().push(NavRoute {
            name: route_name.clone(),
            handle: widget_handle,
        });
    }
    // Initial visibility — match current state value (set by __state_init
    // earlier in the same module init order, so this is always populated
    // by the time NavStack registration fires).
    let current_value = with_state_values(|m| m.get(&synth_id).copied());
    if let Some(value_f64) = current_value {
        let current_str = decode_jsvalue_string(value_f64);
        if current_str != route_name {
            #[cfg(not(feature = "ohos-napi"))]
            {
                let raw = SET_WIDGET_HIDDEN_HANDLER.load(Ordering::Acquire);
                if !raw.is_null() {
                    let func: SetWidgetHiddenHandler = unsafe { std::mem::transmute(raw) };
                    func(widget_handle, 1);
                }
            }
        }
    }
}

/// Called by `js_state_set` after every state write. Walks any routes
/// registered for `synth_id`, toggling each route's visibility so only the
/// route whose `name` equals the new value stays visible. Compares the
/// new value against route names by string-decoding the f64 once.
#[cfg(not(feature = "ohos-napi"))]
fn navstack_dispatch_state_change(synth_id: &str, new_value: f64) {
    let routes: Vec<NavRoute> = {
        let guard = match NAVSTACK_REGISTRY.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.as_ref().and_then(|m| m.get(synth_id)) {
            Some(v) => v.clone(),
            None => return,
        }
    };
    let raw = SET_WIDGET_HIDDEN_HANDLER.load(Ordering::Acquire);
    if raw.is_null() {
        return;
    }
    let new_value_str = decode_jsvalue_string(new_value);
    let func: SetWidgetHiddenHandler = unsafe { std::mem::transmute(raw) };
    for route in &routes {
        let hidden = if route.name == new_value_str { 0 } else { 1 };
        func(route.handle, hidden);
    }
}

#[cfg(feature = "ohos-napi")]
fn navstack_dispatch_state_change(_synth_id: &str, _new_value: f64) {
    // No-op on harmonyos; the arkts harvest does its own setVisibility
    // dispatch through ArkUI's `@State` mechanism.
}

// =============================================================================
// Issue #610 — `perry/ui` `ForEach(state<number>, render)` runtime registry.
//
// Mirrors NAVSTACK_REGISTRY but for dynamic-list re-rendering. When the bound
// `State<number>` fires `.set(n)`, the platform UI crate re-invokes the user
// `render(i)` callback for each `i in [0..n)` and replaces the host
// container's children. The handler itself lives in the platform crate
// (perry-ui-macos / perry-ui-gtk4 / etc.) and is set via
// `js_register_foreach_render_handler` at app startup; before registration
// the handler stays null and dispatch silently no-ops (the binding is still
// recorded so a later-registered handler picks up subsequent changes).
// =============================================================================

#[derive(Clone)]
struct ForEachBinding {
    container_handle: i64,
    /// NaN-boxed closure pointer — the `(i: number) => Widget` callback.
    render_closure: f64,
}

static FOREACH_REGISTRY: Mutex<Option<std::collections::HashMap<String, Vec<ForEachBinding>>>> =
    Mutex::new(None);

/// GC root scanner for cross-platform UI state values and `ForEach` render
/// callbacks held in Rust registries outside the managed heap.
pub fn scan_ui_text_registry_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_ui_text_registry_roots_mut(&mut visitor);
}

pub fn scan_ui_text_registry_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    {
        let mut guard = crate::gc::lock_gc_root_registry(&STATE_VALUES);
        if let Some(values) = guard.as_mut() {
            for value in values.values_mut() {
                visitor.visit_nanbox_f64_slot(value);
            }
        }
    }
    {
        let mut guard = crate::gc::lock_gc_root_registry(&FOREACH_REGISTRY);
        if let Some(bindings_by_state) = guard.as_mut() {
            for bindings in bindings_by_state.values_mut() {
                for binding in bindings.iter_mut() {
                    visitor.visit_nanbox_f64_slot(&mut binding.render_closure);
                }
            }
        }
    }
}

/// Render-handler signature. UI crates implement this on the main thread:
/// clears the host's existing children, calls `render_closure(i)` for each
/// `i in [0..count)`, and inserts each returned widget. `count` is the
/// new state value (truncated to a non-negative integer).
pub type ForEachRenderHandler =
    extern "C" fn(container_handle: i64, render_closure: f64, count: f64);

#[cfg(not(feature = "ohos-napi"))]
static FOREACH_RENDER_HANDLER: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[cfg(not(feature = "ohos-napi"))]
#[no_mangle]
pub extern "C" fn js_register_foreach_render_handler(f: ForEachRenderHandler) {
    FOREACH_RENDER_HANDLER.store(f as *mut (), Ordering::Release);
}

#[cfg(feature = "ohos-napi")]
#[no_mangle]
pub extern "C" fn js_register_foreach_render_handler(_f: ForEachRenderHandler) {
    // No-op on harmonyos — ArkUI's `ForEach` directive uses the state
    // value through the `@State` decorator's automatic re-render path.
}

/// `__foreach_register("synth_id", container_handle, render_closure)` —
/// records one ForEach binding. Called once at App-build time when the
/// state_desugar pass rewrites `ForEach(stateBinding, render)` to its
/// register-and-paint IIFE form. Also paints the initial children
/// (matching the current count value).
#[no_mangle]
pub extern "C" fn js_foreach_register(
    synth_id_handle: f64,
    container_handle: i64,
    render_closure: f64,
) {
    let synth_id = decode_jsvalue_string(synth_id_handle);
    {
        let mut guard = FOREACH_REGISTRY.lock().expect("FOREACH_REGISTRY poisoned");
        let map = guard.get_or_insert_with(std::collections::HashMap::new);
        map.entry(synth_id.clone())
            .or_default()
            .push(ForEachBinding {
                container_handle,
                render_closure,
            });
    }
    // Initial paint — match current state value (set by __state_init
    // earlier in the same module init order, so always populated by the
    // time ForEach registration fires).
    let current_value = with_state_values(|m| m.get(&synth_id).copied());
    if let Some(value_f64) = current_value {
        #[cfg(not(feature = "ohos-napi"))]
        {
            let raw = FOREACH_RENDER_HANDLER.load(Ordering::Acquire);
            if !raw.is_null() {
                let func: ForEachRenderHandler = unsafe { std::mem::transmute(raw) };
                func(container_handle, render_closure, value_f64);
            }
        }
    }
}

/// Called by `js_state_set` after every state write. Walks any ForEach
/// bindings registered for `synth_id`, invoking each one's render handler
/// with the new count value.
#[cfg(not(feature = "ohos-napi"))]
fn foreach_dispatch_state_change(synth_id: &str, new_value: f64) {
    let bindings: Vec<ForEachBinding> = {
        let guard = crate::gc::lock_gc_root_registry(&FOREACH_REGISTRY);
        match guard.as_ref().and_then(|m| m.get(synth_id)) {
            Some(v) => v.clone(),
            None => return,
        }
    };
    let raw = FOREACH_RENDER_HANDLER.load(Ordering::Acquire);
    if raw.is_null() {
        return;
    }
    let func: ForEachRenderHandler = unsafe { std::mem::transmute(raw) };
    for b in &bindings {
        func(b.container_handle, b.render_closure, new_value);
    }
}

#[cfg(feature = "ohos-napi")]
fn foreach_dispatch_state_change(_synth_id: &str, _new_value: f64) {}

/// Cross-platform widget-id registration. Codegen at
/// `lower_call/native.rs` emits a call to this immediately after
/// `perry_ui_text_create` when the user wrote `Text("content", "id")`,
/// so the UI crate can map the id → widget handle for later `setText`
/// lookups.
///
/// Defined unconditionally (no `ohos-napi` gating) because the harmonyos
/// path uses a different mechanism — codegen-arkts emits the
/// `@State text_<id>: string = ...` declaration directly into the .ets
/// page, so the runtime never needs to track id → handle on harmonyos.
/// We still need the symbol to exist so non-arkts codegen can emit the
/// call without target-aware branching; it's just a no-op there.
///
/// Buffers calls before handler registration — see
/// `perry_arkts_show_toast` for the rationale.
#[no_mangle]
pub extern "C" fn perry_arkts_register_text_id(widget_handle: i64, id_handle: f64) {
    let id = decode_jsvalue_string(id_handle);
    #[cfg(feature = "ohos-napi")]
    {
        // ArkUI binds via @State decorators emitted by codegen-arkts; no
        // runtime registration needed. Drop the call.
        let _ = (widget_handle, id);
        return;
    }
    #[cfg(not(feature = "ohos-napi"))]
    {
        let raw = REGISTER_TEXT_ID_HANDLER.load(Ordering::Acquire);
        if raw.is_null() {
            if let Ok(mut q) = PENDING_REGISTER_IDS.lock() {
                q.push((widget_handle, id));
            }
            return;
        }
        unsafe {
            let func: RegisterTextIdHandler = std::mem::transmute(raw);
            let id_bytes = id.as_bytes();
            func(widget_handle, id_bytes.as_ptr(), id_bytes.len());
        }
    }
}

#[cfg(test)]
pub(crate) fn test_clear_ui_text_registry_roots() {
    *crate::gc::lock_gc_root_registry(&STATE_VALUES) = None;
    *crate::gc::lock_gc_root_registry(&FOREACH_REGISTRY) = None;
}

#[cfg(test)]
pub(crate) fn test_seed_ui_text_registry_roots(state_value: f64, render_closure: f64) {
    {
        let mut values = crate::gc::lock_gc_root_registry(&STATE_VALUES);
        let map = values.get_or_insert_with(std::collections::HashMap::new);
        map.clear();
        map.insert("state".to_string(), state_value);
    }
    {
        let mut registry = crate::gc::lock_gc_root_registry(&FOREACH_REGISTRY);
        let map = registry.get_or_insert_with(std::collections::HashMap::new);
        map.clear();
        map.insert(
            "state".to_string(),
            vec![ForEachBinding {
                container_handle: 1,
                render_closure,
            }],
        );
    }
}

#[cfg(test)]
pub(crate) fn test_ui_text_registry_roots_snapshot() -> (u64, u64) {
    let state = crate::gc::lock_gc_root_registry(&STATE_VALUES)
        .as_ref()
        .and_then(|values| values.get("state"))
        .copied()
        .map(f64::to_bits)
        .unwrap_or(0);
    let render = crate::gc::lock_gc_root_registry(&FOREACH_REGISTRY)
        .as_ref()
        .and_then(|bindings_by_state| bindings_by_state.get("state"))
        .and_then(|bindings| bindings.first())
        .map(|binding| binding.render_closure.to_bits())
        .unwrap_or(0);
    (state, render)
}
