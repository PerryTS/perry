use super::*;

#[test]
fn win_kill_probe_and_terminate() {
    let mut child = std::process::Command::new("ping")
        .args(["-n", "30", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ping");
    let proc_handle = cp_win_dup_proc_handle(&child);
    assert_ne!(proc_handle, 0, "DuplicateHandle should succeed");
    assert!(cp_win_kill(proc_handle, 0));
    assert!(cp_win_kill(proc_handle, 15));
    let status = child.wait().expect("wait after TerminateProcess");
    assert_eq!(status.code(), Some(1));
    assert!(!cp_win_kill(proc_handle, 0));
    assert!(!cp_win_kill(proc_handle, 15));
    unsafe {
        let _ = windows_sys::Win32::Foundation::CloseHandle(
            proc_handle as windows_sys::Win32::Foundation::HANDLE,
        );
    }
}

fn handle_is_live(handle: u64) -> bool {
    cp_live_lock()
        .as_ref()
        .is_some_and(|map| map.contains_key(&handle))
}

#[test]
fn reactor_kill_terminates_live_child() {
    let cmd = "ping";
    let cmd_ptr = crate::string::js_string_from_bytes(cmd.as_ptr(), cmd.len() as u32);
    let mut args = crate::array::js_array_alloc(3);
    for a in ["-n", "30", "127.0.0.1"] {
        let s = crate::string::js_string_from_bytes(a.as_ptr(), a.len() as u32);
        args = crate::array::js_array_push_f64(args, crate::value::js_nanbox_string(s as i64));
    }
    let start = std::time::Instant::now();
    let cp = js_child_process_spawn_streams(cmd_ptr as i64, args as i64, 0);
    let pid = cp_get_field(cp, b"pid");
    assert!(pid > 0.0, "spawn should set a real pid, got {pid}");
    let handle = cp_get_field(cp, b"__cpHandle") as u64;
    assert!(handle_is_live(handle));
    cp_reactor_pump();
    assert!(cp_live_kill(handle, cp_undefined()));
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while handle_is_live(handle) {
        assert!(std::time::Instant::now() < deadline);
        cp_reactor_pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(start.elapsed() < Duration::from_secs(20));
    assert_eq!(
        cp_get_field(cp, b"exitCode").to_bits(),
        TAG_NULL_F64.to_bits()
    );
    assert_eq!(
        cp_value_to_string(cp_get_field(cp, b"signalCode")).as_deref(),
        Some("SIGTERM")
    );
    assert!(!cp_live_kill(handle, cp_undefined()));
}
