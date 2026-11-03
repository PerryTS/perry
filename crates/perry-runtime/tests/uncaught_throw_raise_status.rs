//! An uncaught `throw` must take the uncaught path (report on stderr, exit
//! status 1) whatever status `_Unwind_RaiseException` hands back.
//!
//! On aarch64 Linux with glibc's libgcc_s the status of a raise that found no
//! handler is the low half of the exception object's address, never
//! `_URC_END_OF_STACK`, and `js_throw` used to read that as a transport failure
//! (`perry: FATAL: exception transport failed`) and abort. The bad status is
//! target-specific, so this test reproduces it anywhere glibc's unwinder is
//! used: the test executable defines `_Unwind_RaiseException` itself (the
//! executable's definition wins over libgcc_s's), forwards to the real one, and
//! hands the caller the exception pointer instead of the real status whenever
//! the raise returns, which only happens when it found no handler.
//!
//! The throw runs on a raw pthread: `std::thread` and the test harness wrap the
//! body in `catch_unwind`, whose landing pad would accept the foreign exception
//! and there would be no "no handler" outcome to classify. A glibc thread start
//! frame has no handler, so the search ends at the outermost frame.

#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::ffi::c_void;
use std::process::Command;

const CHILD_ENV: &str = "PERRY_TEST_UNCAUGHT_THROW_CHILD";
const SHIM_MARKER: &str = "[shim] raise returned a garbage status";

/// The real unwinder, with the garbage status of the failing libgcc build
/// substituted for whatever a raise that returns reports.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C-unwind" fn _Unwind_RaiseException(exception: *mut c_void) -> i32 {
    type Raise = unsafe extern "C-unwind" fn(*mut c_void) -> i32;
    let real = unsafe { libc::dlsym(libc::RTLD_NEXT, c"_Unwind_RaiseException".as_ptr()) };
    if real.is_null() {
        unsafe { libc::abort() }
    }
    let real: Raise = unsafe { std::mem::transmute(real) };
    // Returns only when no handler accepted the exception.
    let _real_status = unsafe { real(exception) };
    let marker = format!("{SHIM_MARKER}\n");
    unsafe { libc::write(2, marker.as_ptr().cast(), marker.len()) };
    exception as usize as u32 as i32
}

extern "C-unwind" fn throw_on_a_bare_thread(_arg: *mut c_void) -> *mut c_void {
    perry_runtime::gc::initialize_runtime_fixture();
    perry_runtime::exception::js_throw(7.0)
}

fn throw_with_no_handler_anywhere() -> ! {
    type Start = extern "C" fn(*mut c_void) -> *mut c_void;
    // Same ABI as `extern "C"`, but without the abort-on-unwind guard `extern
    // "C"` adds at the function boundary (its filter pad would "accept" the
    // exception during the search phase).
    let start: Start = unsafe {
        std::mem::transmute::<extern "C-unwind" fn(*mut c_void) -> *mut c_void, Start>(
            throw_on_a_bare_thread,
        )
    };
    let mut thread: libc::pthread_t = 0;
    unsafe {
        assert_eq!(
            libc::pthread_create(&mut thread, std::ptr::null(), start, std::ptr::null_mut()),
            0
        );
        libc::pthread_join(thread, std::ptr::null_mut());
    }
    // Unreachable when the throw takes the uncaught path (it exits the process).
    std::process::exit(99)
}

#[test]
fn an_uncaught_throw_exits_with_the_report_when_the_raise_status_is_garbage() {
    if std::env::var_os(CHILD_ENV).is_some() {
        throw_with_no_handler_anywhere();
    }
    let output = Command::new(std::env::current_exe().expect("test executable path"))
        .args([
            "--exact",
            "an_uncaught_throw_exits_with_the_report_when_the_raise_status_is_garbage",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, "1")
        .output()
        .expect("spawn the throwing child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(SHIM_MARKER),
        "the subject was not live: the raise never returned a substituted status\n{stderr}"
    );
    assert!(
        !stderr.contains("exception transport failed"),
        "a raise that found no handler was reported as a transport failure\n{stderr}"
    );
    assert!(
        stderr.contains("Uncaught exception"),
        "the uncaught report is missing\n{stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "an uncaught throw exits with status 1, not an abort\n{stderr}"
    );
}
