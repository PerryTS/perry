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
    static PENDING_TIMERS: RefCell<Vec<(Duration, usize)>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_TIMERS: RefCell<Vec<DispatcherTimer>> = const { RefCell::new(Vec::new()) };
    static RUNTIME_PUMP_STARTED: RefCell<bool> = const { RefCell::new(false) };
}

fn is_fluent() -> bool {
    backend::active() == RenderBackend::Fluent
}

fn closure_ptr(value: f64) -> usize {
    unsafe { js_nanbox_get_pointer(value) as usize }
}

fn with_app_mut(handle: i64, f: impl FnOnce(&mut AppState)) {
    APPS.with(|apps| {
        if let Some(app) = apps.borrow_mut().get_mut(handle.saturating_sub(1) as usize) {
            f(app);
        }
    });
}

pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
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

    if let Some(callback) = ON_ACTIVATE.with(|slot| *slot.borrow()) {
        unsafe {
            js_closure_call0(callback as *const u8);
        }
    }

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
    if let Some(callback) = ON_TERMINATE.with(|slot| slot.borrow_mut().take()) {
        app = app.on_exit(move || unsafe {
            js_closure_call0(callback as *const u8);
        });
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
    PENDING_TIMERS.with(|timers| {
        timers.borrow_mut().push((
            Duration::from_secs_f64((interval_ms.max(1.0)) / 1000.0),
            closure_ptr(callback),
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
        PENDING_TIMERS.with(|pending| {
            for (interval, callback) in pending.borrow_mut().drain(..) {
                if let Ok(timer) = DispatcherTimer::new(interval, move || unsafe {
                    js_closure_call0(callback as *const u8);
                }) {
                    active.push(timer);
                }
            }
        });
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
