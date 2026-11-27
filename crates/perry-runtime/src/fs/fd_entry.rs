//! One entry of the descriptor registry and the I/O it supports.
use std::fs;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

/// One registered descriptor. The entry itself says who owns the descriptor:
/// a file the runtime opened (or a `dup` it made) closes on drop; a
/// descriptor the process inherited is only adopted and never closes on
/// drop, so thread teardown cannot close an fd the program still uses. An
/// embedded asset has no OS descriptor: it is the registry's bytes and an
/// offset.
pub(crate) enum FdEntry {
    Owned(fs::File),
    Adopted(std::mem::ManuallyDrop<fs::File>),
    Embedded(Cursor<&'static [u8]>),
}

impl FdEntry {
    /// The entry as an owning `File`: dropping the result closes the
    /// descriptor. An explicit program close, or a socket taking the fd.
    pub(crate) fn into_file(self) -> Option<fs::File> {
        match self {
            FdEntry::Owned(file) => Some(file),
            FdEntry::Adopted(file) => Some(std::mem::ManuallyDrop::into_inner(file)),
            FdEntry::Embedded(_) => None,
        }
    }

    /// The OS file behind the entry; `None` for an embedded asset.
    pub(crate) fn os_file(&self) -> Option<&fs::File> {
        match self {
            FdEntry::Owned(file) => Some(file),
            FdEntry::Adopted(file) => Some(file),
            FdEntry::Embedded(_) => None,
        }
    }

    pub(crate) fn is_embedded(&self) -> bool {
        matches!(self, FdEntry::Embedded(_))
    }
}

impl From<fs::File> for FdEntry {
    fn from(file: fs::File) -> Self {
        FdEntry::Owned(file)
    }
}

impl Read for FdEntry {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            FdEntry::Owned(file) => file.read(buf),
            FdEntry::Adopted(file) => file.read(buf),
            FdEntry::Embedded(bytes) => bytes.read(buf),
        }
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        match self {
            FdEntry::Owned(file) => file.read_to_end(buf),
            FdEntry::Adopted(file) => file.read_to_end(buf),
            FdEntry::Embedded(bytes) => bytes.read_to_end(buf),
        }
    }
}

impl Write for FdEntry {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            FdEntry::Owned(file) => file.write(buf),
            FdEntry::Adopted(file) => file.write(buf),
            FdEntry::Embedded(_) => Err(super::errors::ebadf_os_error()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            FdEntry::Owned(file) => file.flush(),
            FdEntry::Adopted(file) => file.flush(),
            FdEntry::Embedded(_) => Ok(()),
        }
    }
}

impl Seek for FdEntry {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            FdEntry::Owned(file) => file.seek(pos),
            FdEntry::Adopted(file) => file.seek(pos),
            FdEntry::Embedded(bytes) => bytes.seek(pos),
        }
    }
}
