//! The host-owned descriptor table: one table for every descriptor the guest
//! can name, with the lowest free number reused after a close.

use crate::perry_net::Pending;
use std::os::fd::{AsRawFd, OwnedFd};

/// Preview 1 `rights` bits the reference host grants or checks.
pub mod rights {
    pub const FD_READ: u64 = 1 << 1;
    pub const FD_FDSTAT_SET_FLAGS: u64 = 1 << 3;
    pub const FD_WRITE: u64 = 1 << 6;
    pub const POLL_FD_READWRITE: u64 = 1 << 27;
    pub const SOCK_SHUTDOWN: u64 = 1 << 28;
    pub const SOCK_ACCEPT: u64 = 1 << 29;
}

/// `filetype` values.
pub const FILETYPE_CHARACTER_DEVICE: u8 = 2;
pub const FILETYPE_SOCKET_STREAM: u8 = 6;

/// What a descriptor number stands for. Only the stdio entries borrow an OS
/// descriptor; the others own theirs, so removing an entry closes it.
pub enum Fd {
    Stdio(i32),
    Listener(OwnedFd),
    Stream(OwnedFd),
    Signal(OwnedFd),
    /// The read end of a `perry_net.resolve` answer.
    Resolve(OwnedFd),
    /// A `perry_net.tcp_connect` socket that is still resolving or connecting.
    /// It becomes a `Stream` once connected, or a `Failed` with the errno
    /// `perry_net.sock_error` reports.
    Pending(Pending),
    Failed(u16),
}

impl Fd {
    pub fn os(&self) -> i32 {
        match self {
            Fd::Stdio(fd) => *fd,
            Fd::Listener(fd) | Fd::Stream(fd) | Fd::Signal(fd) | Fd::Resolve(fd) => fd.as_raw_fd(),
            Fd::Pending(pending) => pending.os(),
            // Nothing is left to poll or read.
            Fd::Failed(_) => -1,
        }
    }

    pub fn filetype(&self) -> u8 {
        match self {
            Fd::Listener(_) | Fd::Stream(_) | Fd::Pending(_) | Fd::Failed(_) => {
                FILETYPE_SOCKET_STREAM
            }
            Fd::Stdio(_) | Fd::Signal(_) | Fd::Resolve(_) => FILETYPE_CHARACTER_DEVICE,
        }
    }

    /// The minimal rights the embedder contract documents for each kind.
    /// Operations outside them fail with `ENOTCAPABLE`, so a runtime that
    /// relies on a right the contract does not grant is caught here.
    pub fn rights(&self) -> u64 {
        use rights::*;
        match self {
            Fd::Stdio(0) => FD_READ | POLL_FD_READWRITE,
            Fd::Stdio(_) => FD_WRITE | POLL_FD_READWRITE,
            Fd::Listener(_) => FD_FDSTAT_SET_FLAGS | SOCK_ACCEPT | POLL_FD_READWRITE,
            Fd::Stream(_) | Fd::Pending(_) | Fd::Failed(_) => {
                FD_READ | FD_WRITE | SOCK_SHUTDOWN | POLL_FD_READWRITE | FD_FDSTAT_SET_FLAGS
            }
            Fd::Signal(_) | Fd::Resolve(_) => FD_READ | POLL_FD_READWRITE | FD_FDSTAT_SET_FLAGS,
        }
    }
}

#[derive(Default)]
pub struct Table {
    slots: Vec<Option<Fd>>,
}

impl Table {
    /// A table holding stdin, stdout and stderr as descriptors 0 to 2.
    pub fn with_stdio() -> Self {
        Table {
            slots: (0..3).map(|fd| Some(Fd::Stdio(fd))).collect(),
        }
    }

    /// Store `fd` under the lowest free number and return that number.
    pub fn insert(&mut self, fd: Fd) -> u32 {
        let index = match self.slots.iter().position(Option::is_none) {
            Some(index) => index,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.slots[index] = Some(fd);
        index as u32
    }

    /// Store `fd` under exactly `number`, leaving any numbers below it as they
    /// are; false when that number is taken.
    pub fn insert_at(&mut self, number: u32, fd: Fd) -> bool {
        let index = number as usize;
        if self.slots.len() <= index {
            self.slots.resize_with(index + 1, || None);
        }
        if self.slots[index].is_some() {
            return false;
        }
        self.slots[index] = Some(fd);
        true
    }

    pub fn get(&self, fd: u32) -> Option<&Fd> {
        self.slots.get(fd as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, fd: u32) -> Option<&mut Fd> {
        self.slots.get_mut(fd as usize)?.as_mut()
    }

    pub fn remove(&mut self, fd: u32) -> Option<Fd> {
        self.slots.get_mut(fd as usize)?.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn socket_pair() -> (OwnedFd, OwnedFd) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client.into(), server.into())
    }

    #[test]
    fn numbers_start_after_stdio_and_reuse_the_lowest_free_one() {
        let mut table = Table::with_stdio();
        let (a, b) = socket_pair();
        let (c, d) = socket_pair();
        assert_eq!(table.insert(Fd::Stream(a)), 3);
        assert_eq!(table.insert(Fd::Stream(b)), 4);
        assert_eq!(table.insert(Fd::Stream(c)), 5);
        assert!(table.remove(4).is_some());
        assert!(table.remove(3).is_some());
        // Freed numbers come back lowest first, whatever kind takes them.
        assert_eq!(table.insert(Fd::Stream(d)), 3);
        assert!(table.get(4).is_none());
        assert!(table.get(3).is_some());
    }

    #[test]
    fn a_descriptor_can_be_placed_behind_a_gap_and_the_gap_is_filled_first() {
        let mut table = Table::with_stdio();
        let (a, b) = socket_pair();
        let (c, _d) = socket_pair();
        assert!(table.insert_at(12, Fd::Stream(a)));
        assert!(table.get(5).is_none() && table.get(12).is_some());
        assert!(!table.insert_at(12, Fd::Stream(b)));
        assert_eq!(table.insert(Fd::Stream(c)), 3);
    }

    #[test]
    fn a_closed_descriptor_is_gone_and_reports_nothing() {
        let mut table = Table::with_stdio();
        let (a, _b) = socket_pair();
        let fd = table.insert(Fd::Stream(a));
        assert!(table.remove(fd).is_some());
        assert!(table.get(fd).is_none());
        assert!(table.remove(fd).is_none());
        assert!(table.get(99).is_none());
    }

    #[test]
    fn removing_an_owned_descriptor_closes_it() {
        let mut table = Table::with_stdio();
        let (a, b) = socket_pair();
        let fd = table.insert(Fd::Stream(a));
        drop(table.remove(fd));
        let mut stream = TcpStream::from(b);
        use std::io::Read;
        let mut byte = [0u8; 1];
        // The peer sees the close as end of stream.
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    }
}
