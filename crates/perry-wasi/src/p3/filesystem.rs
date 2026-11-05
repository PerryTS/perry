// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
mod directory;
mod host;
#[cfg(test)]
mod tests;
mod time;
use super::bindings::filesystem::{preopens, types};
use crate::TrappableError;
pub type FilesystemResult<T> = Result<T, FilesystemError>;
pub type FilesystemError = TrappableError<types::ErrorCode>;
impl<'a> From<&'a std::io::Error> for types::ErrorCode {
    fn from(err: &'a std::io::Error) -> Self {
        crate::filesystem::ErrorCode::from(err).into()
    }
}

impl From<std::io::Error> for types::ErrorCode {
    fn from(err: std::io::Error) -> Self {
        Self::from(&err)
    }
}

impl From<std::io::Error> for FilesystemError {
    fn from(error: std::io::Error) -> Self {
        types::ErrorCode::from(error).into()
    }
}

impl From<crate::filesystem::ErrorCode> for types::ErrorCode {
    fn from(error: crate::filesystem::ErrorCode) -> Self {
        match error {
            crate::filesystem::ErrorCode::Access => Self::Access,
            crate::filesystem::ErrorCode::Already => Self::Already,
            crate::filesystem::ErrorCode::BadDescriptor => Self::BadDescriptor,
            crate::filesystem::ErrorCode::Busy => Self::Busy,
            crate::filesystem::ErrorCode::Exist => Self::Exist,
            crate::filesystem::ErrorCode::FileTooLarge => Self::FileTooLarge,
            crate::filesystem::ErrorCode::IllegalByteSequence => Self::IllegalByteSequence,
            crate::filesystem::ErrorCode::InProgress => Self::InProgress,
            crate::filesystem::ErrorCode::Interrupted => Self::Interrupted,
            crate::filesystem::ErrorCode::Invalid => Self::Invalid,
            crate::filesystem::ErrorCode::Io => Self::Io,
            crate::filesystem::ErrorCode::IsDirectory => Self::IsDirectory,
            crate::filesystem::ErrorCode::Loop => Self::Loop,
            crate::filesystem::ErrorCode::TooManyLinks => Self::TooManyLinks,
            crate::filesystem::ErrorCode::NameTooLong => Self::NameTooLong,
            crate::filesystem::ErrorCode::NoEntry => Self::NoEntry,
            crate::filesystem::ErrorCode::InsufficientMemory => Self::InsufficientMemory,
            crate::filesystem::ErrorCode::InsufficientSpace => Self::InsufficientSpace,
            crate::filesystem::ErrorCode::NotDirectory => Self::NotDirectory,
            crate::filesystem::ErrorCode::NotEmpty => Self::NotEmpty,
            crate::filesystem::ErrorCode::Unsupported => Self::Unsupported,
            crate::filesystem::ErrorCode::Overflow => Self::Overflow,
            crate::filesystem::ErrorCode::NotPermitted => Self::NotPermitted,
            crate::filesystem::ErrorCode::Pipe => Self::Pipe,
            crate::filesystem::ErrorCode::InvalidSeek => Self::InvalidSeek,
        }
    }
}

impl From<crate::filesystem::ErrorCode> for FilesystemError {
    fn from(error: crate::filesystem::ErrorCode) -> Self {
        types::ErrorCode::from(error).into()
    }
}

impl From<wasmtime::component::ResourceTableError> for FilesystemError {
    fn from(error: wasmtime::component::ResourceTableError) -> Self {
        Self::trap(error)
    }
}

impl From<types::Advice> for crate::filesystem::Advice {
    fn from(advice: types::Advice) -> Self {
        match advice {
            types::Advice::Normal => Self::Normal,
            types::Advice::Sequential => Self::Sequential,
            types::Advice::Random => Self::Random,
            types::Advice::WillNeed => Self::WillNeed,
            types::Advice::DontNeed => Self::DontNeed,
            types::Advice::NoReuse => Self::NoReuse,
        }
    }
}

impl From<types::OpenFlags> for crate::filesystem::OpenFlags {
    fn from(flags: types::OpenFlags) -> Self {
        let mut out = Self::empty();
        if flags.contains(types::OpenFlags::CREATE) {
            out |= Self::CREATE;
        }
        if flags.contains(types::OpenFlags::DIRECTORY) {
            out |= Self::DIRECTORY;
        }
        if flags.contains(types::OpenFlags::EXCLUSIVE) {
            out |= Self::EXCLUSIVE;
        }
        if flags.contains(types::OpenFlags::TRUNCATE) {
            out |= Self::TRUNCATE;
        }
        out
    }
}

impl From<types::PathFlags> for crate::filesystem::PathFlags {
    fn from(flags: types::PathFlags) -> Self {
        let mut out = Self::empty();
        if flags.contains(types::PathFlags::SYMLINK_FOLLOW) {
            out |= Self::SYMLINK_FOLLOW;
        }
        out
    }
}

impl From<crate::filesystem::DescriptorFlags> for types::DescriptorFlags {
    fn from(flags: crate::filesystem::DescriptorFlags) -> Self {
        let mut out = Self::empty();
        if flags.contains(crate::filesystem::DescriptorFlags::READ) {
            out |= Self::READ;
        }
        if flags.contains(crate::filesystem::DescriptorFlags::WRITE) {
            out |= Self::WRITE;
        }
        if flags.contains(crate::filesystem::DescriptorFlags::FILE_INTEGRITY_SYNC) {
            out |= Self::FILE_INTEGRITY_SYNC;
        }
        if flags.contains(crate::filesystem::DescriptorFlags::DATA_INTEGRITY_SYNC) {
            out |= Self::DATA_INTEGRITY_SYNC;
        }
        if flags.contains(crate::filesystem::DescriptorFlags::REQUESTED_WRITE_SYNC) {
            out |= Self::REQUESTED_WRITE_SYNC;
        }
        if flags.contains(crate::filesystem::DescriptorFlags::MUTATE_DIRECTORY) {
            out |= Self::MUTATE_DIRECTORY;
        }
        out
    }
}

impl From<types::DescriptorFlags> for crate::filesystem::DescriptorFlags {
    fn from(flags: types::DescriptorFlags) -> Self {
        let mut out = Self::empty();
        if flags.contains(types::DescriptorFlags::READ) {
            out |= Self::READ;
        }
        if flags.contains(types::DescriptorFlags::WRITE) {
            out |= Self::WRITE;
        }
        if flags.contains(types::DescriptorFlags::FILE_INTEGRITY_SYNC) {
            out |= Self::FILE_INTEGRITY_SYNC;
        }
        if flags.contains(types::DescriptorFlags::DATA_INTEGRITY_SYNC) {
            out |= Self::DATA_INTEGRITY_SYNC;
        }
        if flags.contains(types::DescriptorFlags::REQUESTED_WRITE_SYNC) {
            out |= Self::REQUESTED_WRITE_SYNC;
        }
        if flags.contains(types::DescriptorFlags::MUTATE_DIRECTORY) {
            out |= Self::MUTATE_DIRECTORY;
        }
        out
    }
}

impl From<crate::filesystem::MetadataHashValue> for types::MetadataHashValue {
    fn from(
        crate::filesystem::MetadataHashValue { lower, upper }: crate::filesystem::MetadataHashValue,
    ) -> Self {
        Self { lower, upper }
    }
}

impl From<crate::filesystem::DescriptorStat> for types::DescriptorStat {
    fn from(
        crate::filesystem::DescriptorStat {
            type_,
            link_count,
            size,
            data_access_timestamp,
            data_modification_timestamp,
            status_change_timestamp,
        }: crate::filesystem::DescriptorStat,
    ) -> Self {
        Self {
            type_: type_.into(),
            link_count,
            size,
            data_access_timestamp: data_access_timestamp.map(Into::into),
            data_modification_timestamp: data_modification_timestamp.map(Into::into),
            status_change_timestamp: status_change_timestamp.map(Into::into),
        }
    }
}

impl From<crate::filesystem::DescriptorType> for types::DescriptorType {
    fn from(ty: crate::filesystem::DescriptorType) -> Self {
        match ty {
            crate::filesystem::DescriptorType::Unknown => Self::Other(None),
            #[cfg(unix)]
            crate::filesystem::DescriptorType::Fifo => Self::Fifo,
            #[cfg(unix)]
            crate::filesystem::DescriptorType::Socket => Self::Socket,
            crate::filesystem::DescriptorType::BlockDevice => Self::BlockDevice,
            crate::filesystem::DescriptorType::CharacterDevice => Self::CharacterDevice,
            crate::filesystem::DescriptorType::Directory => Self::Directory,
            crate::filesystem::DescriptorType::SymbolicLink => Self::SymbolicLink,
            crate::filesystem::DescriptorType::RegularFile => Self::RegularFile,
        }
    }
}

impl From<crate::filesystem::primitives::FileType> for types::DescriptorType {
    fn from(ft: crate::filesystem::primitives::FileType) -> Self {
        crate::filesystem::DescriptorType::from(ft).into()
    }
}

impl From<crate::p2::bindings::filesystem::types::ErrorCode> for types::ErrorCode {
    fn from(value: crate::p2::bindings::filesystem::types::ErrorCode) -> Self {
        use crate::p2::bindings::filesystem::types::ErrorCode as P2;
        match value {
            P2::Access => Self::Access,
            P2::WouldBlock => Self::Other(Some("would-block".into())),
            P2::Already => Self::Already,
            P2::BadDescriptor => Self::BadDescriptor,
            P2::Busy => Self::Busy,
            P2::Deadlock => Self::Deadlock,
            P2::Quota => Self::Quota,
            P2::Exist => Self::Exist,
            P2::FileTooLarge => Self::FileTooLarge,
            P2::IllegalByteSequence => Self::IllegalByteSequence,
            P2::InProgress => Self::InProgress,
            P2::Interrupted => Self::Interrupted,
            P2::Invalid => Self::Invalid,
            P2::Io => Self::Io,
            P2::IsDirectory => Self::IsDirectory,
            P2::Loop => Self::Loop,
            P2::TooManyLinks => Self::TooManyLinks,
            P2::MessageSize => Self::MessageSize,
            P2::NameTooLong => Self::NameTooLong,
            P2::NoDevice => Self::NoDevice,
            P2::NoEntry => Self::NoEntry,
            P2::NoLock => Self::NoLock,
            P2::InsufficientMemory => Self::InsufficientMemory,
            P2::InsufficientSpace => Self::InsufficientSpace,
            P2::NotDirectory => Self::NotDirectory,
            P2::NotEmpty => Self::NotEmpty,
            P2::NotRecoverable => Self::NotRecoverable,
            P2::Unsupported => Self::Unsupported,
            P2::NoTty => Self::NoTty,
            P2::NoSuchDevice => Self::NoSuchDevice,
            P2::Overflow => Self::Overflow,
            P2::NotPermitted => Self::NotPermitted,
            P2::Pipe => Self::Pipe,
            P2::ReadOnly => Self::ReadOnly,
            P2::InvalidSeek => Self::InvalidSeek,
            P2::TextFileBusy => Self::TextFileBusy,
            P2::CrossDevice => Self::CrossDevice,
        }
    }
}

impl From<crate::p2::FsError> for FilesystemError {
    fn from(error: crate::p2::FsError) -> Self {
        match error.downcast() {
            Ok(error) => types::ErrorCode::from(error).into(),
            Err(error) => Self::trap(error),
        }
    }
}
impl super::streams::StreamFailure for types::ErrorCode {
    fn from_io(error: &std::io::Error) -> Self {
        error.into()
    }
    fn closed() -> Self {
        Self::Pipe
    }
}
