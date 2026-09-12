//! Shared JS facade acceptance: all import aliases, subscriptions, pause/resume,
//! output before exit, and registry cleanup on both PTY backends.
use super::{pty_handle_of, reactor};
use crate::child_process::*;
use crate::closure::ClosureHeader;
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

#[cfg(test)]
thread_local! {
    static OUTPUT: RefCell<String> = const { RefCell::new(String::new()) };
    static EXITS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    static REMOVED_CALLS: Cell<usize> = const { Cell::new(0) };
}

extern "C" fn removed(_closure: *const ClosureHeader, _chunk: f64) -> f64 {
    REMOVED_CALLS.with(|n| n.set(n.get() + 1));
    cp_undefined()
}

extern "C" fn data(_closure: *const ClosureHeader, chunk: f64) -> f64 {
    OUTPUT.with(|o| o.borrow_mut().push_str(&cp_value_to_string(chunk).unwrap()));
    cp_undefined()
}

extern "C" fn exited(_closure: *const ClosureHeader, event: f64) -> f64 {
    EXITS.with(|e| e.borrow_mut().push(cp_get_field(event, b"exitCode")));
    cp_undefined()
}

fn callback(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> f64 {
    crate::closure::js_register_closure_arity(f as *const u8, 1);
    cp_box_ptr(crate::closure::js_closure_alloc(f as *const u8, 0).cast())
}

fn call(target: f64, name: &[u8], args: &[f64]) -> f64 {
    unsafe {
        crate::closure::js_native_call_value(cp_get_field(target, name), args.as_ptr(), args.len())
    }
}

#[test]
fn aliases_share_native_spawn_and_paused_output_precedes_exit() {
    extern "C" {
        fn js_nm_install_node_pty();
    }
    unsafe {
        js_nm_install_node_pty();
    }
    for alias in ["node-pty", "@lydell/node-pty", "bun-pty"] {
        OUTPUT.with(|o| o.borrow_mut().clear());
        EXITS.with(|e| e.borrow_mut().clear());
        REMOVED_CALLS.with(|n| n.set(0));
        let scope = crate::gc::RuntimeHandleScope::new();
        let ns = scope.root_nanbox_f64(crate::object::js_create_native_module_namespace(
            alias.as_ptr(),
            alias.len(),
        ));
        let spawn = scope.root_nanbox_f64(cp_get_field(ns.get_nanbox_f64(), b"spawn"));
        #[cfg(windows)]
        let (shell, args, script) = (
            "cmd.exe",
            vec!["/d", "/q"],
            "echo %PERRY_PTY_MARKER%\r\nexit\r\n",
        );
        #[cfg(unix)]
        let (shell, args, script) = ("sh", vec![], "printf '%s\\n' \"$PERRY_PTY_MARKER\"\nexit\n");
        let mut argv = crate::array::js_array_alloc(args.len() as u32);
        for arg in args {
            argv = crate::array::js_array_push_f64(argv, cp_box_string(arg));
        }
        let argv = scope.root_nanbox_f64(cp_box_ptr(argv.cast()));
        let env = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
        for (key, value) in std::env::vars() {
            cp_set_field(env.get_nanbox_f64(), key.as_bytes(), cp_box_string(&value));
        }
        cp_set_field(
            env.get_nanbox_f64(),
            b"PERRY_PTY_MARKER",
            cp_box_string("native_alias_8512"),
        );
        let opts = scope.root_nanbox_f64(cp_box_ptr(crate::object::js_object_alloc(0, 0).cast()));
        cp_set_field(opts.get_nanbox_f64(), b"env", env.get_nanbox_f64());
        let args = [
            cp_box_string(shell),
            argv.get_nanbox_f64(),
            opts.get_nanbox_f64(),
        ];
        let term = scope.root_nanbox_f64(unsafe {
            crate::closure::js_native_call_value(spawn.get_nanbox_f64(), args.as_ptr(), args.len())
        });
        assert!(cp_get_field(term.get_nanbox_f64(), b"pid") > 0.0);
        let handle = pty_handle_of(term.get_nanbox_f64()).unwrap();
        let disposable = call(term.get_nanbox_f64(), b"onData", &[callback(removed)]);
        call(disposable, b"dispose", &[]);
        call(term.get_nanbox_f64(), b"onData", &[callback(data)]);
        call(term.get_nanbox_f64(), b"onExit", &[callback(exited)]);
        call(term.get_nanbox_f64(), b"pause", &[]);
        call(term.get_nanbox_f64(), b"write", &[cp_box_string(script)]);
        let paused = Instant::now() + Duration::from_millis(150);
        while Instant::now() < paused {
            reactor::pty_reactor_pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(OUTPUT.with(|o| o.borrow().is_empty()));
        assert!(EXITS.with(|e| e.borrow().is_empty()));
        call(term.get_nanbox_f64(), b"resume", &[]);
        let deadline = Instant::now() + Duration::from_secs(15);
        while reactor::pty_live_count_for_test() != 0 && Instant::now() < deadline {
            reactor::pty_reactor_pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        if reactor::pty_live_count_for_test() != 0 {
            reactor::pty_live_kill(handle, 9);
        }
        assert_eq!(reactor::pty_live_count_for_test(), 0, "{alias}: leaked PTY");
        assert_eq!(EXITS.with(|e| e.borrow().clone()), vec![0.0], "{alias}");
        let output = OUTPUT.with(|o| o.borrow().clone());
        assert!(output.contains("native_alias_8512"), "{alias}: {output:?}");
        assert_eq!(
            REMOVED_CALLS.with(Cell::get),
            0,
            "{alias}: disposed listener fired"
        );
    }
}
