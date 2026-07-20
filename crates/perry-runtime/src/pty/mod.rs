//! Runtime-native pty under the node-pty JS shape (#6563).
//!
//! Importable as BOTH `node-pty` (kimi-code's dynamic `import("node-pty")`)
//! and `@lydell/node-pty` (opencode's static import) — the alias is folded in
//! `object/native_module.rs::normalize_native_module_alias` and the codegen
//! table (`lower_call/native_table/extras.rs`), so a single implementation
//! serves both package names. There is no N-API host involved: the pty is
//! implemented directly in the runtime (`native.rs`) and pumped through the
//! event loop (`reactor.rs`), following `child_process`'s reactor pattern.
//!
//! JS surface (the subset the two target apps touch — see the issue):
//!
//! ```text
//! spawn(file, args, { name, cols, rows, cwd, env }) -> IPty
//! IPty: pid, cols, rows, process, handleFlowControl,
//!       onData(cb) -> { dispose() }, onExit(cb({ exitCode, signal })) -> { dispose() },
//!       resize(cols, rows), write(data), kill(signal?), pause(), resume()
//! ```
//!
//! `onData` delivers *strings* (UTF-8, with split-sequence carry-over), and
//! `onExit` fires `{ exitCode, signal }` — `signal` is the numeric signo for
//! a signal death, `undefined` otherwise — matching node-pty's unix binding.
//!
//! Windows/ConPTY is out of scope for this stage: on non-unix hosts
//! `js_pty_spawn` throws a descriptive `Error` (the same failure mode the
//! real node-pty has when its prebuilt addon is missing), so a consumer's
//! dynamic-import fallback path still engages.

#[cfg(unix)]
mod native;
#[cfg(unix)]
pub(crate) mod reactor;

#[cfg(unix)]
pub use unix_impl::js_pty_spawn;
#[cfg(unix)]
pub(crate) use unix_impl::pty_emit;

#[cfg(unix)]
mod unix_impl {
    use super::{native, reactor};
    use crate::child_process::{
        cp_array_ptr, cp_box_ptr, cp_box_string, cp_build_object, cp_cast0, cp_cast1, cp_cast2,
        cp_get_field, cp_make_error, cp_object_ptr, cp_set_field, cp_this, cp_undefined,
        cp_value_to_bytes, cp_value_to_string, CpFn,
    };
    use crate::closure::{js_native_call_value, ClosureHeader};
    use crate::object::js_implicit_this_set;
    use crate::value::JSValue;

    // Shape-id band kept clear of cluster (0x7FFF_FC80) and child_process
    // (0x7FFF_FD00+); see child_process/builder.rs for the neighboring bands.
    const PTY_SHAPE_ID: u32 = 0x7FFF_FCC0;
    const PTY_DISPOSABLE_SHAPE_ID: u32 = 0x7FFF_FCE0;

    /// Hidden field key holding the listener array for `event`.
    fn pty_listener_key(event: &str) -> Vec<u8> {
        let mut k = b"__ptyL_".to_vec();
        k.extend_from_slice(event.as_bytes());
        k
    }

    /// Append a listener closure to `target`'s `event` list.
    pub(crate) fn pty_register(target: f64, event: &str, cb: f64) {
        let key = pty_listener_key(event);
        let arr = match cp_array_ptr(cp_get_field(target, &key)) {
            Some(a) => a,
            None => crate::array::js_array_alloc(2),
        };
        let arr = crate::array::js_array_push_f64(arr, cb);
        cp_set_field(target, &key, cp_box_ptr(arr as *const u8));
    }

    /// Invoke every listener registered on `target` for `event`. The listener
    /// array is re-read each iteration so a moving GC during a handler call
    /// can't strand us on a stale array pointer.
    pub(crate) fn pty_emit(target: f64, event: &str, args: &[f64]) {
        let key = pty_listener_key(event);
        let mut i: u32 = 0;
        loop {
            let arr = match cp_array_ptr(cp_get_field(target, &key)) {
                Some(a) => a,
                None => break,
            };
            if i >= crate::array::js_array_length(arr) {
                break;
            }
            let cb = crate::array::js_array_get_f64(arr, i);
            let prev = js_implicit_this_set(target);
            unsafe {
                let _ = js_native_call_value(cb, args.as_ptr(), args.len());
            }
            js_implicit_this_set(prev);
            i += 1;
        }
    }

    /// Remove one listener (matched by NaN-boxed bits) from `target`'s
    /// `event` list — the `dispose()` body of the object `onData`/`onExit`
    /// return.
    fn pty_remove_listener(target: f64, event: &str, cb: f64) {
        let key = pty_listener_key(event);
        if let Some(arr) = cp_array_ptr(cp_get_field(target, &key)) {
            let n = crate::array::js_array_length(arr);
            let mut out = crate::array::js_array_alloc(n);
            for i in 0..n {
                let v = crate::array::js_array_get_f64(arr, i);
                if v.to_bits() != cb.to_bits() {
                    out = crate::array::js_array_push_f64(out, v);
                }
            }
            cp_set_field(target, &key, cp_box_ptr(out as *const u8));
        }
    }

    /// Build the `{ dispose() }` handle returned by `onData`/`onExit`. The
    /// subscription triple is stored as fields on the handle itself
    /// (GC-traced through the object), read back by the dispose body.
    fn pty_make_disposable(target: f64, event: &str, cb: f64) -> f64 {
        let methods: [(&str, CpFn); 1] = [("dispose", cp_cast0(pty_disposable_dispose))];
        let obj = cp_build_object(&methods, PTY_DISPOSABLE_SHAPE_ID + methods.len() as u32);
        let val = cp_box_ptr(obj as *const u8);
        cp_set_field(val, b"__ptyTarget", target);
        cp_set_field(val, b"__ptyEvent", cp_box_string(event));
        cp_set_field(val, b"__ptyCb", cb);
        val
    }

    extern "C" fn pty_disposable_dispose(closure: *const ClosureHeader) -> f64 {
        let this = cp_this(closure);
        let target = cp_get_field(this, b"__ptyTarget");
        let cb = cp_get_field(this, b"__ptyCb");
        if let Some(event) = cp_value_to_string(cp_get_field(this, b"__ptyEvent")) {
            pty_remove_listener(target, &event, cb);
        }
        cp_undefined()
    }

    /// Read the reactor registry key (`__ptyHandle`) off an IPty. `None` for
    /// a foreign object.
    fn pty_handle_of(this: f64) -> Option<u64> {
        let h = cp_get_field(this, b"__ptyHandle");
        if JSValue::from_bits(h.to_bits()).is_undefined() {
            return None;
        }
        if h.is_finite() && h >= 0.0 {
            Some(h as u64)
        } else {
            None
        }
    }

    // ----- IPty method bodies (slot 0 of each closure = the IPty object) ----

    extern "C" fn pty_method_on_data(closure: *const ClosureHeader, cb: f64) -> f64 {
        let this = cp_this(closure);
        pty_register(this, "data", cb);
        pty_make_disposable(this, "data", cb)
    }

    extern "C" fn pty_method_on_exit(closure: *const ClosureHeader, cb: f64) -> f64 {
        let this = cp_this(closure);
        pty_register(this, "exit", cb);
        pty_make_disposable(this, "exit", cb)
    }

    extern "C" fn pty_method_write(closure: *const ClosureHeader, data: f64) -> f64 {
        let this = cp_this(closure);
        if let Some(handle) = pty_handle_of(this) {
            let bytes = cp_value_to_bytes(data);
            if !bytes.is_empty() {
                reactor::pty_live_write(handle, &bytes);
            }
        }
        cp_undefined()
    }

    extern "C" fn pty_method_resize(closure: *const ClosureHeader, cols: f64, rows: f64) -> f64 {
        let this = cp_this(closure);
        let cols_i = pty_arg_i32(cols);
        let rows_i = pty_arg_i32(rows);
        if cols_i <= 0 || rows_i <= 0 || cols_i > u16::MAX as i32 || rows_i > u16::MAX as i32 {
            // node-pty's exact guard message.
            crate::exception::js_throw(cp_make_error(
                "resizing must be done using positive cols and rows",
                &[],
            ));
        }
        if let Some(handle) = pty_handle_of(this) {
            if reactor::pty_live_resize(handle, cols_i as u16, rows_i as u16) {
                cp_set_field(this, b"cols", cols_i as f64);
                cp_set_field(this, b"rows", rows_i as f64);
            }
        }
        cp_undefined()
    }

    extern "C" fn pty_method_kill(closure: *const ClosureHeader, signal: f64) -> f64 {
        let this = cp_this(closure);
        if let Some(handle) = pty_handle_of(this) {
            reactor::pty_live_kill(handle, pty_parse_kill_signal(signal));
        }
        cp_undefined()
    }

    extern "C" fn pty_method_noop0(closure: *const ClosureHeader) -> f64 {
        let _ = closure;
        cp_undefined()
    }

    /// opencode's `Pty` wrapper calls `dispose()`; node-pty's UnixTerminal
    /// exposes the equivalent `destroy()`. Both are "hang up the terminal":
    /// kill with the default SIGHUP.
    extern "C" fn pty_method_dispose(closure: *const ClosureHeader) -> f64 {
        pty_method_kill(closure, cp_undefined())
    }

    /// `kill([signal])` — node-pty defaults to `SIGHUP` (a hangup is how a
    /// real terminal disappears), unlike child_process's `SIGTERM`.
    /// `undefined` and the `0.0` missing-arg padding both mean "default".
    fn pty_parse_kill_signal(signal: f64) -> i32 {
        let bits = signal.to_bits();
        if JSValue::from_bits(bits).is_undefined() || bits == 0 {
            return libc::SIGHUP;
        }
        crate::child_process::cp_signal_from_value(signal)
    }

    /// Best-effort i32 from a NaN-boxed numeric argument.
    fn pty_arg_i32(v: f64) -> i32 {
        let bits = v.to_bits();
        if (bits >> 48) == 0x7FFE {
            return (bits & 0xFFFF_FFFF) as u32 as i32;
        }
        if v.is_finite() {
            v as i32
        } else {
            0
        }
    }

    /// Read an optional positive-i32 option field, falling back to `default`.
    fn pty_i32_field(opts: f64, name: &[u8], default: i32) -> i32 {
        let v = cp_get_field(opts, name);
        if JSValue::from_bits(v.to_bits()).is_undefined() {
            return default;
        }
        let n = pty_arg_i32(v);
        if n > 0 && n <= u16::MAX as i32 {
            n
        } else {
            default
        }
    }

    fn pty_string_field(opts: f64, name: &[u8]) -> Option<String> {
        let v = cp_get_field(opts, name);
        if JSValue::from_bits(v.to_bits()).is_undefined() {
            return None;
        }
        cp_value_to_string(v).filter(|s| !s.is_empty())
    }

    /// Read `options.env` as the FULL child environment (node-pty semantics:
    /// `options.env || process.env`). `None` → inherit the parent env.
    fn pty_env_field(opts: f64) -> Option<Vec<(String, String)>> {
        let env_val = cp_get_field(opts, b"env");
        let obj = cp_object_ptr(env_val)?;
        let keys = crate::object::js_object_keys(obj);
        if keys.is_null() {
            return Some(Vec::new());
        }
        let n = crate::array::js_array_length(keys);
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let Some(k) = cp_value_to_string(crate::array::js_array_get_f64(keys, i)) else {
                continue;
            };
            let v = cp_get_field(env_val, k.as_bytes());
            if JSValue::from_bits(v.to_bits()).is_undefined() {
                continue;
            }
            out.push((k, crate::child_process::cp_coerce_string(v)));
        }
        Some(out)
    }

    /// Coerce the `args` argument: an array of strings, a single string
    /// (node-pty accepts both), or `undefined`/anything else → no args.
    fn pty_args_from_value(v: f64) -> Vec<String> {
        if cp_array_ptr(v).is_some() {
            return crate::child_process::cp_args_from_value(v);
        }
        match cp_value_to_string(v) {
            Some(s) if !s.is_empty() => vec![s],
            _ => Vec::new(),
        }
    }

    fn pty_register_arities() {
        use crate::closure::js_register_closure_arity;
        js_register_closure_arity(pty_method_on_data as *const u8, 1);
        js_register_closure_arity(pty_method_on_exit as *const u8, 1);
        js_register_closure_arity(pty_method_write as *const u8, 1);
        js_register_closure_arity(pty_method_resize as *const u8, 2);
        js_register_closure_arity(pty_method_kill as *const u8, 1);
        js_register_closure_arity(pty_method_noop0 as *const u8, 0);
        js_register_closure_arity(pty_method_dispose as *const u8, 0);
        js_register_closure_arity(pty_disposable_dispose as *const u8, 0);
    }

    /// Build the IPty object shell (methods only; live fields are set by
    /// `js_pty_spawn` after the fork succeeds).
    fn pty_build_ipty() -> f64 {
        let methods: [(&str, CpFn); 8] = [
            ("onData", cp_cast1(pty_method_on_data)),
            ("onExit", cp_cast1(pty_method_on_exit)),
            ("write", cp_cast1(pty_method_write)),
            ("resize", cp_cast2(pty_method_resize)),
            ("kill", cp_cast1(pty_method_kill)),
            ("pause", cp_cast0(pty_method_noop0)),
            ("resume", cp_cast0(pty_method_noop0)),
            ("dispose", cp_cast0(pty_method_dispose)),
        ];
        let obj = cp_build_object(&methods, PTY_SHAPE_ID + methods.len() as u32);
        cp_box_ptr(obj as *const u8)
    }

    /// `spawn(file, args, options)` — the node-pty entry point. Args arrive
    /// as raw NaN-box bits (NA_JSV) so `undefined` slots survive the FFI
    /// boundary.
    ///
    /// Allocates a pty pair, forks with the slave as the child's controlling
    /// terminal (TERM set from `options.name`), and registers the
    /// reader/waiter threads with the reactor. Returns the IPty object.
    #[no_mangle]
    pub extern "C" fn js_pty_spawn(file_bits: i64, args_bits: i64, opts_bits: i64) -> f64 {
        pty_register_arities();

        let file_val = f64::from_bits(file_bits as u64);
        let Some(file) = cp_value_to_string(file_val).filter(|s| !s.is_empty()) else {
            crate::exception::js_throw(cp_make_error("spawn: file argument is required", &[]));
        };
        let args = pty_args_from_value(f64::from_bits(args_bits as u64));
        let opts = f64::from_bits(opts_bits as u64);

        let name = pty_string_field(opts, b"name").unwrap_or_else(|| "xterm".to_string());
        let cols = pty_i32_field(opts, b"cols", 80) as u16;
        let rows = pty_i32_field(opts, b"rows", 24) as u16;
        let cwd = pty_string_field(opts, b"cwd");

        // Child env: options.env (full replacement) or the parent
        // environment, with TERM forced to the requested terminal name.
        let mut env = pty_env_field(opts).unwrap_or_else(|| std::env::vars().collect());
        env.retain(|(k, _)| k != "TERM");
        env.push(("TERM".to_string(), name));

        let ipty = pty_build_ipty();

        let child = match native::spawn_in_pty(&native::PtySpawnRequest {
            file: file.clone(),
            args,
            env,
            cwd,
            cols,
            rows,
        }) {
            Ok(c) => c,
            Err(e) => {
                crate::exception::js_throw(cp_make_error(
                    &format!("spawn {file} failed: {e}"),
                    &[],
                ));
            }
        };

        cp_set_field(ipty, b"pid", child.pid as f64);
        cp_set_field(ipty, b"cols", cols as f64);
        cp_set_field(ipty, b"rows", rows as f64);
        // node-pty's `process` is the terminal's foreground process name;
        // the spawned file's basename is the faithful static answer.
        let proc_name = file.rsplit('/').next().unwrap_or(&file);
        cp_set_field(ipty, b"process", cp_box_string(proc_name));
        cp_set_field(
            ipty,
            b"handleFlowControl",
            crate::child_process::TAG_FALSE_F64,
        );

        let handle = reactor::pty_register_live(ipty, child);
        cp_set_field(ipty, b"__ptyHandle", handle as f64);

        ipty
    }

    #[cfg(test)]
    pub(super) mod tests {
        use super::*;
        use crate::string::js_string_from_bytes;
        use std::sync::Mutex;

        static DATA_SINK: Mutex<String> = Mutex::new(String::new());
        static EXIT_SINK: Mutex<Option<(f64, f64)>> = Mutex::new(None);
        /// The two tests below share the global reactor (live count, event
        /// queue) and the sinks above — serialize them so a parallel test
        /// runner can't interleave their pumps.
        static TEST_LOCK: Mutex<()> = Mutex::new(());

        extern "C" fn test_data_listener(_closure: *const ClosureHeader, chunk: f64) -> f64 {
            if let Some(s) = cp_value_to_string(chunk) {
                DATA_SINK.lock().unwrap().push_str(&s);
            }
            cp_undefined()
        }

        extern "C" fn test_exit_listener(_closure: *const ClosureHeader, payload: f64) -> f64 {
            let code = cp_get_field(payload, b"exitCode");
            let signal = cp_get_field(payload, b"signal");
            *EXIT_SINK.lock().unwrap() = Some((code, signal));
            cp_undefined()
        }

        fn listener_value(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> f64 {
            crate::closure::js_register_closure_arity(f as *const u8, 1);
            let closure = crate::closure::js_closure_alloc(f as *const u8, 0);
            cp_box_ptr(closure as *const u8)
        }

        fn boxed_str(s: &str) -> f64 {
            let sh = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            crate::value::js_nanbox_string(sh as i64)
        }

        fn pump_until_closed(deadline_secs: u64) {
            let start = std::time::Instant::now();
            while reactor::pty_live_count_for_test() > 0 {
                reactor::pty_reactor_pump();
                assert!(
                    start.elapsed().as_secs() < deadline_secs,
                    "pty did not close within {deadline_secs}s"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // One final pump in case the close landed after the last check.
            reactor::pty_reactor_pump();
        }

        /// Full-stack (JS-object level) e2e: spawn a shell in a pty,
        /// round-trip `printf`, and observe `onExit {exitCode: 0}` through
        /// the reactor pump — what compiled TS does, minus codegen.
        #[test]
        fn js_pty_spawn_shell_data_and_exit() {
            let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            DATA_SINK.lock().unwrap().clear();
            *EXIT_SINK.lock().unwrap() = None;

            let opts = unsafe {
                crate::value::js_nanbox_pointer(crate::child_process::make_two_field_object(
                    "cols", 80.0, "rows", 24.0,
                ) as i64)
            };
            let ipty = js_pty_spawn(
                boxed_str("sh").to_bits() as i64,
                cp_undefined().to_bits() as i64,
                opts.to_bits() as i64,
            );

            assert!(cp_get_field(ipty, b"pid") > 0.0, "pid must be set");
            assert_eq!(cp_get_field(ipty, b"cols"), 80.0);
            assert_eq!(cp_get_field(ipty, b"rows"), 24.0);

            pty_register(ipty, "data", listener_value(test_data_listener));
            pty_register(ipty, "exit", listener_value(test_exit_listener));

            let handle = pty_handle_of(ipty).expect("__ptyHandle set");
            // Output prints PTY_OK while the echoed command shows PTY_%s, so
            // the assertion can't pass on terminal echo alone.
            assert!(reactor::pty_live_write(
                handle,
                b"printf 'PTY_%s\\n' OK\nexit\n"
            ));

            pump_until_closed(15);

            let data = DATA_SINK.lock().unwrap().clone();
            assert!(
                data.contains("PTY_OK"),
                "onData must carry output: {data:?}"
            );
            let (code, signal) = EXIT_SINK.lock().unwrap().expect("onExit fired");
            assert_eq!(code, 0.0, "clean exit code");
            assert!(
                JSValue::from_bits(signal.to_bits()).is_undefined(),
                "no signal on clean exit"
            );
        }

        /// kill(SIGTERM) must surface as `onExit {signal: 15}`.
        #[test]
        fn js_pty_kill_reports_signal() {
            let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            *EXIT_SINK.lock().unwrap() = None;

            let ipty = js_pty_spawn(
                boxed_str("sleep").to_bits() as i64,
                boxed_str("30").to_bits() as i64,
                cp_undefined().to_bits() as i64,
            );
            pty_register(ipty, "exit", listener_value(test_exit_listener));
            let handle = pty_handle_of(ipty).expect("__ptyHandle set");

            std::thread::sleep(std::time::Duration::from_millis(150));
            assert!(reactor::pty_live_kill(handle, libc::SIGTERM));

            pump_until_closed(15);

            let (code, signal) = EXIT_SINK.lock().unwrap().expect("onExit fired");
            assert_eq!(
                signal,
                libc::SIGTERM as f64,
                "signal death must be reported"
            );
            assert_eq!(code, 0.0, "node-pty reports exitCode 0 for signal deaths");
        }
    }
}

/// Windows/ConPTY stub (#6563 stage 2): throw a descriptive error so
/// consumers' import-failure fallbacks (kimi's non-pty terminal backend)
/// engage instead of crashing.
#[cfg(not(unix))]
#[no_mangle]
pub extern "C" fn js_pty_spawn(_file_bits: i64, _args_bits: i64, _opts_bits: i64) -> f64 {
    crate::exception::js_throw(crate::child_process::cp_make_error(
        "node-pty: this platform is not supported yet by the perry runtime (POSIX only; ConPTY tracked in #6563)",
        &[],
    ));
}
