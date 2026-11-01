use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn attributes(fd: i32) -> libc::termios {
    let mut attrs = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut attrs) }, 0);
    attrs
}

fn assert_restored(actual: &libc::termios, expected: &libc::termios) {
    assert_eq!(actual.c_iflag, expected.c_iflag);
    assert_eq!(actual.c_oflag, expected.c_oflag);
    assert_eq!(actual.c_cflag, expected.c_cflag);
    // macOS may set PENDIN when canonical input is restored; it is a
    // kernel-maintained pending-input marker, not an unrestored mode bit.
    assert_eq!(
        actual.c_lflag & !libc::PENDIN,
        expected.c_lflag & !libc::PENDIN
    );
    assert_eq!(actual.c_cc, expected.c_cc);
    unsafe {
        assert_eq!(libc::cfgetispeed(actual), libc::cfgetispeed(expected));
        assert_eq!(libc::cfgetospeed(actual), libc::cfgetospeed(expected));
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn raw_mode_preserves_pty_output() {
    // Each child owns stdin and a fresh SAVED, so this cannot change the
    // test runner's terminal or interfere with other readline tests.
    for flags in [libc::OPOST | libc::ONLCR, 0, libc::OPOST, libc::ONLCR] {
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let (mut master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        let mut initial = attributes(slave.as_raw_fd());
        initial.c_oflag = flags;
        initial.c_lflag |= libc::ECHO | libc::ICANON | libc::ISIG;
        initial.c_iflag |= libc::ICRNL | libc::IXON;
        assert_eq!(
            unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &initial) },
            0
        );
        let initial = attributes(slave.as_raw_fd());
        assert_eq!(initial.c_oflag, flags, "PTY must exercise requested flags");
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "readline::raw_mode::termios_impl::tests::raw_mode_pty_child",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .stdin(Stdio::from(slave.try_clone().unwrap()))
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let expected: &[u8] = if flags == libc::OPOST | libc::ONLCR {
            b"first\r\nsecond\r\nthird\r\n"
        } else {
            b"first\nsecond\nthird\n"
        };
        for _ in 0..2 {
            let mut received = Vec::new();
            while received.iter().filter(|&&byte| byte == b'\n').count() < 3 {
                let mut pollfd = libc::pollfd {
                    fd: master.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                assert_eq!(
                    unsafe { libc::poll(&mut pollfd, 1, 5000) },
                    1,
                    "PTY output timed out"
                );
                let mut buffer = [0; 64];
                let count = master.read(&mut buffer).unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
            }
            assert_eq!(received, expected, "output flags {flags:#x}");
            assert_eq!(attributes(slave.as_raw_fd()).c_oflag, flags);
            // No newline: canonical input would block; ISIG would consume
            // Ctrl-C; ICRNL would translate the CR. None may echo.
            master.write_all(b"\x03\r").unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "PTY child failed: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "raw input timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_restored(&attributes(slave.as_raw_fd()), &initial);
    }
}

#[test]
#[ignore = "runs in a PTY subprocess of raw_mode_preserves_pty_output"]
fn raw_mode_pty_child() {
    let initial = attributes(0);
    let fd = unsafe { libc::dup(0) };
    assert!(fd >= 0);
    let mut terminal = unsafe { File::from_raw_fd(fd) };
    for _ in 0..2 {
        assert!(super::enable());
        assert!(
            super::enable(),
            "repeated enable must preserve original settings"
        );
        let raw = attributes(0);
        assert_eq!(raw.c_lflag & (libc::ECHO | libc::ICANON | libc::ISIG), 0);
        assert_eq!(raw.c_iflag & (libc::ICRNL | libc::IXON), 0);
        assert_eq!(raw.c_cc[libc::VMIN], 1);
        assert_eq!(raw.c_cc[libc::VTIME], 0);
        terminal.write_all(b"first\nsecond\nthird\n").unwrap();
        let mut input = [0; 2];
        terminal.read_exact(&mut input).unwrap();
        assert_eq!(input, *b"\x03\r");
        assert!(super::disable());
        assert_restored(&attributes(0), &initial);
    }
}
