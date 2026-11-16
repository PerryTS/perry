use std::io;
use std::net::Shutdown;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::thread::{self, JoinHandle};

pub(super) struct Reader {
    cancel: UnixStream,
    thread: Option<JoinHandle<()>>,
}

impl Reader {
    pub(super) fn start(input: RawFd) -> io::Result<Self> {
        let input = unsafe { libc::fcntl(input, libc::F_DUPFD_CLOEXEC, 0) };
        if input < 0 {
            return Err(io::Error::last_os_error());
        }
        let input = unsafe { OwnedFd::from_raw_fd(input) };
        let (cancel, wake) = UnixStream::pair()?;
        let thread = thread::Builder::new()
            .name("perry-tui-input".to_string())
            .spawn(move || read_input(input, wake))?;
        Ok(Self {
            cancel,
            thread: Some(thread),
        })
    }

    pub(super) fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.cancel.shutdown(Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_input(input: OwnedFd, wake: UnixStream) {
    let mut descriptors = [
        libc::pollfd {
            fd: wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: input.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        let ready = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, -1) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if descriptors[0].revents != 0 {
            break;
        }
        if descriptors[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let mut byte = 0u8;
            let count = unsafe { libc::read(input.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
            if count == 1 {
                super::PENDING_BYTES.lock().unwrap().push(byte);
            } else if count == 0 {
                break;
            } else if !matches!(
                io::Error::last_os_error().kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                break;
            }
        } else if descriptors[1].revents != 0 {
            break;
        }
    }
}
