//! TTY module — Phase 3 of #347.
//!
//! Provides:
//!   - `tty.isatty(fd)` — bool (libc::isatty / GetConsoleMode probe on the
//!     std handle on Windows)
//!   - `process.std{in,out,err}.isTTY` — same as isatty(0/1/2)
//!   - `process.stdout.columns` / `.rows` — terminal dimensions via
//!     TIOCGWINSZ on Unix / GetConsoleScreenBufferInfo on Windows
//!   - `process.stdout.on('resize', cb)` — SIGWINCH handler that fires
//!     the registered callback when the terminal is resized (Unix only;
//!     Windows has no SIGWINCH — resize events need event-loop polling
//!     and are not wired up yet)
//!
//! All calls are synchronous and return `undefined` when stdout isn't a
//! TTY. The resize event handler is async-signal-safe (only sets an
//! atomic flag); the actual callback dispatch happens on the next
//! event-loop tick via `js_tty_resize_drain()`.

use crate::closure::{js_closure_call0, ClosureHeader};
use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::{JSValue, TAG_FALSE, TAG_TRUE, TAG_UNDEFINED};

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
#[cfg(unix)]
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Cross-thread state
// ---------------------------------------------------------------------------

/// Set by the SIGWINCH handler; cleared by the drain on each tick. The
/// handler is async-signal-safe (this is the only thing it touches).
static RESIZE_PENDING: AtomicBool = AtomicBool::new(false);
/// Cached last-known columns/rows. Re-read on every Columns/Rows call,
/// but the SIGWINCH handler also caches the new value here so the
/// drain sees up-to-date dimensions.
static CACHED_COLS: AtomicI32 = AtomicI32::new(0);
static CACHED_ROWS: AtomicI32 = AtomicI32::new(0);
/// Whether SIGWINCH has been installed yet (idempotent install).
static SIGWINCH_INSTALLED: AtomicBool = AtomicBool::new(false);
static TTY_PROTOTYPES_INITIALIZED: AtomicBool = AtomicBool::new(false);

pub const CLASS_ID_TTY_READ_STREAM: u32 = 0xFFFF_0084;
pub const CLASS_ID_TTY_WRITE_STREAM: u32 = 0xFFFF_0085;

const TAG_TRUE_F64: f64 = f64::from_bits(TAG_TRUE);
const TAG_FALSE_F64: f64 = f64::from_bits(TAG_FALSE);
const TAG_UNDEFINED_F64: f64 = f64::from_bits(TAG_UNDEFINED);

#[cfg(unix)]
static RAW_MODE_SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

thread_local! {
    /// Callback for `process.stdout.on('resize', cb)`. Stored on main
    /// thread; only touched by the drain (which runs on main).
    static RESIZE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Per-platform isatty + winsize
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn isatty_impl(fd: i32) -> bool {
    unsafe { libc::isatty(fd) != 0 }
}

#[cfg(unix)]
fn winsize_impl(fd: i32) -> Option<(i32, i32)> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            Some((ws.ws_col as i32, ws.ws_row as i32))
        } else {
            None
        }
    }
}

#[cfg(windows)]
fn isatty_impl(fd: i32) -> bool {
    // A std handle counts as a TTY when `GetConsoleMode` succeeds on it —
    // the same probe `std::io::IsTerminal` uses (see `builtins/console.rs`)
    // and the same criterion libuv uses to classify a handle as `UV_TTY`.
    // Piped/redirected handles fail the probe → false, matching Node.
    crate::win_console::is_console_fd(fd)
}

#[cfg(windows)]
fn winsize_impl(fd: i32) -> Option<(i32, i32)> {
    // GetConsoleScreenBufferInfo srWindow extents (right-left+1,
    // bottom-top+1) for the stdout/stderr console handle.
    crate::win_console::window_size(fd)
}

#[cfg(not(any(unix, windows)))]
fn isatty_impl(_fd: i32) -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
fn winsize_impl(_fd: i32) -> Option<(i32, i32)> {
    None
}

pub(crate) fn is_tty_fd(fd: i32) -> bool {
    isatty_impl(fd)
}

fn validate_fd_number(fd: f64) -> i32 {
    if !fd.is_finite() || fd < 0.0 || fd.fract() != 0.0 {
        throw_invalid_fd(fd);
    }
    fd as i32
}

fn tty_isatty_fd_arg(value: f64) -> Option<i32> {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_int32() {
        let fd = js_value.as_int32();
        return (fd >= 0).then_some(fd);
    }
    if !js_value.is_number() {
        return None;
    }

    let fd = js_value.as_number();
    if !fd.is_finite() || fd < 0.0 || fd.fract() != 0.0 || fd > i32::MAX as f64 {
        return None;
    }
    Some(fd as i32)
}

#[cfg(unix)]
fn write_stream_fd_type_supported(fd: i32) -> bool {
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &mut stat) != 0 {
            return false;
        }
        let file_type = stat.st_mode & libc::S_IFMT;
        file_type == libc::S_IFIFO || file_type == libc::S_IFSOCK
    }
}

#[cfg(not(unix))]
fn write_stream_fd_type_supported(_fd: i32) -> bool {
    false
}

fn can_init_write_stream_fd(fd: i32) -> bool {
    is_tty_fd(fd) || write_stream_fd_type_supported(fd)
}

fn validate_tty_read_fd(fd: f64) -> i32 {
    let fd_i = validate_fd_number(fd);
    if !is_tty_fd(fd_i) {
        throw_tty_init_failed();
    }
    fd_i
}

fn validate_tty_write_fd(fd: f64) -> i32 {
    let fd_i = validate_fd_number(fd);
    if !can_init_write_stream_fd(fd_i) {
        throw_tty_init_failed();
    }
    fd_i
}

fn js_bool(value: bool) -> f64 {
    if value {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

fn ptr_value(ptr: *mut ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

fn named_key(name: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn heap_object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let ptr = jsval.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            Some(ptr as *mut ObjectHeader)
        } else {
            None
        }
    }
}

fn this_value() -> f64 {
    crate::object::js_implicit_this_get()
}

fn this_object() -> Option<*mut ObjectHeader> {
    heap_object_ptr(this_value())
}

fn fd_from_object(obj: *mut ObjectHeader) -> Option<i32> {
    let fd = crate::object::js_object_get_field_by_name_f64(obj, named_key(b"fd"));
    let jsval = JSValue::from_bits(fd.to_bits());
    if jsval.is_int32() {
        return Some(jsval.as_int32());
    }
    if jsval.is_number() && fd.is_finite() && fd.fract() == 0.0 {
        return Some(fd as i32);
    }
    None
}

fn current_fd(default_fd: i32) -> i32 {
    this_object().and_then(fd_from_object).unwrap_or(default_fd)
}

fn this_has_fd() -> bool {
    this_object().and_then(fd_from_object).is_some()
}

fn value_to_string(value: f64) -> Option<String> {
    crate::builtins::jsvalue_string_content(value)
}

fn event_name_from_value(value: f64) -> Option<String> {
    value_to_string(value)
}

fn callback_ptr_from_value(value: f64) -> i64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return 0;
    }
    (value.to_bits() & crate::value::POINTER_MASK) as i64
}

// ---------------------------------------------------------------------------
// SIGWINCH handler (Unix only)
// ---------------------------------------------------------------------------

#[cfg(unix)]
extern "C" fn sigwinch_handler(_sig: libc::c_int) {
    // Async-signal-safe: ONLY set the atomic flag. Don't do ioctl here
    // (TIOCGWINSZ is technically AS-safe but tradition says no), don't
    // touch JS state, don't allocate. The drain reads the flag and
    // does the real work on the next event-loop tick.
    RESIZE_PENDING.store(true, Ordering::Release);
}

#[cfg(unix)]
fn install_sigwinch() {
    if SIGWINCH_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigwinch_handler as *const () as usize;
        // SA_RESTART so a stray SIGWINCH during a `read` doesn't return
        // EINTR — important for the readline byte-mode reader.
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());
    }
}

#[cfg(not(unix))]
fn install_sigwinch() {}

#[cfg(unix)]
fn set_fd_raw_mode(fd: i32, enabled: bool) -> bool {
    unsafe {
        if enabled {
            let mut current: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut current) != 0 {
                return false;
            }
            {
                let mut saved = RAW_MODE_SAVED.lock().unwrap();
                if saved.is_none() {
                    *saved = Some(current);
                }
            }
            let mut raw = current;
            raw.c_iflag &= !(libc::IGNBRK
                | libc::BRKINT
                | libc::PARMRK
                | libc::ISTRIP
                | libc::INLCR
                | libc::IGNCR
                | libc::ICRNL
                | libc::IXON);
            raw.c_oflag &= !libc::OPOST;
            raw.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
            raw.c_cflag &= !(libc::CSIZE | libc::PARENB);
            raw.c_cflag |= libc::CS8;
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(fd, libc::TCSANOW, &raw) == 0
        } else {
            let saved = RAW_MODE_SAVED.lock().unwrap();
            if let Some(termios) = saved.as_ref() {
                libc::tcsetattr(fd, libc::TCSANOW, termios) == 0
            } else {
                true
            }
        }
    }
}

#[cfg(windows)]
fn set_fd_raw_mode(fd: i32, enabled: bool) -> bool {
    // Only the console *input* handle can be flipped raw on Windows, and
    // fd 0 is the only fd that maps to it via GetStdHandle. The shared
    // helper mirrors the readline.rs / tui/input.rs flag handling (#406):
    // clear ENABLE_{LINE,ECHO,PROCESSED}_INPUT, set
    // ENABLE_VIRTUAL_TERMINAL_INPUT so arrow keys arrive as ANSI
    // `\x1b[A..D`, save the cooked mode on first enable, restore on
    // disable. Fails (returns false) when stdin is piped/redirected.
    if fd != 0 {
        return false;
    }
    crate::win_console::set_stdin_raw(enabled)
}

#[cfg(not(any(unix, windows)))]
fn set_fd_raw_mode(_fd: i32, _enabled: bool) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Public FFI
// ---------------------------------------------------------------------------

/// `tty.isatty(fd)` — return 1 if the fd refers to a terminal.
#[no_mangle]
pub extern "C" fn js_tty_isatty(fd: f64) -> f64 {
    let Some(fd_i) = tty_isatty_fd_arg(fd) else {
        return TAG_FALSE_F64;
    };
    if isatty_impl(fd_i) {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

#[derive(Clone, Copy)]
struct ColorEnv<'a> {
    object: Option<*const ObjectHeader>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl ColorEnv<'_> {
    fn from_value(value: f64) -> Self {
        Self {
            object: heap_object_ptr(value).map(|p| p as *const ObjectHeader),
            _marker: std::marker::PhantomData,
        }
    }

    fn process() -> Self {
        Self {
            object: None,
            _marker: std::marker::PhantomData,
        }
    }

    fn get(&self, name: &str) -> Option<String> {
        if let Some(obj) = self.object {
            let value =
                crate::object::js_object_get_field_by_name_f64(obj, named_key(name.as_bytes()));
            let jsval = JSValue::from_bits(value.to_bits());
            if jsval.is_undefined() || jsval.is_null() {
                return None;
            }
            if let Some(s) = value_to_string(value) {
                return Some(s);
            }
            if jsval.is_bool() {
                return Some(jsval.as_bool().to_string());
            }
            if jsval.is_int32() {
                return Some(jsval.as_int32().to_string());
            }
            if jsval.is_number() && value.is_finite() {
                return Some(value.to_string());
            }
            return Some("[object Object]".to_string());
        }
        std::env::var(name).ok()
    }
}

fn color_depth_from_lookup<F>(mut get: F) -> u8
where
    F: FnMut(&str) -> Option<String>,
{
    let has = |name: &str, get: &mut F| get(name).is_some();

    if let Some(force) = get("FORCE_COLOR") {
        return match force.as_str() {
            "" | "1" | "true" => 4,
            "2" => 8,
            "3" => 24,
            _ => 1,
        };
    }

    if has("NODE_DISABLE_COLORS", &mut get)
        || has("NO_COLOR", &mut get)
        || get("TERM").as_deref() == Some("dumb")
    {
        return 1;
    }

    if has("TMUX", &mut get) {
        return 24;
    }

    if has("TF_BUILD", &mut get) && has("AGENT_NAME", &mut get) {
        return 4;
    }

    if has("CI", &mut get) {
        if has("CIRCLECI", &mut get)
            || has("GITEA_ACTIONS", &mut get)
            || has("GITHUB_ACTIONS", &mut get)
        {
            return 24;
        }
        if has("APPVEYOR", &mut get)
            || has("BUILDKITE", &mut get)
            || has("DRONE", &mut get)
            || has("GITLAB_CI", &mut get)
            || has("TRAVIS", &mut get)
        {
            return 8;
        }
        if get("CI_NAME").as_deref() == Some("codeship") {
            return 8;
        }
        return 1;
    }

    if let Some(teamcity) = get("TEAMCITY_VERSION") {
        let mut parts = teamcity.split('.');
        let major = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let minor = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        return if major > 9 || (major == 9 && minor >= 1) {
            4
        } else {
            1
        };
    }

    match get("TERM_PROGRAM").as_deref() {
        Some("iTerm.app") => {
            if get("TERM_PROGRAM_VERSION")
                .as_deref()
                .is_some_and(|version| {
                    version.starts_with("0.")
                        || version.starts_with("1.")
                        || version.starts_with("2.")
                })
            {
                return 8;
            }
            return 24;
        }
        Some("HyperTerm") | Some("MacTerm") => return 24,
        Some("Apple_Terminal") => return 8,
        _ => {}
    }

    if matches!(get("COLORTERM").as_deref(), Some("truecolor" | "24bit")) {
        return 24;
    }

    if let Some(term) = get("TERM") {
        let term_lower = term.to_ascii_lowercase();
        if term_lower.contains("truecolor") {
            return 24;
        }
        if term_lower.starts_with("xterm-256") {
            return 8;
        }
        let exact_depth = match term_lower.as_str() {
            "eterm" | "cons25" | "console" | "cygwin" | "dtterm" | "gnome" | "hurd" | "jfbterm"
            | "konsole" | "kterm" | "mlterm" | "putty" | "st" => Some(4),
            "mosh" | "rxvt-unicode-24bit" | "terminator" | "xterm-kitty" => Some(24),
            _ => None,
        };
        if let Some(depth) = exact_depth {
            return depth;
        }
        if term_lower.contains("ansi")
            || term_lower.contains("color")
            || term_lower.contains("linux")
            || term_lower.contains("direct")
            || term_lower.starts_with("rxvt")
            || term_lower.starts_with("screen")
            || term_lower.starts_with("xterm")
            || term_lower.starts_with("vt100")
            || term_lower.starts_with("vt220")
            || (term_lower.starts_with("con") && term_lower.contains('x'))
        {
            return 4;
        }
    }

    if get("COLORTERM").is_some() {
        return 4;
    }

    1
}

fn color_depth_for_env(env: ColorEnv<'_>) -> u8 {
    color_depth_from_lookup(|name| env.get(name))
}

fn has_colors_for_depth(count: f64, depth: u8) -> bool {
    count <= 2_f64.powi(depth as i32)
}

fn count_value(count: f64) -> Result<f64, &'static str> {
    let jsval = JSValue::from_bits(count.to_bits());
    if jsval.is_int32() {
        return Ok(jsval.as_int32() as f64);
    }
    if jsval.is_number() {
        return Ok(jsval.as_number());
    }
    Err("type")
}

fn display_number(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "Infinity".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn validate_color_count(count: f64) -> f64 {
    let number = match count_value(count) {
        Ok(number) => number,
        Err(_) => {
            let message = format!(
                "The \"count\" argument must be of type number. Received {}",
                crate::fs::validate::describe_received(count)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        }
    };
    if !number.is_finite() || number.fract() != 0.0 {
        let message = format!(
            "The value of \"count\" is out of range. It must be an integer. Received {}",
            display_number(number)
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    if !(2.0..=9_007_199_254_740_991.0).contains(&number) {
        let message = format!(
            "The value of \"count\" is out of range. It must be >= 2 && <= 9007199254740991. Received {}",
            display_number(number)
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    number
}

/// Node-compatible `tty.WriteStream.prototype.getColorDepth([env])`.
#[no_mangle]
pub extern "C" fn js_tty_write_stream_get_color_depth(
    _closure: *const ClosureHeader,
    env: f64,
) -> f64 {
    let jsval = JSValue::from_bits(env.to_bits());
    let env = if jsval.is_undefined() {
        ColorEnv::process()
    } else {
        ColorEnv::from_value(env)
    };
    color_depth_for_env(env) as f64
}

/// Node-compatible `tty.WriteStream.prototype.hasColors([count][, env])`.
#[no_mangle]
pub extern "C" fn js_tty_write_stream_has_colors(
    _closure: *const ClosureHeader,
    count: f64,
    env: f64,
) -> f64 {
    let count_js = JSValue::from_bits(count.to_bits());
    let env_js = JSValue::from_bits(env.to_bits());
    let (count, env) = if env_js.is_undefined()
        && (count_js.is_undefined() || (count_js.is_pointer() && heap_object_ptr(count).is_some()))
    {
        (
            16.0,
            if count_js.is_undefined() {
                ColorEnv::process()
            } else {
                ColorEnv::from_value(count)
            },
        )
    } else {
        let count = validate_color_count(count);
        (
            count,
            if env_js.is_undefined() {
                ColorEnv::process()
            } else {
                ColorEnv::from_value(env)
            },
        )
    };
    js_bool(has_colors_for_depth(count, color_depth_for_env(env)))
}

fn closure_value(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn ensure_tty_prototypes() {
    if TTY_PROTOTYPES_INITIALIZED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let read_keys = b"constructor\0setRawMode\0";
    let read_proto = crate::object::js_object_alloc_with_shape(
        0x7FFF_FF31,
        2,
        read_keys.as_ptr(),
        read_keys.len() as u32,
    );
    unsafe {
        crate::object::js_object_set_field(
            read_proto,
            1,
            JSValue::from_bits(
                closure_value(
                    js_tty_read_stream_set_raw_mode as *const u8,
                    "setRawMode",
                    1,
                )
                .to_bits(),
            ),
        );
    }
    crate::object::class_prototype_object_root_store(CLASS_ID_TTY_READ_STREAM, read_proto);

    let write_keys = b"constructor\0isTTY\0getColorDepth\0hasColors\0_refreshSize\0cursorTo\0moveCursor\0clearLine\0clearScreenDown\0getWindowSize\0";
    let write_proto = crate::object::js_object_alloc_with_shape(
        0x7FFF_FF32,
        10,
        write_keys.as_ptr(),
        write_keys.len() as u32,
    );
    unsafe {
        crate::object::js_object_set_field(write_proto, 1, JSValue::from_bits(TAG_TRUE));
        crate::object::js_object_set_field(
            write_proto,
            2,
            JSValue::from_bits(
                closure_value(
                    js_tty_write_stream_get_color_depth as *const u8,
                    "getColorDepth",
                    1,
                )
                .to_bits(),
            ),
        );
        crate::object::js_object_set_field(
            write_proto,
            3,
            JSValue::from_bits(
                closure_value(js_tty_write_stream_has_colors as *const u8, "hasColors", 2)
                    .to_bits(),
            ),
        );
        crate::object::js_object_set_field(
            write_proto,
            4,
            JSValue::from_bits(
                closure_value(
                    js_tty_write_stream_refresh_size as *const u8,
                    "_refreshSize",
                    0,
                )
                .to_bits(),
            ),
        );
        crate::object::js_object_set_field(
            write_proto,
            5,
            JSValue::from_bits(
                closure_value(js_tty_write_stream_cursor_to as *const u8, "cursorTo", 3).to_bits(),
            ),
        );
        crate::object::js_object_set_field(
            write_proto,
            6,
            JSValue::from_bits(
                closure_value(
                    js_tty_write_stream_move_cursor as *const u8,
                    "moveCursor",
                    3,
                )
                .to_bits(),
            ),
        );
        crate::object::js_object_set_field(
            write_proto,
            7,
            JSValue::from_bits(
                closure_value(js_tty_write_stream_clear_line as *const u8, "clearLine", 2)
                    .to_bits(),
            ),
        );
        crate::object::js_object_set_field(
            write_proto,
            8,
            JSValue::from_bits(
                closure_value(
                    js_tty_write_stream_clear_screen_down as *const u8,
                    "clearScreenDown",
                    1,
                )
                .to_bits(),
            ),
        );
        crate::object::js_object_set_field(
            write_proto,
            9,
            JSValue::from_bits(
                closure_value(
                    js_tty_write_stream_get_window_size as *const u8,
                    "getWindowSize",
                    0,
                )
                .to_bits(),
            ),
        );
    }
    crate::object::class_prototype_object_root_store(CLASS_ID_TTY_WRITE_STREAM, write_proto);
}

pub(crate) fn attach_tty_constructor_prototype(constructor_value: f64, name: &str) {
    ensure_tty_prototypes();
    let (class_id, method_index, method_value) = if name == "WriteStream" {
        (CLASS_ID_TTY_WRITE_STREAM, None, None)
    } else {
        (
            CLASS_ID_TTY_READ_STREAM,
            Some(1),
            Some(closure_value(
                js_tty_read_stream_set_raw_mode as *const u8,
                "setRawMode",
                1,
            )),
        )
    };
    let proto = crate::object::class_prototype_object(class_id);
    if proto.is_null() {
        return;
    }
    unsafe {
        crate::object::js_object_set_field(
            proto,
            0,
            JSValue::from_bits(constructor_value.to_bits()),
        );
        if let (Some(index), Some(value)) = (method_index, method_value) {
            crate::object::js_object_set_field(proto, index, JSValue::from_bits(value.to_bits()));
        }
    }
    crate::closure::closure_set_dynamic_prop(
        (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
    );
}

fn add_write_stream_listener_fields(obj: *mut ObjectHeader) {
    let on = tty_listener_on_value();
    let remove = tty_listener_remove_value();
    let remove_all = tty_listener_remove_all_value();
    crate::object::js_object_set_field_by_name(obj, named_key(b"on"), on);
    crate::object::js_object_set_field_by_name(obj, named_key(b"addListener"), on);
    crate::object::js_object_set_field_by_name(obj, named_key(b"once"), on);
    crate::object::js_object_set_field_by_name(obj, named_key(b"removeListener"), remove);
    crate::object::js_object_set_field_by_name(obj, named_key(b"off"), remove);
    crate::object::js_object_set_field_by_name(obj, named_key(b"removeAllListeners"), remove_all);
}

pub(crate) fn tty_listener_on_value() -> f64 {
    closure_value(js_tty_write_stream_on as *const u8, "on", 2)
}

pub(crate) fn tty_listener_remove_value() -> f64 {
    closure_value(
        js_tty_write_stream_remove_listener as *const u8,
        "removeListener",
        2,
    )
}

pub(crate) fn tty_listener_remove_all_value() -> f64 {
    closure_value(
        js_tty_write_stream_remove_all_listeners as *const u8,
        "removeAllListeners",
        1,
    )
}

#[no_mangle]
pub extern "C" fn js_tty_read_stream_new(fd: f64) -> f64 {
    validate_tty_read_fd(fd);
    ensure_tty_prototypes();
    let keys = b"isRaw\0isTTY\0";
    let obj = crate::object::js_object_alloc_class_with_keys(
        CLASS_ID_TTY_READ_STREAM,
        0,
        2,
        keys.as_ptr(),
        keys.len() as u32,
    );
    unsafe {
        crate::object::js_object_set_field(obj, 0, JSValue::from_bits(TAG_FALSE));
        crate::object::js_object_set_field(obj, 1, JSValue::from_bits(TAG_TRUE));
    }
    ptr_value(obj)
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_new(fd: f64) -> f64 {
    validate_tty_write_fd(fd);
    ensure_tty_prototypes();
    let obj = crate::object::js_object_alloc_class_with_keys(
        CLASS_ID_TTY_WRITE_STREAM,
        0,
        0,
        std::ptr::null(),
        0,
    );
    add_write_stream_listener_fields(obj);
    ptr_value(obj)
}

pub(crate) fn is_tty_stream_instance(value: f64, class_name: &str) -> bool {
    let Some(obj) = heap_object_ptr(value) else {
        return false;
    };
    let class_id = unsafe { (*obj).class_id };
    matches!(
        (class_name, class_id),
        ("ReadStream", CLASS_ID_TTY_READ_STREAM) | ("WriteStream", CLASS_ID_TTY_WRITE_STREAM)
    )
}

#[no_mangle]
pub extern "C" fn js_tty_read_stream_set_raw_mode(
    _closure: *const ClosureHeader,
    mode: f64,
) -> f64 {
    let enabled = crate::value::js_is_truthy(mode) != 0;
    let fd = current_fd(0);
    let _ = set_fd_raw_mode(fd, enabled);
    if let Some(obj) = this_object() {
        crate::object::js_object_set_field_by_name(obj, named_key(b"isRaw"), js_bool(enabled));
    }
    this_value()
}

fn write_all_fd(fd: i32, bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    #[cfg(unix)]
    unsafe {
        let mut written = 0usize;
        while written < bytes.len() {
            let n = libc::write(
                fd,
                bytes[written..].as_ptr() as *const libc::c_void,
                bytes.len() - written,
            );
            if n <= 0 {
                return false;
            }
            written += n as usize;
        }
        true
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        let ok = if fd == 2 {
            std::io::stderr().lock().write_all(bytes).is_ok()
        } else {
            std::io::stdout().lock().write_all(bytes).is_ok()
        };
        ok
    }
}

fn numeric_arg(value: f64, default: i32) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_int32() {
        return jsval.as_int32();
    }
    if jsval.is_number() && value.is_finite() {
        return value as i32;
    }
    default
}

fn write_control_sequence(sequence: String) -> f64 {
    js_bool(write_all_fd(current_fd(1), sequence.as_bytes()))
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_cursor_to(
    _closure: *const ClosureHeader,
    x: f64,
    y: f64,
    _callback: f64,
) -> f64 {
    let x = numeric_arg(x, 0).max(0);
    let y_js = JSValue::from_bits(y.to_bits());
    let sequence = if y_js.is_undefined() {
        format!("\x1b[{}G", x + 1)
    } else {
        let y = numeric_arg(y, 0).max(0);
        format!("\x1b[{};{}H", y + 1, x + 1)
    };
    write_control_sequence(sequence)
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_move_cursor(
    _closure: *const ClosureHeader,
    dx: f64,
    dy: f64,
    _callback: f64,
) -> f64 {
    let dx = numeric_arg(dx, 0);
    let dy = numeric_arg(dy, 0);
    let mut sequence = String::new();
    if dx < 0 {
        sequence.push_str(&format!("\x1b[{}D", -dx));
    } else if dx > 0 {
        sequence.push_str(&format!("\x1b[{}C", dx));
    }
    if dy < 0 {
        sequence.push_str(&format!("\x1b[{}A", -dy));
    } else if dy > 0 {
        sequence.push_str(&format!("\x1b[{}B", dy));
    }
    write_control_sequence(sequence)
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_clear_line(
    _closure: *const ClosureHeader,
    dir: f64,
    _callback: f64,
) -> f64 {
    let mode = match numeric_arg(dir, 0) {
        -1 => 1,
        1 => 0,
        _ => 2,
    };
    write_control_sequence(format!("\x1b[{}K", mode))
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_clear_screen_down(
    _closure: *const ClosureHeader,
    _callback: f64,
) -> f64 {
    write_control_sequence("\x1b[0J".to_string())
}

fn winsize_value(fd: i32) -> Option<(i32, i32)> {
    winsize_impl(fd).inspect(|(cols, rows)| {
        CACHED_COLS.store(*cols, Ordering::Relaxed);
        CACHED_ROWS.store(*rows, Ordering::Relaxed);
    })
}

pub(crate) fn tty_write_stream_dimension(property: &str) -> Option<f64> {
    let (cols, rows) = winsize_value(1)?;
    match property {
        "columns" => Some(cols as f64),
        "rows" => Some(rows as f64),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_get_window_size(_closure: *const ClosureHeader) -> f64 {
    let fd = current_fd(1);
    let arr = crate::array::js_array_alloc(2);
    let (cols, rows) = winsize_value(fd).unwrap_or((0, 0));
    let col_value = if cols > 0 {
        cols as f64
    } else {
        TAG_UNDEFINED_F64
    };
    let row_value = if rows > 0 {
        rows as f64
    } else {
        TAG_UNDEFINED_F64
    };
    let arr = crate::array::js_array_push_f64(arr, col_value);
    let arr = crate::array::js_array_push_f64(arr, row_value);
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_refresh_size(_closure: *const ClosureHeader) -> f64 {
    let _ = winsize_value(current_fd(1));
    TAG_UNDEFINED_F64
}

fn register_resize_callback(callback: i64) {
    RESIZE_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
    install_sigwinch();
    if let Some((cols, rows)) = winsize_impl(1) {
        CACHED_COLS.store(cols, Ordering::Relaxed);
        CACHED_ROWS.store(rows, Ordering::Relaxed);
    }
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_on(
    _closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    if event_name_from_value(event).as_deref() == Some("resize") && this_has_fd() {
        let callback = callback_ptr_from_value(callback);
        if callback != 0 {
            register_resize_callback(callback);
        }
    }
    this_value()
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_remove_listener(
    _closure: *const ClosureHeader,
    event: f64,
    _callback: f64,
) -> f64 {
    if event_name_from_value(event).as_deref() == Some("resize") && this_has_fd() {
        RESIZE_CALLBACK.with(|cb| *cb.borrow_mut() = None);
    }
    this_value()
}

#[no_mangle]
pub extern "C" fn js_tty_write_stream_remove_all_listeners(
    _closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let event_js = JSValue::from_bits(event.to_bits());
    if (event_js.is_undefined() || event_name_from_value(event).as_deref() == Some("resize"))
        && this_has_fd()
    {
        RESIZE_CALLBACK.with(|cb| *cb.borrow_mut() = None);
    }
    this_value()
}

pub fn throw_invalid_fd(fd: f64) -> ! {
    let obj = crate::object::js_object_alloc(crate::error::CLASS_ID_RANGE_ERROR, 4);
    unsafe {
        crate::object::js_register_class_extends_error(crate::error::CLASS_ID_RANGE_ERROR);
        let str_val = |s: &str| -> f64 {
            let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        };
        let set = |key: &str, value: f64| {
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key_ptr, value);
        };
        set("name", str_val("RangeError"));
        set("code", str_val("ERR_INVALID_FD"));
        set(
            "message",
            str_val(&format!("\"fd\" must be a positive integer: {}", fd as i64)),
        );
    }
    crate::exception::js_throw(crate::value::js_nanbox_pointer(obj as i64))
}

pub fn throw_tty_init_failed() -> ! {
    let message = "TTY initialization failed: uv_tty_init returned EINVAL (invalid argument)";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_TTY_INIT_FAILED");
    let err = crate::error::js_error_new_with_name_message(b"SystemError", msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// `process.stdin.isTTY` / `process.stdout.isTTY` / `process.stderr.isTTY`.
/// Per Node's docs these are `true` when the stream is a TTY and
/// `undefined` otherwise (intentionally — it's a presence test). This
/// differs from `tty.isatty(fd)`, which always returns a boolean.
#[no_mangle]
pub extern "C" fn js_process_stdin_isatty() -> f64 {
    if isatty_impl(0) {
        f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
    } else {
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
    }
}
#[no_mangle]
pub extern "C" fn js_process_stdout_isatty() -> f64 {
    if isatty_impl(1) {
        f64::from_bits(0x7FFC_0000_0000_0004)
    } else {
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}
#[no_mangle]
pub extern "C" fn js_process_stderr_isatty() -> f64 {
    if isatty_impl(2) {
        f64::from_bits(0x7FFC_0000_0000_0004)
    } else {
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}

/// `process.stdout.columns` — terminal width in cells, or `undefined`
/// when stdout isn't a TTY.
#[no_mangle]
pub extern "C" fn js_process_stdout_columns() -> f64 {
    match winsize_impl(1) {
        Some((cols, rows)) => {
            CACHED_COLS.store(cols, Ordering::Relaxed);
            CACHED_ROWS.store(rows, Ordering::Relaxed);
            cols as f64
        }
        None => f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
    }
}

/// `process.stdout.rows` — terminal height in cells, or `undefined`
/// when stdout isn't a TTY.
#[no_mangle]
pub extern "C" fn js_process_stdout_rows() -> f64 {
    match winsize_impl(1) {
        Some((cols, rows)) => {
            CACHED_COLS.store(cols, Ordering::Relaxed);
            CACHED_ROWS.store(rows, Ordering::Relaxed);
            rows as f64
        }
        None => f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
    }
}

/// `process.stdout.on(event, cb)` — currently only handles `'resize'`,
/// which installs SIGWINCH and stashes the callback. Other events are
/// silently ignored (they're not currently supported on stdout).
#[no_mangle]
pub extern "C" fn js_process_stdout_on(event_ptr: *const StringHeader, callback: i64) -> f64 {
    if event_ptr.is_null() {
        return crate::os::js_process_stdout();
    }
    let event = unsafe {
        let len = (*event_ptr).byte_len as usize;
        let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    if event == "resize" {
        register_resize_callback(callback);
    }
    crate::os::js_process_stdout()
}

/// Drain the resize-pending flag and fire the registered resize
/// callback. Called from the event-loop pump on every tick. Returns
/// the number of callbacks fired (0 or 1).
#[no_mangle]
pub extern "C" fn js_tty_resize_drain() -> i32 {
    if !RESIZE_PENDING.swap(false, Ordering::AcqRel) {
        return 0;
    }
    // Refresh the cache before firing — the callback typically reads
    // process.stdout.columns/.rows, and we want it to see the new
    // values, not the pre-resize values.
    if let Some((cols, rows)) = winsize_impl(1) {
        CACHED_COLS.store(cols, Ordering::Relaxed);
        CACHED_ROWS.store(rows, Ordering::Relaxed);
    }
    let cb = RESIZE_CALLBACK.with(|c| *c.borrow());
    if let Some(cb_i64) = cb {
        let closure = cb_i64 as *const ClosureHeader;
        js_closure_call0(closure);
        return 1;
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    struct FdGuard(i32);

    #[cfg(unix)]
    impl Drop for FdGuard {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.0);
            }
        }
    }

    #[test]
    fn isatty_zero_for_pipe() {
        // In test runner stdin is not a TTY (cargo test pipes stdin).
        // Note: this is more of a smoke test; the real value is
        // test-environment-dependent.
        let _ = js_tty_isatty(0.0);
        let _ = js_tty_isatty(1.0);
        let _ = js_tty_isatty(2.0);
    }

    #[test]
    fn columns_undefined_when_not_tty() {
        // In test runner stdout is not a TTY → columns/rows return TAG_UNDEFINED.
        let cols = js_process_stdout_columns();
        let rows = js_process_stdout_rows();
        // Both should be TAG_UNDEFINED bits (0x7FFC_0000_0000_0001).
        assert_eq!(cols.to_bits(), 0x7FFC_0000_0000_0001);
        assert_eq!(rows.to_bits(), 0x7FFC_0000_0000_0001);
    }

    #[test]
    fn resize_drain_with_no_callback_returns_zero() {
        RESIZE_PENDING.store(true, Ordering::Release);
        // No callback registered → drain consumes flag, returns 0.
        assert_eq!(js_tty_resize_drain(), 0);
        // Flag now cleared.
        assert_eq!(js_tty_resize_drain(), 0);
    }

    #[test]
    fn isatty_returns_tag_true_or_false() {
        // Result is always a NaN-boxed bool — TAG_TRUE or TAG_FALSE.
        let v = js_tty_isatty(0.0);
        let bits = v.to_bits();
        assert!(
            bits == 0x7FFC_0000_0000_0003 || bits == 0x7FFC_0000_0000_0004,
            "expected TAG_FALSE or TAG_TRUE, got {:#x}",
            bits
        );
    }

    #[test]
    fn isatty_fd_arg_matches_node_validation() {
        assert_eq!(tty_isatty_fd_arg(0.0), Some(0));
        assert_eq!(tty_isatty_fd_arg(-0.0), Some(0));
        assert_eq!(tty_isatty_fd_arg(i32::MAX as f64), Some(i32::MAX));
        assert_eq!(tty_isatty_fd_arg((i32::MAX as f64) + 1.0), None);
        assert_eq!(tty_isatty_fd_arg(-1.0), None);
        assert_eq!(tty_isatty_fd_arg(1.5), None);
        assert_eq!(tty_isatty_fd_arg(f64::NAN), None);
        assert_eq!(tty_isatty_fd_arg(f64::INFINITY), None);
        assert_eq!(tty_isatty_fd_arg(f64::NEG_INFINITY), None);

        assert_eq!(
            tty_isatty_fd_arg(f64::from_bits(JSValue::int32(7).bits())),
            Some(7)
        );
        assert_eq!(
            tty_isatty_fd_arg(f64::from_bits(JSValue::int32(-1).bits())),
            None
        );

        assert_eq!(tty_isatty_fd_arg(TAG_TRUE_F64), None);
        assert_eq!(tty_isatty_fd_arg(TAG_FALSE_F64), None);
        assert_eq!(tty_isatty_fd_arg(TAG_UNDEFINED_F64), None);
        assert_eq!(
            tty_isatty_fd_arg(f64::from_bits(crate::value::TAG_NULL)),
            None
        );
        assert_eq!(
            tty_isatty_fd_arg(f64::from_bits(
                JSValue::try_short_string(b"0").unwrap().bits()
            )),
            None
        );
    }

    /// Windows: isatty/columns/rows/setRawMode against std handles that are
    /// guaranteed pipes. `cargo test` doesn't guarantee the harness's own
    /// std handles are redirected (a direct terminal run inherits the
    /// console), so the test re-invokes itself with all three std streams
    /// piped and does the assertions in the child — deterministic whether
    /// or not the environment has a console.
    #[cfg(windows)]
    #[test]
    fn isatty_and_winsize_report_false_for_piped_std_handles() {
        if std::env::var_os("PERRY_TTY_TEST_PIPED_CHILD").is_some() {
            // All three std handles are pipes here.
            assert_eq!(js_tty_isatty(0.0).to_bits(), TAG_FALSE_F64.to_bits());
            assert_eq!(js_tty_isatty(1.0).to_bits(), TAG_FALSE_F64.to_bits());
            assert_eq!(js_tty_isatty(2.0).to_bits(), TAG_FALSE_F64.to_bits());
            assert!(!is_tty_fd(0));
            // Node: `.isTTY` is undefined (not false) on non-TTY streams.
            assert_eq!(
                js_process_stdout_isatty().to_bits(),
                TAG_UNDEFINED_F64.to_bits()
            );
            // No console screen buffer → columns/rows undefined.
            assert_eq!(
                js_process_stdout_columns().to_bits(),
                TAG_UNDEFINED_F64.to_bits()
            );
            assert_eq!(
                js_process_stdout_rows().to_bits(),
                TAG_UNDEFINED_F64.to_bits()
            );
            // setRawMode(true) must fail on a piped stdin — and must not
            // have saved anything to restore.
            assert!(!set_fd_raw_mode(0, true));
            return;
        }
        let exe = std::env::current_exe().expect("current_exe");
        let output = std::process::Command::new(exe)
            .args([
                "tty::tests::isatty_and_winsize_report_false_for_piped_std_handles",
                "--exact",
                "--nocapture",
            ])
            .env("PERRY_TTY_TEST_PIPED_CHILD", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .expect("spawn self with piped std handles");
        assert!(
            output.status.success(),
            "piped-child assertions failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_stream_init_accepts_pipe_fd() {
        let mut fds = [0; 2];
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(rc, 0);
        let _read_guard = FdGuard(fds[0]);
        let write_guard = FdGuard(fds[1]);

        assert!(!is_tty_fd(write_guard.0));
        assert!(can_init_write_stream_fd(write_guard.0));
    }

    #[cfg(unix)]
    #[test]
    fn write_stream_init_rejects_regular_file_fd() {
        use std::os::unix::io::AsRawFd;

        let path =
            std::env::temp_dir().join(format!("perry-tty-regular-file-{}", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .unwrap();

        assert!(!is_tty_fd(file.as_raw_fd()));
        assert!(!can_init_write_stream_fd(file.as_raw_fd()));

        drop(file);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn color_depth_env_rules_match_node_levels() {
        let depth = |pairs: &[(&str, &str)]| {
            color_depth_from_lookup(|name| {
                pairs
                    .iter()
                    .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
            })
        };
        assert_eq!(depth(&[]), 1);
        assert_eq!(depth(&[("FORCE_COLOR", "0")]), 1);
        assert_eq!(depth(&[("FORCE_COLOR", "1")]), 4);
        assert_eq!(depth(&[("FORCE_COLOR", "2")]), 8);
        assert_eq!(depth(&[("FORCE_COLOR", "3")]), 24);
        assert_eq!(depth(&[("TERM", "dumb")]), 1);
        assert_eq!(depth(&[("TERM", "xterm")]), 4);
        assert_eq!(depth(&[("TERM", "xterm-256color")]), 8);
        assert_eq!(depth(&[("COLORTERM", "truecolor")]), 24);
        assert_eq!(depth(&[("TMUX", "1")]), 24);
        assert_eq!(depth(&[("CI", "1"), ("TRAVIS", "1")]), 8);
        assert_eq!(depth(&[("CI", "1"), ("GITHUB_ACTIONS", "true")]), 24);
    }

    #[test]
    fn has_colors_uses_depth_thresholds() {
        assert!(has_colors_for_depth(2.0, 1));
        assert!(!has_colors_for_depth(3.0, 1));
        assert!(has_colors_for_depth(16.0, 4));
        assert!(!has_colors_for_depth(17.0, 4));
        assert!(has_colors_for_depth(256.0, 8));
        assert!(!has_colors_for_depth(257.0, 8));
        assert!(has_colors_for_depth(16_777_216.0, 24));
        assert!(!has_colors_for_depth(16_777_217.0, 24));
    }
}
