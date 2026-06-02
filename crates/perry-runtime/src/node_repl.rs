use crate::array::ArrayHeader;
use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
use crate::value::{JSValue, POINTER_MASK, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED};

const KEY_COMMANDS: &[u8] = b"__perryReplCommands";
const KEY_PROMPT: &[u8] = b"__perryReplPrompt";
const LISTENERS_PREFIX: &[u8] = b"__perryReplListeners:";
const ONCE_PREFIX: &[u8] = b"__perryReplOnce:";

thread_local! {
    static RECOVERABLE_ERRORS: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn hidden_key(bytes: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn string_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn object_value(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn array_value(arr: *mut ArrayHeader) -> f64 {
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    js_object_set_field_by_name(obj, key(name), value);
}

fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

fn get_prop(value: f64, name: &str) -> f64 {
    let Some(obj) = object_ptr_from_value(value) else {
        return undefined();
    };
    js_object_get_field_by_name_f64(obj as *const ObjectHeader, key(name))
}

fn set_hidden_value(object: f64, name: &[u8], value: f64) {
    if let Some(obj) = object_ptr_from_value(object) {
        js_object_set_field_by_name(obj, hidden_key(name), value);
    }
}

fn get_hidden_value(object: f64, name: &[u8]) -> f64 {
    let Some(obj) = object_ptr_from_value(object) else {
        return undefined();
    };
    js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(name))
}

fn string_to_rust(value: f64) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).to_string())
    }
}

fn number_value(value: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_int32() {
        Some(jsval.as_int32() as f64)
    } else if jsval.is_number() {
        Some(value)
    } else {
        None
    }
}

fn bool_option(options: f64, name: &str, default: bool) -> bool {
    let value = get_prop(options, name);
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_bool() {
        jsval.as_bool()
    } else {
        default
    }
}

fn string_option(options: f64, name: &str, default: &str) -> String {
    string_to_rust(get_prop(options, name)).unwrap_or_else(|| default.to_string())
}

fn option_or_undefined(options: f64, name: &str) -> f64 {
    get_prop(options, name)
}

fn is_callable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000 && !crate::closure::get_valid_func_ptr(raw as *const ClosureHeader).is_null()
}

fn call_function(callback: f64, this: f64, args: &[f64]) -> f64 {
    if !is_callable_value(callback) {
        return undefined();
    }
    let rebound = f64::from_bits(crate::closure::clone_closure_rebind_this(
        callback.to_bits(),
        this,
    ));
    let prev = crate::object::js_implicit_this_set(this);
    let result =
        unsafe { crate::closure::js_native_call_value(rebound, args.as_ptr(), args.len()) };
    crate::object::js_implicit_this_set(prev);
    result
}

fn call_method(receiver: f64, name: &str, args: &[f64]) -> f64 {
    let method = get_prop(receiver, name);
    call_function(method, receiver, args)
}

fn listener_event_key(prefix: &[u8], event: f64) -> Option<*mut crate::StringHeader> {
    let event = string_to_rust(event)?;
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(event.as_bytes());
    Some(hidden_key(&bytes))
}

fn listener_storage(server: f64, event: f64) -> Option<(f64, f64)> {
    let obj = object_ptr_from_value(server)?;
    let listener_key = listener_event_key(LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(ONCE_PREFIX, event)?;
    let listeners = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
    if listeners.to_bits() == TAG_UNDEFINED {
        return None;
    }
    let once = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
    if once.to_bits() == TAG_UNDEFINED {
        return None;
    }
    Some((listeners, once))
}

fn ensure_listener_storage(server: f64, event: f64) -> Option<(f64, f64)> {
    let obj = object_ptr_from_value(server)?;
    let listener_key = listener_event_key(LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(ONCE_PREFIX, event)?;
    let listeners = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = array_value(crate::array::js_array_alloc(0));
            js_object_set_field_by_name(obj, listener_key, arr);
            arr
        } else {
            value
        }
    };
    let once = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = array_value(crate::array::js_array_alloc(0));
            js_object_set_field_by_name(obj, once_key, arr);
            arr
        } else {
            value
        }
    };
    Some((listeners, once))
}

fn set_listener_storage(server: f64, event: f64, listeners: f64, once: f64) {
    let Some(obj) = object_ptr_from_value(server) else {
        return;
    };
    if let Some(listener_key) = listener_event_key(LISTENERS_PREFIX, event) {
        js_object_set_field_by_name(obj, listener_key, listeners);
    }
    if let Some(once_key) = listener_event_key(ONCE_PREFIX, event) {
        js_object_set_field_by_name(obj, once_key, once);
    }
}

fn add_listener(server: f64, event: f64, listener: f64, once: bool) {
    if string_to_rust(event).is_none() {
        return;
    }
    if !is_callable_value(listener) {
        crate::fs::validate::throw_type_error_with_code(
            "The \"listener\" argument must be of type function",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let Some((listeners, once_flags)) = ensure_listener_storage(server, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len + 1);
    let mut out_once = crate::array::js_array_alloc(len + 1);
    for i in 0..len {
        out_listeners = crate::array::js_array_push_f64(
            out_listeners,
            crate::array::js_array_get_f64(listeners_raw, i),
        );
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_raw, i));
    }
    out_listeners = crate::array::js_array_push_f64(out_listeners, listener);
    out_once = crate::array::js_array_push_f64(out_once, bool_value(once));
    set_listener_storage(
        server,
        event,
        array_value(out_listeners),
        array_value(out_once),
    );
}

fn listener_snapshot(server: f64, event: f64) -> Vec<(f64, bool)> {
    let Some((listeners, once_flags)) = listener_storage(server, event) else {
        return Vec::new();
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push((
            crate::array::js_array_get_f64(listeners_raw, i),
            crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0,
        ));
    }
    out
}

fn remove_once_listeners(server: f64, event: f64) {
    let Some((listeners, once_flags)) = listener_storage(server, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return;
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len);
    let mut out_once = crate::array::js_array_alloc(len);
    for i in 0..len {
        let once = crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0;
        if !once {
            out_listeners = crate::array::js_array_push_f64(
                out_listeners,
                crate::array::js_array_get_f64(listeners_raw, i),
            );
            out_once = crate::array::js_array_push_f64(
                out_once,
                crate::array::js_array_get_f64(once_raw, i),
            );
        }
    }
    set_listener_storage(
        server,
        event,
        array_value(out_listeners),
        array_value(out_once),
    );
}

fn emit_event(server: f64, event: &str, args: &[f64]) {
    let event_value = string_value(event);
    let snapshot = listener_snapshot(server, event_value);
    if snapshot.is_empty() {
        return;
    }
    if snapshot.iter().any(|(_, once)| *once) {
        remove_once_listeners(server, event_value);
    }
    for (listener, _) in snapshot {
        call_function(listener, server, args);
    }
}

fn output_write(server: f64, text: &str) {
    let output = get_prop(server, "outputStream");
    let chunk = string_value(text);
    call_method(output, "write", &[chunk]);
}

fn prompt_string(server: f64) -> String {
    string_to_rust(get_hidden_value(server, KEY_PROMPT)).unwrap_or_else(|| "> ".to_string())
}

fn display_prompt_for(server: f64) {
    output_write(server, &prompt_string(server));
}

fn fn_value(func: *const u8, name: &str, arity: u32) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::value::js_nanbox_pointer(closure as i64)
}

extern "C" fn repl_on_thunk(_closure: *const ClosureHeader, event: f64, listener: f64) -> f64 {
    let server = crate::object::js_implicit_this_get();
    add_listener(server, event, listener, false);
    server
}

extern "C" fn repl_once_thunk(_closure: *const ClosureHeader, event: f64, listener: f64) -> f64 {
    let server = crate::object::js_implicit_this_get();
    add_listener(server, event, listener, true);
    server
}

extern "C" fn repl_emit_thunk(
    _closure: *const ClosureHeader,
    event: f64,
    arg0: f64,
    arg1: f64,
) -> f64 {
    let server = crate::object::js_implicit_this_get();
    let Some(event_name) = string_to_rust(event) else {
        return bool_value(false);
    };
    let args: Vec<f64> = [arg0, arg1]
        .into_iter()
        .filter(|value| value.to_bits() != TAG_UNDEFINED)
        .collect();
    emit_event(server, &event_name, &args);
    bool_value(true)
}

extern "C" fn repl_display_prompt_thunk(
    _closure: *const ClosureHeader,
    _preserve_cursor: f64,
) -> f64 {
    let server = crate::object::js_implicit_this_get();
    display_prompt_for(server);
    undefined()
}

extern "C" fn repl_clear_buffered_command_thunk(_closure: *const ClosureHeader) -> f64 {
    undefined()
}

extern "C" fn repl_setup_history_thunk(
    _closure: *const ClosureHeader,
    _path: f64,
    callback: f64,
) -> f64 {
    let server = crate::object::js_implicit_this_get();
    if let Some(obj) = object_ptr_from_value(server) {
        set_field(obj, "history", array_value(crate::array::js_array_alloc(0)));
        set_field(obj, "historySize", 30.0);
    }
    call_function(callback, server, &[null(), server]);
    undefined()
}

extern "C" fn repl_define_command_thunk(
    _closure: *const ClosureHeader,
    keyword: f64,
    command: f64,
) -> f64 {
    let server = crate::object::js_implicit_this_get();
    let Some(name) = string_to_rust(keyword) else {
        return undefined();
    };
    let commands = get_or_create_commands(server);
    if let Some(commands_obj) = object_ptr_from_value(commands) {
        set_field(commands_obj, &name, command);
    }
    undefined()
}

extern "C" fn repl_write_thunk(_closure: *const ClosureHeader, chunk: f64) -> f64 {
    let server = crate::object::js_implicit_this_get();
    let input = string_to_rust(chunk).unwrap_or_default();
    for line in input.split_inclusive('\n') {
        let line = line.strip_suffix('\n').unwrap_or(line);
        run_repl_line(server, line);
    }
    undefined()
}

fn install_server_methods(obj: *mut ObjectHeader) {
    set_field(obj, "on", fn_value(repl_on_thunk as *const u8, "on", 2));
    set_field(
        obj,
        "addListener",
        fn_value(repl_on_thunk as *const u8, "addListener", 2),
    );
    set_field(
        obj,
        "once",
        fn_value(repl_once_thunk as *const u8, "once", 2),
    );
    set_field(
        obj,
        "emit",
        fn_value(repl_emit_thunk as *const u8, "emit", 1),
    );
    set_field(
        obj,
        "defineCommand",
        fn_value(repl_define_command_thunk as *const u8, "defineCommand", 2),
    );
    set_field(
        obj,
        "displayPrompt",
        fn_value(repl_display_prompt_thunk as *const u8, "displayPrompt", 1),
    );
    set_field(
        obj,
        "clearBufferedCommand",
        fn_value(
            repl_clear_buffered_command_thunk as *const u8,
            "clearBufferedCommand",
            0,
        ),
    );
    set_field(
        obj,
        "setupHistory",
        fn_value(repl_setup_history_thunk as *const u8, "setupHistory", 2),
    );
    set_field(
        obj,
        "write",
        fn_value(repl_write_thunk as *const u8, "write", 1),
    );
}

fn get_or_create_commands(server: f64) -> f64 {
    let existing = get_hidden_value(server, KEY_COMMANDS);
    if existing.to_bits() != TAG_UNDEFINED {
        return existing;
    }
    let commands = object_value(js_object_alloc(0, 0));
    set_hidden_value(server, KEY_COMMANDS, commands);
    commands
}

fn get_command(server: f64, name: &str) -> f64 {
    let commands = get_hidden_value(server, KEY_COMMANDS);
    let Some(commands_obj) = object_ptr_from_value(commands) else {
        return undefined();
    };
    js_object_get_field_by_name_f64(commands_obj as *const ObjectHeader, key(name))
}

fn own_data_prop(value: f64, name: &str) -> f64 {
    let Some(obj) = object_ptr_from_value(value) else {
        return undefined();
    };
    unsafe {
        crate::object::own_data_field_by_name(obj as *const ObjectHeader, key(name))
            .map(|value| f64::from_bits(value.bits()))
            .unwrap_or_else(undefined)
    }
}

fn mark_recoverable_error(error: *mut crate::error::ErrorHeader) {
    RECOVERABLE_ERRORS.with(|errors| {
        errors.borrow_mut().insert(error as usize);
    });
}

fn is_marked_recoverable_error(raw: usize) -> bool {
    RECOVERABLE_ERRORS.with(|errors| errors.borrow().contains(&raw))
}

fn context_number(server: f64, name: &str) -> Option<f64> {
    let context = get_prop(server, "context");
    let context_obj = object_ptr_from_value(context)?;
    number_value(js_object_get_field_by_name_f64(
        context_obj as *const ObjectHeader,
        key(name),
    ))
}

fn eval_atom(server: f64, atom: &str) -> Option<f64> {
    let atom = atom.trim();
    if atom.is_empty() {
        return None;
    }
    if let Ok(value) = atom.parse::<f64>() {
        return Some(value);
    }
    context_number(server, atom)
}

fn eval_simple_expression(server: f64, expression: &str) -> Option<Option<f64>> {
    let expression = expression.trim();
    if expression == "undefined" {
        return Some(None);
    }
    if let Some((left, right)) = expression.split_once('+') {
        let left = eval_atom(server, left)?;
        let right = eval_atom(server, right)?;
        return Some(Some(left + right));
    }
    eval_atom(server, expression).map(Some)
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() <= i64::MAX as f64 {
        (value as i64).to_string()
    } else {
        value.to_string()
    }
}

fn run_repl_line(server: f64, line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        display_prompt_for(server);
        return;
    }
    if trimmed == ".exit" {
        emit_event(server, "exit", &[]);
        return;
    }
    if trimmed == ".clear" {
        let context = object_value(js_object_alloc(0, 0));
        if let Some(server_obj) = object_ptr_from_value(server) {
            set_field(server_obj, "context", context);
        }
        output_write(server, "Clearing context...\n");
        emit_event(server, "reset", &[context]);
        display_prompt_for(server);
        return;
    }
    if let Some(command_line) = trimmed.strip_prefix('.') {
        let (name, rest) = command_line
            .split_once(char::is_whitespace)
            .unwrap_or((command_line, ""));
        let command = get_command(server, name);
        let action = if is_callable_value(command) {
            command
        } else {
            own_data_prop(command, "action")
        };
        if is_callable_value(action) {
            call_function(action, server, &[string_value(rest)]);
            return;
        }
        display_prompt_for(server);
        return;
    }
    let ignore_undefined = bool_option(server, "ignoreUndefined", false);
    match eval_simple_expression(server, trimmed) {
        Some(Some(value)) => {
            output_write(server, &(format_number(value) + "\n"));
            display_prompt_for(server);
        }
        Some(None) => {
            if !ignore_undefined {
                output_write(server, "undefined\n");
            }
            display_prompt_for(server);
        }
        None => {
            if !ignore_undefined {
                output_write(server, "undefined\n");
            }
            display_prompt_for(server);
        }
    }
}

fn repl_mode_symbol(name: &str) -> f64 {
    let key = string_value(name);
    unsafe { crate::symbol::js_symbol_for(key) }
}

pub fn repl_mode_sloppy() -> f64 {
    repl_mode_symbol("nodejs.repl.mode.sloppy")
}

pub fn repl_mode_strict() -> f64 {
    repl_mode_symbol("nodejs.repl.mode.strict")
}

#[no_mangle]
pub extern "C" fn js_repl_start(options: f64) -> f64 {
    js_repl_repl_server_new(options)
}

#[no_mangle]
pub extern "C" fn js_repl_repl_server_new(options: f64) -> f64 {
    let server = js_object_alloc(0, 16);
    let server_value = object_value(server);
    let context = object_value(js_object_alloc(0, 0));
    let input = option_or_undefined(options, "input");
    let output = option_or_undefined(options, "output");
    let prompt = string_option(options, "prompt", "> ");
    let repl_mode = {
        let mode = option_or_undefined(options, "replMode");
        if mode.to_bits() == TAG_UNDEFINED {
            repl_mode_sloppy()
        } else {
            mode
        }
    };

    set_field(server, "context", context);
    set_field(server, "inputStream", input);
    set_field(server, "outputStream", output);
    set_field(server, "editorMode", bool_value(false));
    set_field(
        server,
        "useColors",
        bool_value(bool_option(options, "useColors", false)),
    );
    set_field(
        server,
        "useGlobal",
        bool_value(bool_option(options, "useGlobal", false)),
    );
    set_field(
        server,
        "ignoreUndefined",
        bool_value(bool_option(options, "ignoreUndefined", false)),
    );
    set_field(
        server,
        "terminal",
        bool_value(bool_option(options, "terminal", false)),
    );
    set_field(server, "replMode", repl_mode);
    set_field(
        server,
        "constructor",
        crate::object::bound_native_callable_export_value("repl", "REPLServer"),
    );
    set_field(
        server,
        "history",
        array_value(crate::array::js_array_alloc(0)),
    );
    set_field(server, "historySize", 30.0);
    install_server_methods(server);
    set_hidden_value(server_value, KEY_PROMPT, string_value(&prompt));
    set_hidden_value(
        server_value,
        KEY_COMMANDS,
        object_value(js_object_alloc(0, 0)),
    );

    display_prompt_for(server_value);
    server_value
}

#[no_mangle]
pub extern "C" fn js_repl_recoverable_new(err: f64) -> f64 {
    let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    let recoverable = crate::error::js_syntaxerror_new(empty);
    crate::node_submodules::set_error_user_prop(recoverable as usize, "err", err);
    mark_recoverable_error(recoverable);
    crate::value::js_nanbox_pointer(recoverable as i64)
}

pub fn is_recoverable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let gc_header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_ERROR {
            return false;
        }
        if !is_marked_recoverable_error(raw) {
            return false;
        }
        let err = raw as *mut crate::error::ErrorHeader;
        (*err).error_kind == crate::error::ERROR_KIND_SYNTAX_ERROR
            && crate::node_submodules::error_user_prop(raw, "err").is_some()
    }
}

pub fn is_repl_server_value(value: f64) -> bool {
    object_ptr_from_value(value)
        .map(|obj| {
            let server = object_value(obj);
            get_prop(server, "context").to_bits() != TAG_UNDEFINED
                && get_hidden_value(server, KEY_COMMANDS).to_bits() != TAG_UNDEFINED
        })
        .unwrap_or(false)
}
