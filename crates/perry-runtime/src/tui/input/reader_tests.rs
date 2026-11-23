use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn attributes(fd: i32) -> libc::termios {
    let mut result = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut result) }, 0);
    result
}

fn assert_restored(actual: &libc::termios, expected: &libc::termios) {
    assert_eq!(actual.c_iflag, expected.c_iflag);
    assert_eq!(actual.c_oflag, expected.c_oflag);
    assert_eq!(actual.c_cflag, expected.c_cflag);
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

fn announce(message: &str) {
    println!("{message}");
    std::io::stdout().flush().unwrap();
}

fn wait_for_bytes(expected: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if super::PENDING_BYTES.lock().unwrap().as_slice() == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "reader did not deliver {expected:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn idle_reader_joins_without_consuming_later_input() {
    let _serial = super::tests::reset();
    let (mut input, mut producer) = UnixStream::pair().unwrap();
    let flags = unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    let reader = super::reader_unix::Reader::start(input.as_raw_fd()).unwrap();
    assert!(!reader.is_finished());
    drop(reader);
    assert_eq!(
        unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GETFL) },
        flags
    );
    producer.write_all(b"next").unwrap();
    input
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut following = [0; 4];
    input.read_exact(&mut following).unwrap();
    assert_eq!(&following, b"next");
    assert!(super::PENDING_BYTES.lock().unwrap().is_empty());
}

#[test]
fn reader_eof_finishes_and_new_reader_delivers_bytes() {
    let _serial = super::tests::reset();
    let (input, producer) = UnixStream::pair().unwrap();
    let reader = super::reader_unix::Reader::start(input.as_raw_fd()).unwrap();
    assert!(!reader.is_finished());
    drop(producer);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !reader.is_finished() {
        assert!(Instant::now() < deadline, "EOF reader did not terminate");
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(reader);
    let (input, mut producer) = UnixStream::pair().unwrap();
    let reader = super::reader_unix::Reader::start(input.as_raw_fd()).unwrap();
    producer.write_all(b"restarted").unwrap();
    wait_for_bytes(b"restarted");
    drop(reader);
    assert_eq!(
        super::PENDING_BYTES.lock().unwrap().as_slice(),
        b"restarted"
    );
}

#[test]
fn invalid_input_does_not_start_a_reader() {
    assert!(super::reader_unix::Reader::start(-1).is_err());
}

#[test]
fn enable_restarts_after_eof() {
    crate::test_isolation::in_own_process(|| {
        let _serial = super::tests::reset();
        let (input, producer) = UnixStream::pair().unwrap();
        assert_eq!(unsafe { libc::dup2(input.as_raw_fd(), 0) }, 0);
        super::enable_raw_mode();
        drop(producer);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !super::READER
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_finished()
        {
            assert!(Instant::now() < deadline, "EOF reader did not terminate");
            std::thread::sleep(Duration::from_millis(5));
        }
        let (input, mut producer) = UnixStream::pair().unwrap();
        assert_eq!(unsafe { libc::dup2(input.as_raw_fd(), 0) }, 0);
        super::enable_raw_mode();
        producer.write_all(b"new session").unwrap();
        wait_for_bytes(b"new session");
        super::disable_raw_mode();
        assert!(super::READER.lock().unwrap().is_none());
        assert!(super::PENDING_BYTES.lock().unwrap().is_empty());
    });
}

#[test]
fn reader_stops_without_input_releases_stdin_and_restarts() {
    let _serial = super::tests::reset();
    let mut master_fd = -1;
    let mut slave_fd = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let (mut master, slave) =
        unsafe { (File::from_raw_fd(master_fd), File::from_raw_fd(slave_fd)) };
    for descriptor in [master.as_raw_fd(), slave.as_raw_fd()] {
        assert_eq!(
            unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    let initial = attributes(slave.as_raw_fd());
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .env(
                "__RUST_TEST_INVOKE",
                "tui::input::reader_tests::reader_lifecycle_child",
            )
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut transcript = String::new();
    let mut wait_for = |message: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = receiver.recv_timeout(remaining).unwrap_or_else(|error| {
                let _ = child.0.kill();
                let _ = child.0.wait();
                panic!("waiting for {message}: {error}; child output: {transcript}")
            });
            transcript.push_str(&line);
            transcript.push('\n');
            if line.ends_with(message) {
                break;
            }
        }
    };
    wait_for("READER_READY");
    master.write_all(b"a").unwrap();
    wait_for("READER_STOPPED");
    assert_restored(&attributes(slave.as_raw_fd()), &initial);
    master.write_all(b"after\n").unwrap();
    wait_for("READER_RESTARTED");
    master.write_all(b"b").unwrap();
    wait_for("READER_DONE");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(50), "{transcript}");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.0.kill();
            let _ = child.0.wait();
            panic!("child did not exit: {transcript}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_restored(&attributes(slave.as_raw_fd()), &initial);
}

#[test]
#[ignore = "runs in the reader lifecycle PTY subprocess"]
fn reader_lifecycle_child() {
    let initial = attributes(0);
    super::enable_raw_mode();
    super::enable_raw_mode();
    assert_eq!(attributes(0).c_lflag & (libc::ECHO | libc::ICANON), 0);
    announce("READER_READY");
    wait_for_bytes(b"a");
    super::PENDING_BYTES.lock().unwrap().clear();
    super::disable_raw_mode();
    super::disable_raw_mode();
    assert_restored(&attributes(0), &initial);
    assert!(super::PENDING_BYTES.lock().unwrap().is_empty());
    announce("READER_STOPPED");
    let mut following = [0u8; 6];
    std::io::stdin().read_exact(&mut following).unwrap();
    assert_eq!(&following, b"after\n");
    let mut changed = initial;
    changed.c_oflag ^= libc::OPOST;
    assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &changed) }, 0);
    super::enable_raw_mode();
    announce("READER_RESTARTED");
    wait_for_bytes(b"b");
    super::disable_raw_mode();
    assert!(super::PENDING_BYTES.lock().unwrap().is_empty());
    assert_restored(&attributes(0), &changed);
    assert_eq!(unsafe { libc::tcsetattr(0, libc::TCSANOW, &initial) }, 0);
    announce("READER_DONE");
    std::process::exit(50);
}
