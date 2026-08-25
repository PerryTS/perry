//! WinUI application lifecycle adapter.

use std::cell::RefCell;
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows_reactor::winui::host::PresenterKind;
use windows_reactor::{App, Backdrop, DispatcherTimer, InnerConstraints};

use crate::winui::backend::{self, RenderBackend};

extern "C" {
    fn js_callback_timer_tick() -> i32;
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_frame_pump_default() -> i32;
    fn js_gc_step_us(budget_us: u64, out: *mut u8) -> u32;
    fn js_interval_timer_tick() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

#[derive(Clone, Default)]
struct AppState {
    title: String,
    width: f64,
    height: f64,
    root: i64,
    min_size: Option<(f64, f64)>,
    max_size: Option<(f64, f64)>,
    presenter: PresenterKind,
}

thread_local! {
    static APPS: RefCell<Vec<AppState>> = const { RefCell::new(Vec::new()) };
    static ON_ACTIVATE: RefCell<Option<usize>> = const { RefCell::new(None) };
    static ON_TERMINATE: RefCell<Option<usize>> = const { RefCell::new(None) };
    static TIMER_CALLBACKS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static PENDING_TIMERS: RefCell<Vec<(Duration, usize)>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_TIMERS: RefCell<Vec<DispatcherTimer>> = const { RefCell::new(Vec::new()) };
    static RUNTIME_PUMP_STARTED: RefCell<bool> = const { RefCell::new(false) };
}

/// Visit the JavaScript closures this module keeps alive across collections.
///
/// `APPS` is deliberately absent: `AppState` holds a Rust-owned `String`, two
/// `f64` window dimensions, an `i64` WIDGET handle (a 1-based index into
/// `widgets::NODES`, not an address), two optional size pairs and a
/// `PresenterKind` — no JS value, so it is not a GC root.
///
/// The lifecycle slots and `TIMER_CALLBACKS` are the sole owners of raw
/// callback pointers. Reactor closures capture stable keys and re-read these
/// scanned slots at invocation time, so an evacuating collection's rewritten
/// address is always observed. `PENDING_TIMERS` holds only durations and
/// indices into `TIMER_CALLBACKS`.
pub(crate) fn scan_winui_app_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    for slot in [&ON_ACTIVATE, &ON_TERMINATE] {
        slot.with(|slot| {
            if let Some(callback) = slot.borrow_mut().as_mut() {
                if *callback != 0 {
                    visitor.visit_usize_slot(callback);
                }
            }
        });
    }
    TIMER_CALLBACKS.with(|callbacks| {
        for callback in callbacks.borrow_mut().iter_mut() {
            if *callback != 0 {
                visitor.visit_usize_slot(callback);
            }
        }
    });
}

fn is_fluent() -> bool {
    backend::active() == RenderBackend::Fluent
}

fn closure_ptr(value: f64) -> usize {
    unsafe { js_nanbox_get_pointer(value) as usize }
}

fn app_callback(slot: &'static std::thread::LocalKey<RefCell<Option<usize>>>) -> usize {
    slot.with(|slot| slot.borrow().unwrap_or(0))
}

fn invoke_app_callback(slot: &'static std::thread::LocalKey<RefCell<Option<usize>>>) {
    let callback = app_callback(slot);
    if callback != 0 {
        unsafe {
            js_closure_call0(callback as *const u8);
        }
    }
}

fn timer_callback(key: usize) -> usize {
    TIMER_CALLBACKS.with(|callbacks| callbacks.borrow().get(key).copied().unwrap_or(0))
}

fn invoke_timer_callback(key: usize) {
    let callback = timer_callback(key);
    if callback != 0 {
        unsafe {
            js_closure_call0(callback as *const u8);
        }
    }
}

fn with_app_mut(handle: i64, f: impl FnOnce(&mut AppState)) {
    APPS.with(|apps| {
        if let Some(app) = apps.borrow_mut().get_mut(handle.saturating_sub(1) as usize) {
            f(app);
        }
    });
}

pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    crate::gc::ensure_registered();
    if !is_fluent() {
        return perry_ui_windows::app::app_create(title_ptr, width, height);
    }
    let title = unsafe { perry_ffi::copy_string_from_raw(title_ptr) }.to_owned();
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        apps.push(AppState {
            title,
            width,
            height,
            ..AppState::default()
        });
        apps.len() as i64
    })
}

pub fn app_set_body(app_handle: i64, root_handle: i64) {
    if !is_fluent() {
        perry_ui_windows::app::app_set_body(app_handle, root_handle);
        return;
    }
    with_app_mut(app_handle, |app| app.root = root_handle);
    crate::widgets::set_root(root_handle);
}

pub fn app_run(app_handle: i64) {
    if !is_fluent() {
        perry_ui_windows::app::app_run(app_handle);
        return;
    }

    let Some(state) = APPS.with(|apps| {
        apps.borrow()
            .get(app_handle.saturating_sub(1) as usize)
            .cloned()
    }) else {
        return;
    };
    crate::widgets::set_root(state.root);

    invoke_app_callback(&ON_ACTIVATE);

    let constraints = InnerConstraints {
        min_width: state.min_size.map(|v| v.0),
        min_height: state.min_size.map(|v| v.1),
        max_width: state.max_size.map(|v| v.0),
        max_height: state.max_size.map(|v| v.1),
    };
    let mut app = App::new()
        .title(state.title)
        .inner_size(state.width, state.height)
        .inner_constraints(constraints)
        .presenter(state.presenter)
        .backdrop(Backdrop::Mica);
    if app_callback(&ON_TERMINATE) != 0 {
        app = app.on_exit(move || invoke_app_callback(&ON_TERMINATE));
    }
    if let Err(error) = app.render(crate::widgets::render_root) {
        eprintln!("[perry-winui] application failed: {error}");
    }
}

pub fn get_dpi_scale() -> f64 {
    if is_fluent() {
        1.0
    } else {
        perry_ui_windows::app::get_dpi_scale()
    }
}

pub fn request_layout() {
    if is_fluent() {
        crate::widgets::request_render();
    } else {
        perry_ui_windows::app::request_layout();
    }
}

pub fn app_set_size(app_handle: i64, width: f64, height: f64) {
    if !is_fluent() {
        perry_ui_windows::app::app_set_size(app_handle, width, height);
        return;
    }
    with_app_mut(app_handle, |app| {
        app.width = width;
        app.height = height;
    });
}

pub fn set_min_size(app_handle: i64, width: f64, height: f64) {
    if !is_fluent() {
        perry_ui_windows::app::set_min_size(app_handle, width, height);
        return;
    }
    with_app_mut(app_handle, |app| app.min_size = Some((width, height)));
}

pub fn set_max_size(app_handle: i64, width: f64, height: f64) {
    if !is_fluent() {
        perry_ui_windows::app::set_max_size(app_handle, width, height);
        return;
    }
    with_app_mut(app_handle, |app| app.max_size = Some((width, height)));
}

pub fn set_window_state(app_handle: i64, value_ptr: *const u8) {
    if !is_fluent() {
        perry_ui_windows::app::set_window_state(app_handle, value_ptr);
        return;
    }
    let value = unsafe { perry_ffi::copy_string_from_raw(value_ptr) };
    let presenter = if value.eq_ignore_ascii_case("fullscreen") {
        PresenterKind::FullScreen
    } else {
        PresenterKind::Default
    };
    with_app_mut(app_handle, |app| app.presenter = presenter);
}

pub fn set_timer(interval_ms: f64, callback: f64) {
    if !is_fluent() {
        perry_ui_windows::app::set_timer(interval_ms, callback);
        return;
    }
    let callback_key = TIMER_CALLBACKS.with(|callbacks| {
        let mut callbacks = callbacks.borrow_mut();
        callbacks.push(closure_ptr(callback));
        callbacks.len() - 1
    });
    PENDING_TIMERS.with(|timers| {
        timers.borrow_mut().push((
            Duration::from_secs_f64((interval_ms.max(1.0)) / 1000.0),
            callback_key,
        ));
    });
}

pub(crate) fn start_runtime_pump() {
    if !is_fluent() {
        return;
    }
    let already_started = RUNTIME_PUMP_STARTED.with(|started| {
        let old = *started.borrow();
        *started.borrow_mut() = true;
        old
    });
    if already_started {
        return;
    }

    ACTIVE_TIMERS.with(|active| {
        let mut active = active.borrow_mut();
        if let Ok(timer) = DispatcherTimer::new(Duration::from_millis(16), || unsafe {
            js_callback_timer_tick();
            js_interval_timer_tick();
            js_frame_pump_default();
            js_gc_step_us(750, std::ptr::null_mut());
        }) {
            active.push(timer);
        }
        let pending = PENDING_TIMERS.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
        for (interval, callback_key) in pending {
            if let Ok(timer) =
                DispatcherTimer::new(interval, move || invoke_timer_callback(callback_key))
            {
                active.push(timer);
            }
        }
    });
}

pub fn on_activate(callback: f64) {
    if is_fluent() {
        ON_ACTIVATE.with(|slot| *slot.borrow_mut() = Some(closure_ptr(callback)));
    } else {
        perry_ui_windows::app::on_activate(callback);
    }
}

pub fn on_terminate(callback: f64) {
    if is_fluent() {
        ON_TERMINATE.with(|slot| *slot.borrow_mut() = Some(closure_ptr(callback)));
    } else {
        perry_ui_windows::app::on_terminate(callback);
    }
}

pub fn get_main_hwnd() -> Option<HWND> {
    if is_fluent() {
        None
    } else {
        perry_ui_windows::app::get_main_hwnd()
    }
}

pub fn add_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64) {
    perry_ui_windows::app::add_keyboard_shortcut(key_ptr, modifiers, callback);
}

pub fn register_global_hotkey(key_ptr: *const u8, modifiers: f64, callback: f64) {
    perry_ui_windows::app::register_global_hotkey(key_ptr, modifiers, callback);
}

pub fn get_app_icon(path_ptr: *const u8) -> i64 {
    perry_ui_windows::app::get_app_icon(path_ptr)
}

pub fn app_set_frameless(app_handle: i64, value: f64) {
    if !is_fluent() {
        perry_ui_windows::app::app_set_frameless(app_handle, value);
    }
}

pub fn app_set_level(app_handle: i64, value_ptr: *const u8) {
    if !is_fluent() {
        perry_ui_windows::app::app_set_level(app_handle, value_ptr);
    }
}

pub fn app_set_transparent(app_handle: i64, value: f64) {
    if !is_fluent() {
        perry_ui_windows::app::app_set_transparent(app_handle, value);
    }
}

pub fn app_set_vibrancy(app_handle: i64, value_ptr: *const u8) {
    if !is_fluent() {
        perry_ui_windows::app::app_set_vibrancy(app_handle, value_ptr);
    }
}

pub fn app_set_activation_policy(app_handle: i64, value_ptr: *const u8) {
    if !is_fluent() {
        perry_ui_windows::app::app_set_activation_policy(app_handle, value_ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactor_callback_keys_observe_rewritten_app_slots() {
        ON_ACTIVATE.with(|slot| *slot.borrow_mut() = Some(0x101));
        ON_TERMINATE.with(|slot| *slot.borrow_mut() = Some(0x111));
        assert_eq!(app_callback(&ON_ACTIVATE), 0x101);
        assert_eq!(app_callback(&ON_TERMINATE), 0x111);
        ON_ACTIVATE.with(|slot| *slot.borrow_mut() = Some(0x202));
        ON_TERMINATE.with(|slot| *slot.borrow_mut() = Some(0x222));
        assert_eq!(app_callback(&ON_ACTIVATE), 0x202);
        assert_eq!(app_callback(&ON_TERMINATE), 0x222);

        let key = TIMER_CALLBACKS.with(|callbacks| {
            let mut callbacks = callbacks.borrow_mut();
            callbacks.push(0x333);
            callbacks.len() - 1
        });
        assert_eq!(timer_callback(key), 0x333);
        TIMER_CALLBACKS.with(|callbacks| callbacks.borrow_mut()[key] = 0x444);
        assert_eq!(timer_callback(key), 0x444);
    }
}
