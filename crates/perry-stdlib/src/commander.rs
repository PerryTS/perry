//! Commander implementation
//!
//! Native implementation of the commander npm package for CLI parsing.
//! Provides a fluent API for building command-line interfaces, including
//! subcommands, action callbacks, automatic `--help` / `--version`, and
//! options object construction passed back to the user's `.action()`
//! handler.
//!
//! Closes #187: pre-fix this module stored option metadata but never
//! invoked the `.action()` callback, never linked subcommands to their
//! parent, and never printed help. The docs example silently no-op'd.

use perry_runtime::closure::js_closure_call1;
use perry_runtime::{
    js_object_alloc, js_object_set_field_by_name, js_string_from_bytes, ClosureHeader, StringHeader,
};
use std::collections::HashMap;

use crate::common::{for_each_handle_of, get_handle_mut, register_handle, Handle};

// NaN-box tags. Mirror perry-runtime/src/value.rs constants. Duplicated
// here because they're not exported across crate boundaries; if either
// definition drifts the runtime tests catch it before this code does.
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

#[inline(always)]
fn nanbox_pointer(addr: u64) -> u64 {
    POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF)
}

#[inline(always)]
fn nanbox_string(addr: u64) -> u64 {
    STRING_TAG | (addr & 0x0000_FFFF_FFFF_FFFF)
}

/// CommanderHandle stores the command configuration and parsed values.
pub struct CommanderHandle {
    name: String,
    description: String,
    version: String,
    options: Vec<CommandOption>,
    parsed_values: HashMap<String, ParsedValue>,
    args: Vec<String>,
    /// (subcommand-name, sub-CommanderHandle) — populated by `.command(name)`.
    subcommands: Vec<(String, Handle)>,
    /// Closure pointer (raw bits) for `.action(cb)`. 0 = no action registered.
    /// Stored as i64 for the same Send + Sync reason events.rs stores listener
    /// closures as i64 — raw pointers aren't Send/Sync but the underlying
    /// closure data is managed by the runtime + GC root scanner below.
    action_callback: i64,
}

struct CommandOption {
    short: Option<char>,
    long: String,
    description: String,
    default_value: Option<String>,
    is_flag: bool, // true for boolean flags, false for value options
}

#[derive(Clone)]
enum ParsedValue {
    Str(String),
    Bool(bool),
}

impl CommanderHandle {
    fn new() -> Self {
        CommanderHandle {
            name: String::new(),
            description: String::new(),
            version: String::new(),
            options: Vec::new(),
            parsed_values: HashMap::new(),
            args: Vec::new(),
            subcommands: Vec::new(),
            action_callback: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// GC root scanning — pin user-supplied .action() closures across collections.

static GC_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_gc_scanner_registered() {
    GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_root_scanner(scan_commander_roots);
    });
}

fn scan_commander_roots(mark: &mut dyn FnMut(f64)) {
    for_each_handle_of::<CommanderHandle, _>(|cmd| {
        if cmd.action_callback != 0 {
            mark(f64::from_bits(nanbox_pointer(cmd.action_callback as u64)));
        }
    });
}

// ---------------------------------------------------------------------------
// Helpers

unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 4096 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
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

// ---------------------------------------------------------------------------
// Constructor + fluent setters

#[no_mangle]
pub extern "C" fn js_commander_new() -> Handle {
    ensure_gc_scanner_registered();
    register_handle(CommanderHandle::new())
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_name(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    if let Some(name) = string_from_header(name_ptr) {
        if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
            cmd.name = name;
        }
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_description(
    handle: Handle,
    desc_ptr: *const StringHeader,
) -> Handle {
    if let Some(desc) = string_from_header(desc_ptr) {
        if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
            cmd.description = desc;
        }
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_version(
    handle: Handle,
    version_ptr: *const StringHeader,
) -> Handle {
    if let Some(version) = string_from_header(version_ptr) {
        if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
            cmd.version = version;
        }
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_option(
    handle: Handle,
    flags_ptr: *const StringHeader,
    desc_ptr: *const StringHeader,
    default_ptr: *const StringHeader,
) -> Handle {
    let flags = match string_from_header(flags_ptr) {
        Some(f) => f,
        None => return handle,
    };
    let description = string_from_header(desc_ptr).unwrap_or_default();
    let default_value = string_from_header(default_ptr);
    let (short, long, is_flag) = parse_flag_spec(&flags);
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        cmd.options.push(CommandOption {
            short,
            long,
            description,
            default_value,
            is_flag,
        });
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_required_option(
    handle: Handle,
    flags_ptr: *const StringHeader,
    desc_ptr: *const StringHeader,
    default_ptr: *const StringHeader,
) -> Handle {
    // Required-validation isn't enforced at runtime yet; treat as a normal option.
    js_commander_option(handle, flags_ptr, desc_ptr, default_ptr)
}

/// Register an action callback. `callback` is a raw closure pointer
/// (NaN-box-stripped) — codegen passes it via the NA_PTR coercion which
/// runs `unbox_to_i64` before this entry sees it. Non-zero is the stable
/// "action registered" signal (a real ClosureHeader pointer is far above
/// the small-handle range).
#[no_mangle]
pub extern "C" fn js_commander_action(handle: Handle, callback: i64) -> Handle {
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        cmd.action_callback = callback;
    }
    handle
}

/// Create a subcommand and register it on the parent. Returns the new
/// sub-handle so chained `.command("x").option(...).action(...)` accrues
/// state on the subcommand, not the parent.
#[no_mangle]
pub unsafe extern "C" fn js_commander_command(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> Handle {
    let sub_name = string_from_header(name_ptr).unwrap_or_default();
    let sub_handle = register_handle(CommanderHandle::new());
    if let Some(parent) = get_handle_mut::<CommanderHandle>(handle) {
        parent.subcommands.push((sub_name, sub_handle));
    }
    sub_handle
}

// ---------------------------------------------------------------------------
// Parse + dispatch

/// Top-level parse entry. The second arg is the user's `process.argv`
/// expression — we ignore it and read `std::env::args()` directly so the
/// program name (argv[0]) is always real and the surrounding sub-command
/// argv-slicing logic stays consistent. Codegen still evaluates the user's
/// argv expression for side effects via the NA_F64 dispatch coercion.
#[no_mangle]
pub extern "C" fn js_commander_parse(handle: Handle, _argv: f64) -> Handle {
    let argv: Vec<String> = std::env::args().collect();
    parse_and_dispatch(handle, &argv[1..]);
    handle
}

/// Parse `args` against the command at `handle`, then run its `.action()`
/// (or recurse into a matched subcommand which does the same). On
/// `--help` / `--version` this exits the process with code 0 directly,
/// matching npm commander's behavior.
fn parse_and_dispatch(handle: Handle, args: &[String]) {
    // Snapshot what we need from the command up front. `for_each_handle_of`
    // and `get_handle_mut` both borrow the same handle registry; cloning
    // the relevant fields out avoids overlapping borrows during recursion.
    let snapshot = match get_handle_mut::<CommanderHandle>(handle) {
        Some(cmd) => {
            // Reset parsed state from any prior invocation, then seed with
            // declared defaults.
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
            }
        }
        None => return,
    };

    let mut i = 0usize;
    let mut positional: Vec<String> = Vec::new();
    while i < args.len() {
        let arg = &args[i];

        // --help / -h: print help and exit. Mirrors npm commander.
        if arg == "--help" || arg == "-h" {
            print_help(&snapshot);
            std::process::exit(0);
        }
        // --version / -V: print version and exit (only if a version was set).
        if (arg == "--version" || arg == "-V") && !snapshot.version.is_empty() {
            println!("{}", snapshot.version);
            std::process::exit(0);
        }
        // No version registered: fall through to the unknown-flag path.

        // Subcommand dispatch: when no positional has been collected yet,
        // a bare token matching a registered subcommand recurses with the
        // remaining args, and we hand off entirely (the parent's action
        // does NOT also run — npm commander semantics).
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
                // Unknown long option — store as boolean true so user code
                // calling `options.someFlag` at least sees a defined value.
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

    // Persist positionals (queryable via getArg/argsCount).
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        cmd.args = positional;
    }

    // No subcommand consumed. If this command has its own .action(), run
    // it now. Otherwise it's a no-op (matches npm commander when neither
    // an action nor a subcommand fires).
    run_action(handle);
}

fn set_str(handle: Handle, key: &str, value: &str) {
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        cmd.parsed_values
            .insert(key.to_string(), ParsedValue::Str(value.to_string()));
    }
}

fn set_bool(handle: Handle, key: &str, value: bool) {
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        cmd.parsed_values
            .insert(key.to_string(), ParsedValue::Bool(value));
    }
}

/// Build the `options` JS object passed to `.action(opts => ...)` and
/// invoke the registered closure. No-op if no closure was registered.
fn run_action(handle: Handle) {
    let (cb, parsed) = match get_handle_mut::<CommanderHandle>(handle) {
        Some(cmd) => (cmd.action_callback, cmd.parsed_values.clone()),
        None => return,
    };
    if cb == 0 {
        return;
    }
    unsafe {
        let opts_obj = build_options_object(&parsed);
        let opts_f64 = f64::from_bits(nanbox_pointer(opts_obj as u64));
        let closure_ptr = cb as *const ClosureHeader;
        js_closure_call1(closure_ptr, opts_f64);
    }
}

/// Allocate a fresh JS Object and populate it with one field per parsed
/// option. Strings are stored as STRING_TAG-tagged StringHeader pointers,
/// booleans as the canonical TAG_TRUE / TAG_FALSE bits — matching the
/// values codegen emits for string literals and boolean literals so the
/// user's `options.port` access goes through the same dynamic property
/// lookup path it would for a hand-built object literal.
unsafe fn build_options_object(
    parsed: &HashMap<String, ParsedValue>,
) -> *mut perry_runtime::ObjectHeader {
    let count = parsed.len() as u32;
    let obj = js_object_alloc(0, count);
    for (key, value) in parsed.iter() {
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let val_bits: u64 = match value {
            ParsedValue::Str(s) => {
                let s_ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                nanbox_string(s_ptr as u64)
            }
            ParsedValue::Bool(true) => TAG_TRUE,
            ParsedValue::Bool(false) => TAG_FALSE,
        };
        js_object_set_field_by_name(obj, key_ptr, f64::from_bits(val_bits));
    }
    obj
}

// ---------------------------------------------------------------------------
// Help formatting

struct ParseSnapshot {
    name: String,
    description: String,
    version: String,
    options: Vec<OptionMeta>,
    subcommands: Vec<(String, Handle)>,
}

struct OptionMeta {
    short: Option<char>,
    long: String,
    is_flag: bool,
    description: String,
}

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
    let usage_tail = if s.subcommands.is_empty() {
        "[options]".to_string()
    } else {
        "[options] [command]".to_string()
    };
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

// ---------------------------------------------------------------------------
// Read-back accessors (queryable post-parse from user TS code).

#[no_mangle]
pub extern "C" fn js_commander_opts(handle: Handle) -> Handle {
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_get_option(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = match string_from_header(name_ptr) {
        Some(n) => n,
        None => return std::ptr::null(),
    };
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        if let Some(ParsedValue::Str(value)) = cmd.parsed_values.get(&name) {
            return js_string_from_bytes(value.as_ptr(), value.len() as u32);
        }
    }
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_get_option_number(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> f64 {
    let name = match string_from_header(name_ptr) {
        Some(n) => n,
        None => return f64::NAN,
    };
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        if let Some(ParsedValue::Str(value)) = cmd.parsed_values.get(&name) {
            return value.parse::<f64>().unwrap_or(f64::NAN);
        }
    }
    f64::NAN
}

#[no_mangle]
pub unsafe extern "C" fn js_commander_get_option_bool(
    handle: Handle,
    name_ptr: *const StringHeader,
) -> f64 {
    let name = match string_from_header(name_ptr) {
        Some(n) => n,
        None => return f64::from_bits(TAG_FALSE),
    };
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
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
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        return cmd.args.len() as f64;
    }
    0.0
}

#[no_mangle]
pub extern "C" fn js_commander_get_arg(handle: Handle, index: f64) -> *const StringHeader {
    let idx = index as usize;
    if let Some(cmd) = get_handle_mut::<CommanderHandle>(handle) {
        if idx < cmd.args.len() {
            let arg = &cmd.args[idx];
            return js_string_from_bytes(arg.as_ptr(), arg.len() as u32);
        }
    }
    std::ptr::null()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
