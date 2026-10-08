//! First-execution recording for `perry compile --record-function-order`.
//!
//! A recording build calls [`js_function_order_first_call`] at the top of
//! every generated function with that function's private record: one flag
//! byte followed by the NUL-terminated symbol name, both in the binary's own
//! data (see perry-codegen `function_order`). The first call flips the flag
//! and appends the name to the file named by `PERRY_FUNCTION_ORDER_OUT`, so
//! the file lists functions in the order they first ran: the input to
//! `perry compile --function-order`.
//!
//! Each name goes out as one `O_APPEND` write, so a run that is killed (a
//! TUI stopped at its prompt) keeps everything it recorded. Normal builds
//! never reference this function.

use std::ffi::{c_char, CStr};
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

static OUTPUT: OnceLock<Option<File>> = OnceLock::new();

fn open_output() -> Option<File> {
    let path = std::env::var_os("PERRY_FUNCTION_ORDER_OUT")?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
}

/// # Safety
/// `record` points at a generated record: a writable flag byte followed by a
/// NUL-terminated name.
#[no_mangle]
pub unsafe extern "C" fn js_function_order_first_call(record: *mut u8) {
    let flag = &*(record as *const AtomicU8);
    if flag.load(Ordering::Relaxed) != 0 || flag.swap(1, Ordering::Relaxed) != 0 {
        return;
    }
    let Some(file) = OUTPUT.get_or_init(open_output).as_ref() else {
        return;
    };
    let name = CStr::from_ptr(record.add(1) as *const c_char).to_bytes();
    let mut line = Vec::with_capacity(name.len() + 1);
    line.extend_from_slice(name);
    line.push(b'\n');
    let _ = (&*file).write_all(&line);
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_FIRST_CALL: unsafe extern "C" fn(*mut u8) = js_function_order_first_call;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_written_once() {
        let dir = std::env::temp_dir().join(format!("perry-fnorder-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("order.txt");
        let _ = std::fs::remove_file(&out);
        // The output is resolved once per process; this is the only test that
        // sets it.
        std::env::set_var("PERRY_FUNCTION_ORDER_OUT", &out);
        let mut a = *b"\0perry_fn_a\0";
        let mut b = *b"\0perry_fn_b\0";
        unsafe {
            js_function_order_first_call(a.as_mut_ptr());
            js_function_order_first_call(b.as_mut_ptr());
            js_function_order_first_call(a.as_mut_ptr());
        }
        assert_eq!(a[0], 1);
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "perry_fn_a\nperry_fn_b\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
