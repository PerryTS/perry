use super::*;
use std::time::Duration;

#[test]
fn console_fixture() {
    if std::env::var("PERRY_8512_CONSOLE_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    use std::io::BufRead;
    use windows_sys::Win32::System::Console::{
        GetConsoleScreenBufferInfo, GetStdHandle, CONSOLE_SCREEN_BUFFER_INFO, STD_OUTPUT_HANDLE,
    };
    println!("CONSOLE_READY");
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().trim() {
            "size" => {
                let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { zeroed() };
                assert_ne!(
                    unsafe {
                        GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE), &mut info)
                    },
                    0
                );
                println!("CONSOLE_SIZE:{}x{}", info.dwSize.X, info.dwSize.Y);
            }
            "tree" => {
                let mut child = Command::new("ping.exe")
                    .args(["-n", "30", "127.0.0.1"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap();
                println!("CONSOLE_CHILD:{}:END", child.id());
                child.wait().unwrap();
            }
            _ => {}
        }
    }
}

fn request() -> PtySpawnRequest {
    PtySpawnRequest {
        file: std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
        args: vec!["/d".into(), "/q".into()],
        env: std::env::vars().collect(),
        cwd: None,
        cols: 80,
        rows: 24,
    }
}

fn drain(session: Arc<PtySession>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buf = [0; 8192];
        loop {
            match read_pty(&session, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => output.extend_from_slice(&buf[..n]),
            }
        }
        let _ = tx.send(String::from_utf8_lossy(&output).into_owned());
    });
    rx
}

fn wait(session: Arc<PtySession>) -> mpsc::Receiver<(Option<i32>, Option<i32>)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(wait_child(session));
    });
    rx
}

#[test]
fn conpty_echo_resize_cwd_env_exit() {
    let mut req = request();
    req.env
        .push(("PERRY_PTY_MARKER".into(), "native_8512_roundtrip".into()));
    let cwd = std::env::temp_dir();
    req.cwd = Some(cwd.to_string_lossy().into_owned());
    let child = spawn_in_pty(&req).expect("spawn ConPTY shell");
    let output = drain(child.master.clone());
    let exited = wait(child.master.clone());
    assert!(resize_pty(child.master.clone(), 120, 40));
    assert!(!resize_pty(child.master.clone(), 0, 40));
    assert!(write_pty(
        &child.master,
        b"echo %PERRY_PTY_MARKER%\r\ncd\r\nexit 7\r\n"
    ));
    let status = exited.recv_timeout(Duration::from_secs(15));
    if status.is_err() {
        signal_pty(&child.master, 9);
    }
    let output = output
        .recv_timeout(Duration::from_secs(5))
        .expect("final frame and EOF");
    assert_eq!(status.unwrap(), (Some(7), None), "{output:?}");
    assert!(output.contains("native_8512_roundtrip"), "{output:?}");
    assert!(
        output
            .to_lowercase()
            .contains(&cwd.to_string_lossy().trim_end_matches('\\').to_lowercase()),
        "{output:?}"
    );
    assert!(
        !signal_pty(&child.master, 9),
        "must not act on a reaped process"
    );
    assert!(!resize_pty(child.master.clone(), 80, 24));
}

#[test]
fn conpty_kill_closes_streams_and_process() {
    let mut req = request();
    req.file = "ping.exe".into();
    req.args = vec!["-n".into(), "30".into(), "127.0.0.1".into()];
    let child = spawn_in_pty(&req).expect("spawn ConPTY ping");
    let output = drain(child.master.clone());
    let exited = wait(child.master.clone());
    assert!(signal_pty(&child.master, 0));
    assert!(signal_pty(&child.master, 15));
    assert_eq!(
        exited.recv_timeout(Duration::from_secs(15)).unwrap(),
        (Some(1), None)
    );
    output
        .recv_timeout(Duration::from_secs(5))
        .expect("kill must close output");
    assert!(!signal_pty(&child.master, 0));
}

#[test]
fn conpty_spawn_errors_are_synchronous() {
    let mut req = request();
    req.file = "perry-nonexistent-8512.exe".into();
    assert!(spawn_in_pty(&req).is_err());
    req = request();
    req.cwd = Some("Z:\\perry-nonexistent-8512".into());
    assert!(spawn_in_pty(&req).is_err());
    req = request();
    req.args.push("invalid\0argument".into());
    assert!(spawn_in_pty(&req).is_err());
}

#[test]
fn conpty_dimensions_reach_child_and_kill_reaps_attached_tree() {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
    let mut req = request();
    req.file = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    req.args = [
        "--exact",
        "pty::native::tests::console_fixture",
        "--nocapture",
    ]
    .map(str::to_string)
    .to_vec();
    req.env
        .push(("PERRY_8512_CONSOLE_FIXTURE".into(), "1".into()));
    let child = spawn_in_pty(&req).unwrap();
    let session = child.master.clone();
    let (tx, output) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match read_pty(&session, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx
                        .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    let exited = wait(child.master.clone());
    let mut text = String::new();
    let mut expect = |marker: &str| {
        while !text.contains(marker) {
            match output.recv_timeout(Duration::from_secs(10)) {
                Ok(chunk) => text.push_str(&chunk),
                Err(err) => {
                    signal_pty(&child.master, 9);
                    panic!("{marker}: {err}: {text:?}");
                }
            }
        }
    };
    expect("CONSOLE_READY");
    assert!(resize_pty(child.master.clone(), 120, 40));
    assert!(write_pty(&child.master, b"size\r"));
    expect("CONSOLE_SIZE:120x40");
    assert!(write_pty(&child.master, b"tree\r"));
    expect("CONSOLE_CHILD:");
    // Wait for the terminator if a read split the decimal pid.
    expect(":END");
    let suffix = text.split("CONSOLE_CHILD:").last().unwrap();
    let pid: u32 = suffix
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap();
    let descendant = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!descendant.is_null());
    let descendant = unsafe { OwnedHandle::from_raw_handle(descendant) };
    assert_eq!(
        unsafe { WaitForSingleObject(descendant.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    assert!(signal_pty(&child.master, 15));
    assert_eq!(
        exited.recv_timeout(Duration::from_secs(10)).unwrap(),
        (Some(1), None)
    );
    assert_eq!(
        unsafe { WaitForSingleObject(descendant.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
}
