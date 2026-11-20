use super::{set_fd_raw_mode, RAW_MODE_SAVED};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};

fn attributes(fd: i32) -> libc::termios {
    let mut attrs = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut attrs) }, 0);
    attrs
}

#[test]
fn raw_mode_preserves_pty_output() {
    crate::test_isolation::in_own_process(check_pty_output);
}

fn check_pty_output() {
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
        let (mut master, mut slave) =
            unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        let fd = slave.as_raw_fd();
        let mut initial = attributes(fd);
        initial.c_oflag = flags;
        initial.c_lflag |= libc::ECHO | libc::ICANON | libc::ISIG;
        assert_eq!(unsafe { libc::tcsetattr(fd, libc::TCSANOW, &initial) }, 0);
        let initial = attributes(fd);
        assert_eq!(initial.c_oflag, flags);
        // The isolated process owns this global; each PTY needs a fresh save.
        *RAW_MODE_SAVED.lock().unwrap() = None;
        for _ in 0..2 {
            assert!(set_fd_raw_mode(fd, true));
            assert!(set_fd_raw_mode(fd, true));
            let raw = attributes(fd);
            assert_eq!(raw.c_oflag, flags);
            assert_eq!(raw.c_lflag & (libc::ECHO | libc::ICANON | libc::ISIG), 0);
            slave.write_all(b"first\nsecond\n").unwrap();
            let expected: &[u8] = if flags == libc::OPOST | libc::ONLCR {
                b"first\r\nsecond\r\n"
            } else {
                b"first\nsecond\n"
            };
            let mut received = vec![0; expected.len()];
            let mut offset = 0;
            while offset < received.len() {
                let mut pollfd = libc::pollfd {
                    fd: master.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                assert_eq!(unsafe { libc::poll(&mut pollfd, 1, 5000) }, 1);
                let count = master.read(&mut received[offset..]).unwrap();
                assert!(count > 0);
                offset += count;
            }
            assert_eq!(received, expected);
            assert!(set_fd_raw_mode(fd, false));
            let restored = attributes(fd);
            assert_eq!(restored.c_iflag, initial.c_iflag);
            assert_eq!(restored.c_oflag, initial.c_oflag);
            assert_eq!(restored.c_cflag, initial.c_cflag);
            // macOS sets PENDIN while restoring canonical input.
            assert_eq!(
                restored.c_lflag & !libc::PENDIN,
                initial.c_lflag & !libc::PENDIN
            );
            assert_eq!(restored.c_cc, initial.c_cc);
        }
    }
}
