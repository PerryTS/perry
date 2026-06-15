//! Native bindings for the npm `commander` package — fluent CLI
//! parser. Uses only perry-ffi v0.5: strings, handle registry,
//! object alloc-with-shape, closure invocation, GC root scanner.
//!
//! Functional parity with perry-stdlib's existing copy: command +
//! subcommand fluent setup, option parsing (long/short, flags vs
//! values, defaults, --key=value), `.action(opts => ...)`,
//! automatic --help / --version, post-parse query accessors.

use perry_ffi::{
    alloc_string, gc_register_mutable_root_scanner_named, get_handle, get_handle_mut,
    iter_handles_of_mut, js_array_alloc, js_array_get, js_array_length, js_array_push,
    js_object_alloc_with_shape, js_object_set_field, read_string, register_handle, with_handle_mut,
    ArrayHeader, GcRootVisitor, Handle, JsClosure, JsString, JsValue, RawClosureHeader,
    StringHeader,
};
use std::collections::HashMap;

const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub struct CommanderHandle {
    name: String,
    description: String,
    version: String,
    options: Vec<CommandOption>,
    parsed_values: HashMap<String, ParsedValue>,
    args: Vec<String>,
    /// Declared positional argument specs from `.argument("<file>")` /
    /// `.argument("[dir]")` — used only for the `--help` usage line. Parsing
    /// itself collects every non-option token into `args` regardless.
    declared_args: Vec<String>,
    /// (subcommand-name, sub-CommanderHandle) — populated by `.command(name)`.
    subcommands: Vec<(String, Handle)>,
    /// Closure pointer (raw bits) for `.action(cb)`. 0 = no action.
    /// Stored as i64 for the same Send + Sync reason perry-ext-events
    /// stores listener closures as i64 — raw pointers aren't
    /// Send/Sync but the underlying closure data is GC-managed.
    action_callback: i64,
}

struct CommandOption {
    short: Option<char>,
    long: String,
    description: String,
    default_value: Option<String>,
    is_flag: bool,
}

#[derive(Clone)]
enum ParsedValue {
    Str(String),
    Bool(bool),
}

impl Default for CommanderHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CommanderHandle {
    pub fn new() -> Self {
        CommanderHandle {
            name: String::new(),
            description: String::new(),
            version: String::new(),
            options: Vec::new(),
            parsed_values: HashMap::new(),
            args: Vec::new(),
            declared_args: Vec::new(),
            subcommands: Vec::new(),
            action_callback: 0,
        }
    }
}

// ── GC root scanning ──────────────────────────────────────────────

static GC_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_gc_scanner_registered() {
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-commander", scan_commander_roots);
    });
}

fn scan_commander_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<CommanderHandle, _>(|cmd| {
        visitor.visit_i64_slot(&mut cmd.action_callback);
    });
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 4096 {
        return None;
    }
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// Parse the commander flag-spec mini-language used in `.option(...)`:
/// `"-p, --port <number>"` → `(Some('p'), "port", false)`.
/// `"-v, --verbose"`        → `(Some('v'), "verbose", true)`.
/// `"--config <path>"`      → `(None, "config", false)`.
fn parse_flag_spec(flags: &str) -> (Option<char>, String, bool) {
    let is_flag = !flags.contains('<') && !flags.contains('[');
    let mut short: Option<char> = None;
    let mut long = String::new();
    for part in flags.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("--") {
            long = rest.split_whitespace().next().unwrap_or("").to_string();
        } else if let Some(rest) = part.strip_prefix('-') {
            short = rest.chars().next();
        }
    }
    (short, long, is_flag)
}

// ── Constructor + fluent setters ──────────────────────────────────

#[no_mangle]
pub extern "C" fn js_commander_new() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(CommanderHandle::new())
}

/// # Safety
/// `name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_name(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    if let Some(name) = read_str(name_ptr) {
        with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| cmd.name = name);
    }
    handle
}

/// # Safety
/// `desc_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_description(
    handle: Handle,
    desc_ptr: *const StringHeader,
) -> Handle {
    if let Some(desc) = read_str(desc_ptr) {
        with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| cmd.description = desc);
    }
    handle
}

/// # Safety
/// `version_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_version(
    handle: Handle,
    version_ptr: *const StringHeader,
) -> Handle {
    if let Some(version) = read_str(version_ptr) {
        with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| cmd.version = version);
    }
    handle
}

/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_commander_option(
    handle: Handle,
    flags_ptr: *const StringHeader,
    desc_ptr: *const StringHeader,
    default_ptr: *const StringHeader,
) -> Handle {
    let flags = match read_str(flags_ptr) {
        Some(f) => f,
        None => return handle,
    };
    let description = read_str(desc_ptr).unwrap_or_default();
    let default_value = read_str(default_ptr);
    let (short, long, is_flag) = parse_flag_spec(&flags);
    with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| {
        cmd.options.push(CommandOption {
            short,
            long,
            description,
            default_value,
            is_flag,
        });
    });
    handle
}

/// Required-validation isn't enforced at runtime yet; treat as a normal option.
///
/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_commander_required_option(
    handle: Handle,
    flags_ptr: *const StringHeader,
    desc_ptr: *const StringHeader,
    default_ptr: *const StringHeader,
) -> Handle {
    js_commander_option(handle, flags_ptr, desc_ptr, default_ptr)
}

/// `.argument("<file>")` / `.argument("[dir]")` — declare a positional
/// argument. Parsing always collects non-option tokens into `args`, so this
/// only records the spec for the `--help` usage line and returns the handle so
/// the fluent chain keeps flowing. #5137: without this entry the call fell
/// through to generic dynamic dispatch (a silent no-op) instead of staying on
/// the commander handle.
///
/// # Safety
/// `spec_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_argument(
    handle: Handle,
    spec_ptr: *const StringHeader,
) -> Handle {
    if let Some(spec) = read_str(spec_ptr) {
        with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| {
            cmd.declared_args.push(spec);
        });
    }
    handle
}

/// Register an action callback. `callback` is a raw closure pointer
/// (NaN-box-stripped) — codegen passes it via the NA_PTR coercion which
/// runs `unbox_to_i64` before this entry sees it. Non-zero is the
/// stable "action registered" signal.
#[no_mangle]
pub extern "C" fn js_commander_action(handle: Handle, callback: i64) -> Handle {
    ensure_gc_scanner_registered();
    with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| {
        cmd.action_callback = callback;
    });
    handle
}

/// Create a subcommand and register it on the parent. Returns the
/// new sub-handle so chained `.command("x").option(...).action(...)`
/// accrues state on the subcommand, not the parent.
///
/// # Safety
/// `name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_command(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    let sub_name = read_str(name_ptr).unwrap_or_default();
    let sub_handle = register_handle(CommanderHandle::new());
    with_handle_mut::<CommanderHandle, _, _>(handle, |parent| {
        parent.subcommands.push((sub_name, sub_handle));
    });
    sub_handle
}

// ── Parse + dispatch ──────────────────────────────────────────────

/// Resolve the argument list `parse(argv?)` should operate on.
///
/// npm commander's `parse()` defaults to `from: 'node'`: when an explicit
/// array is supplied (`program.parse(['node', 'script', ...])`) the first two
/// entries are the executable + script path and the real args start at index
/// 2. When called with no argument it reads `process.argv`, which on a Perry
/// binary is `[exePath, ...realArgs]` (no separate script entry) — so we skip
/// only the leading exe path. #5137: previously this always read
/// `std::env::args()` and ignored the passed array, so `program.parse([...])`
/// with a synthetic argv (the common test/REPL shape, and the issue repro)
/// silently parsed nothing.
fn resolve_parse_args(argv: f64) -> Vec<String> {
    let value = JsValue::from_bits(argv.to_bits());
    if value.is_pointer() {
        let arr = value.as_pointer::<ArrayHeader>();
        if !arr.is_null() {
            let len = unsafe { js_array_length(arr) };
            let mut out = Vec::with_capacity(len as usize);
            for i in 0..len {
                let elem = unsafe { js_array_get(arr, i) };
                if let Some(s) = unsafe { read_str(elem.as_string_ptr()) } {
                    out.push(s);
                }
            }
            // `from: 'node'` default — drop argv[0] (exe) and argv[1] (script).
            return out.into_iter().skip(2).collect();
        }
    }
    std::env::args().skip(1).collect()
}

/// Top-level parse entry. The second arg is the user's `parse(argv)`
/// expression: when it's an explicit array we honor it (commander's
/// `from: 'node'` default), otherwise we fall back to the real
/// `std::env::args()`. Codegen passes the NaN-boxed value through unchanged
/// via the NA_F64 dispatch slot.
#[no_mangle]
pub extern "C" fn js_commander_parse(handle: Handle, argv: f64) -> Handle {
    let args = resolve_parse_args(argv);
    parse_and_dispatch(handle, &args);
    handle
}

struct ParseSnapshot {
    name: String,
    description: String,
    version: String,
    options: Vec<OptionMeta>,
    subcommands: Vec<(String, Handle)>,
    declared_args: Vec<String>,
}

struct OptionMeta {
    short: Option<char>,
    long: String,
    is_flag: bool,
    description: String,
}

fn snapshot_for_parse(handle: Handle) -> Option<ParseSnapshot> {
    get_handle_mut::<CommanderHandle>(handle).map(|cmd| {
        cmd.parsed_values.clear();
        cmd.args.clear();
        for opt in &cmd.options {
            if let Some(ref dv) = opt.default_value {
                cmd.parsed_values
                    .insert(opt.long.clone(), ParsedValue::Str(dv.clone()));
            }
        }
        ParseSnapshot {
            name: cmd.name.clone(),
            description: cmd.description.clone(),
            version: cmd.version.clone(),
            options: cmd
                .options
                .iter()
                .map(|o| OptionMeta {
                    short: o.short,
                    long: o.long.clone(),
                    is_flag: o.is_flag,
                    description: o.description.clone(),
                })
                .collect(),
            subcommands: cmd.subcommands.clone(),
            declared_args: cmd.declared_args.clone(),
        }
    })
}

/// Parse `args` against the command at `handle`, then run its
/// `.action()` (or recurse into a matched subcommand). On `--help`
/// / `--version` this exits the process with code 0 directly,
/// matching npm commander's behavior.
fn parse_and_dispatch(handle: Handle, args: &[String]) {
    let Some(snapshot) = snapshot_for_parse(handle) else {
        return;
    };

    let mut i = 0usize;
    let mut positional: Vec<String> = Vec::new();
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            print_help(&snapshot);
            std::process::exit(0);
        }
        if (arg == "--version" || arg == "-V") && !snapshot.version.is_empty() {
            println!("{}", snapshot.version);
            std::process::exit(0);
        }
        if positional.is_empty() {
            if let Some((_, sub_handle)) = snapshot.subcommands.iter().find(|(n, _)| n == arg) {
                let rest: Vec<String> = args[i + 1..].to_vec();
                parse_and_dispatch(*sub_handle, &rest);
                return;
            }
        }
        if let Some(opt_name) = arg.strip_prefix("--") {
            if let Some(eq_pos) = opt_name.find('=') {
                let key = opt_name[..eq_pos].to_string();
                let value = opt_name[eq_pos + 1..].to_string();
                set_str(handle, &key, &value);
            } else if let Some(meta) = snapshot.options.iter().find(|o| o.long == opt_name) {
                if meta.is_flag {
                    set_bool(handle, &meta.long, true);
                } else if i + 1 < args.len() {
                    i += 1;
                    set_str(handle, &meta.long, &args[i]);
                }
            } else {
                set_bool(handle, opt_name, true);
            }
        } else if let Some(short_str) = arg.strip_prefix('-') {
            if short_str.len() == 1 {
                let ch = short_str.chars().next().unwrap();
                if let Some(meta) = snapshot.options.iter().find(|o| o.short == Some(ch)) {
                    if meta.is_flag {
                        set_bool(handle, &meta.long, true);
                    } else if i + 1 < args.len() {
                        i += 1;
                        set_str(handle, &meta.long, &args[i]);
                    }
                }
            }
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }

    with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| {
        cmd.args = positional;
    });

    run_action(handle);
}

fn set_str(handle: Handle, key: &str, value: &str) {
    let key = key.to_string();
    let value = value.to_string();
    with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| {
        cmd.parsed_values.insert(key, ParsedValue::Str(value));
    });
}

fn set_bool(handle: Handle, key: &str, value: bool) {
    let key = key.to_string();
    with_handle_mut::<CommanderHandle, _, _>(handle, |cmd| {
        cmd.parsed_values.insert(key, ParsedValue::Bool(value));
    });
}

/// Build the `options` JS object passed to `.action(opts => ...)`
/// and invoke the registered closure. No-op if no closure was
/// registered.
fn run_action(handle: Handle) {
    let parsed = match get_handle::<CommanderHandle>(handle) {
        Some(cmd) => (cmd.action_callback, cmd.parsed_values.clone()),
        None => return,
    };
    let (cb, parsed) = parsed;
    if cb == 0 {
        return;
    }
    let opts_value = build_options_object(&parsed);
    let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
    if !closure.is_null() {
        // SAFETY: cb is a non-null closure pointer kept alive by the
        // GC root scanner registered in `ensure_gc_scanner_registered`.
        let _ = unsafe { closure.call1(f64::from_bits(opts_value.bits())) };
    }
}

/// Allocate a fresh JS Object using perry-ffi's
/// `js_object_alloc_with_shape` and populate it with one field per
/// parsed option. Strings are stored as STRING_TAG-tagged, booleans
/// as TAG_TRUE / TAG_FALSE — the dynamic property lookup user code
/// runs on `options.port` traverses the same path it would for a
/// hand-built object literal.
fn build_options_object(parsed: &HashMap<String, ParsedValue>) -> JsValue {
    if parsed.is_empty() {
        // Allocate an empty object (zero fields) so user code can
        // still call .someField → undefined without faulting.
        let (packed, shape_id) = perry_ffi::build_object_shape(&[]);
        let obj = unsafe {
            js_object_alloc_with_shape(shape_id, 0, packed.as_ptr(), packed.len() as u32)
        };
        return JsValue::from_object_ptr(obj);
    }

    let keys: Vec<String> = parsed.keys().cloned().collect();
    let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    let (packed, shape_id) = perry_ffi::build_object_shape(&key_refs);
    let obj = unsafe {
        js_object_alloc_with_shape(
            shape_id,
            keys.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    for (i, k) in keys.iter().enumerate() {
        let val = match parsed.get(k) {
            Some(ParsedValue::Str(s)) => JsValue::from_string_ptr(alloc_string(s).as_raw()),
            Some(ParsedValue::Bool(true)) => JsValue::from_bits(TAG_TRUE),
            Some(ParsedValue::Bool(false)) => JsValue::from_bits(TAG_FALSE),
            None => JsValue::UNDEFINED,
        };
        unsafe { js_object_set_field(obj, i as u32, val) };
    }
    JsValue::from_object_ptr(obj)
}

// ── Help formatting ───────────────────────────────────────────────

fn print_help(s: &ParseSnapshot) {
    if !s.description.is_empty() {
        println!("{}", s.description);
        println!();
    }
    let prog = if s.name.is_empty() {
        "<program>".to_string()
    } else {
        s.name.clone()
    };
    let mut usage_tail = if s.subcommands.is_empty() {
        "[options]".to_string()
    } else {
        "[options] [command]".to_string()
    };
    for arg in &s.declared_args {
        usage_tail.push(' ');
        usage_tail.push_str(arg);
    }
    println!("Usage: {} {}", prog, usage_tail);
    println!();
    println!("Options:");
    if !s.version.is_empty() {
        println!("  {:<24}  output the version number", "-V, --version");
    }
    for opt in &s.options {
        let placeholder = if opt.is_flag { "" } else { " <value>" };
        let flag_str = match opt.short {
            Some(ch) => format!("-{}, --{}{}", ch, opt.long, placeholder),
            None => format!("--{}{}", opt.long, placeholder),
        };
        println!("  {:<24}  {}", flag_str, opt.description);
    }
    println!("  {:<24}  display help for command", "-h, --help");
    if !s.subcommands.is_empty() {
        println!();
        println!("Commands:");
        for (sub_name, _) in &s.subcommands {
            println!("  {}", sub_name);
        }
    }
}

// ── Read-back accessors ───────────────────────────────────────────

/// `program.opts()` — return a fresh plain object of the parsed option
/// values (matching npm commander, where `opts()` returns a data object).
/// #5137: previously returned the raw handle, so `JSON.stringify(opts)` saw a
/// bogus pointer and printed `null` and `opts.verbose` never resolved. The
/// NR_PTR return ABI NaN-boxes the returned heap pointer as a JS object value.
#[no_mangle]
pub extern "C" fn js_commander_opts(handle: Handle) -> Handle {
    let parsed = get_handle_mut::<CommanderHandle>(handle)
        .map(|cmd| cmd.parsed_values.clone())
        .unwrap_or_default();
    build_options_object(&parsed).as_pointer::<u8>() as Handle
}

/// `program.args` — return a fresh JS array of the parsed positional
/// arguments (everything that wasn't an option flag or option value).
/// #5137: a bare `program.args` member read lowers to a 0-arg
/// NativeMethodCall through the commander table; without this getter it
/// resolved to the zero-sentinel and `program.args[0]` read `undefined`.
#[no_mangle]
pub extern "C" fn js_commander_args_array(handle: Handle) -> Handle {
    let args = get_handle_mut::<CommanderHandle>(handle)
        .map(|cmd| cmd.args.clone())
        .unwrap_or_default();
    unsafe {
        let mut arr = js_array_alloc(args.len() as u32);
        for a in &args {
            let val = JsValue::from_string_ptr(alloc_string(a).as_raw());
            arr = js_array_push(arr, val);
        }
        arr as Handle
    }
}

/// # Safety
/// `name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_get_option(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = match read_str(name_ptr) {
        Some(n) => n,
        None => return std::ptr::null(),
    };
    if let Some(cmd) = get_handle::<CommanderHandle>(handle) {
        if let Some(ParsedValue::Str(value)) = cmd.parsed_values.get(&name) {
            return alloc_string(value).as_raw();
        }
    }
    std::ptr::null()
}

/// # Safety
/// `name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_get_option_number(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> f64 {
    let name = match read_str(name_ptr) {
        Some(n) => n,
        None => return f64::NAN,
    };
    if let Some(cmd) = get_handle::<CommanderHandle>(handle) {
        if let Some(ParsedValue::Str(value)) = cmd.parsed_values.get(&name) {
            return value.parse::<f64>().unwrap_or(f64::NAN);
        }
    }
    f64::NAN
}

/// # Safety
/// `name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_commander_get_option_bool(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> f64 {
    let name = match read_str(name_ptr) {
        Some(n) => n,
        None => return f64::from_bits(TAG_FALSE),
    };
    if let Some(cmd) = get_handle::<CommanderHandle>(handle) {
        match cmd.parsed_values.get(&name) {
            Some(ParsedValue::Bool(true)) => return f64::from_bits(TAG_TRUE),
            Some(ParsedValue::Str(_)) => return f64::from_bits(TAG_TRUE),
            _ => {}
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_commander_args_count(handle: Handle) -> f64 {
    get_handle::<CommanderHandle>(handle)
        .map(|cmd| cmd.args.len() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_commander_get_arg(handle: Handle, index: f64) -> *const StringHeader {
    let idx = index as usize;
    if let Some(cmd) = get_handle::<CommanderHandle>(handle) {
        if idx < cmd.args.len() {
            return alloc_string(&cmd.args[idx]).as_raw();
        }
    }
    std::ptr::null()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::drop_handle;
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn assert_rewritten(before: i64, after: i64) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
    }

    #[test]
    fn parse_flag_spec_value_with_short() {
        let (s, l, f) = parse_flag_spec("-p, --port <number>");
        assert_eq!(s, Some('p'));
        assert_eq!(l, "port");
        assert!(!f);
    }

    #[test]
    fn parse_flag_spec_boolean_long_only() {
        let (s, l, f) = parse_flag_spec("--verbose");
        assert_eq!(s, None);
        assert_eq!(l, "verbose");
        assert!(f);
    }

    #[test]
    fn parse_flag_spec_optional_value() {
        let (s, l, f) = parse_flag_spec("-c, --config [path]");
        assert_eq!(s, Some('c'));
        assert_eq!(l, "config");
        assert!(!f);
    }

    #[test]
    fn gc_mutable_scanner_rewrites_action_callback_root() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named(
            "perry-ext-commander",
            scan_commander_roots,
        );

        let callback = young_gc_root();
        let mut cmd = CommanderHandle::new();
        cmd.action_callback = callback;
        let handle = register_handle(cmd);

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let cmd =
                get_handle::<CommanderHandle>(handle).expect("commander handle should remain live");
            assert_rewritten(callback, cmd.action_callback);
        }
        drop_handle(handle);
    }

    #[test]
    fn fluent_setters_round_trip() {
        let h = js_commander_new();
        let name = alloc_string("myprog");
        unsafe { js_commander_name(h, name.as_raw()) };
        let desc = alloc_string("A test program");
        unsafe { js_commander_description(h, desc.as_raw()) };
        let ver = alloc_string("1.2.3");
        unsafe { js_commander_version(h, ver.as_raw()) };
        if let Some(cmd) = get_handle::<CommanderHandle>(h) {
            assert_eq!(cmd.name, "myprog");
            assert_eq!(cmd.description, "A test program");
            assert_eq!(cmd.version, "1.2.3");
        } else {
            panic!("handle missing");
        }
    }

    #[test]
    fn option_added_to_command() {
        let h = js_commander_new();
        let flags = alloc_string("-p, --port <number>");
        let desc = alloc_string("listen port");
        let null = std::ptr::null::<StringHeader>();
        unsafe { js_commander_option(h, flags.as_raw(), desc.as_raw(), null) };
        if let Some(cmd) = get_handle::<CommanderHandle>(h) {
            assert_eq!(cmd.options.len(), 1);
            assert_eq!(cmd.options[0].long, "port");
            assert_eq!(cmd.options[0].short, Some('p'));
            assert!(!cmd.options[0].is_flag);
        }
    }
}
