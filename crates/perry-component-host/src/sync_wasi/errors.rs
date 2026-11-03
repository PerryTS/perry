use super::{bindings::wasi::filesystem::types::ErrorCode, State};
use std::io;
use wasmtime::component::{Resource, ResourceTableError};

pub struct IoError(pub io::Error);

impl super::bindings::wasi::io::error::Host for State {}
impl super::bindings::wasi::io::error::HostError for State {
    fn to_debug_string(&mut self, error: Resource<IoError>) -> wasmtime::Result<String> {
        Ok(self.table.get(&error)?.0.to_string())
    }
    fn drop(&mut self, error: Resource<IoError>) -> wasmtime::Result<()> {
        self.table.delete(error)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum FsError {
    Code(ErrorCode),
    Trap(wasmtime::Error),
}
impl From<ErrorCode> for FsError {
    fn from(value: ErrorCode) -> Self {
        Self::Code(value)
    }
}
impl From<io::Error> for FsError {
    fn from(value: io::Error) -> Self {
        Self::Code(code(&value))
    }
}
impl From<ResourceTableError> for FsError {
    fn from(value: ResourceTableError) -> Self {
        Self::Trap(value.into())
    }
}
impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FsError {}
pub type FsResult<T> = Result<T, FsError>;

pub fn code(error: &io::Error) -> ErrorCode {
    use ErrorCode as E;
    #[cfg(unix)]
    if let Some(raw) = error.raw_os_error() {
        if raw == libc::ENOTSUP || raw == libc::EOPNOTSUPP {
            return E::Unsupported;
        }
        let specific = match raw {
            libc::EPIPE => Some(E::Pipe),
            libc::EPERM => Some(E::NotPermitted),
            libc::ENOENT => Some(E::NoEntry),
            libc::ENOMEM => Some(E::InsufficientMemory),
            libc::EIO => Some(E::Io),
            libc::EBADF => Some(E::BadDescriptor),
            libc::EBUSY => Some(E::Busy),
            libc::EACCES => Some(E::Access),
            libc::ENOTDIR => Some(E::NotDirectory),
            libc::EISDIR => Some(E::IsDirectory),
            libc::EINVAL => Some(E::Invalid),
            libc::EEXIST => Some(E::Exist),
            libc::EFBIG => Some(E::FileTooLarge),
            libc::ENOSPC => Some(E::InsufficientSpace),
            libc::ESPIPE => Some(E::InvalidSeek),
            libc::EMLINK => Some(E::TooManyLinks),
            libc::ENAMETOOLONG => Some(E::NameTooLong),
            libc::ENOTEMPTY => Some(E::NotEmpty),
            libc::ELOOP => Some(E::Loop),
            libc::EOVERFLOW => Some(E::Overflow),
            libc::EILSEQ => Some(E::IllegalByteSequence),
            libc::EALREADY => Some(E::Already),
            libc::EINPROGRESS => Some(E::InProgress),
            libc::EINTR => Some(E::Interrupted),
            _ => None,
        };
        if let Some(code) = specific {
            return code;
        }
    }
    #[cfg(windows)]
    if let Some(raw) = error.raw_os_error() {
        use windows_sys::Win32::Foundation::*;
        let specific = match raw as u32 {
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_INVALID_NAME => Some(E::NoEntry),
            ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION => Some(E::Access),
            ERROR_PRIVILEGE_NOT_HELD => Some(E::NotPermitted),
            ERROR_INVALID_HANDLE => Some(E::BadDescriptor),
            ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY => Some(E::InsufficientMemory),
            ERROR_DIR_NOT_EMPTY => Some(E::NotEmpty),
            ERROR_NOT_READY | ERROR_BUSY => Some(E::Busy),
            ERROR_NOT_SUPPORTED => Some(E::Unsupported),
            ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS => Some(E::Exist),
            ERROR_BROKEN_PIPE => Some(E::Pipe),
            ERROR_BUFFER_OVERFLOW => Some(E::NameTooLong),
            ERROR_NOT_A_REPARSE_POINT | ERROR_NEGATIVE_SEEK => Some(E::Invalid),
            ERROR_DIRECTORY => Some(E::NotDirectory),
            ERROR_STOPPED_ON_SYMLINK => Some(E::Loop),
            ERROR_DIRECTORY_NOT_SUPPORTED => Some(E::IsDirectory),
            _ => None,
        };
        if let Some(code) = specific {
            return code;
        }
    }
    // Preserve the former provider's fallback for synthetic/non-native errors.
    match error.kind() {
        io::ErrorKind::NotFound => E::NoEntry,
        io::ErrorKind::PermissionDenied => E::NotPermitted,
        io::ErrorKind::AlreadyExists => E::Exist,
        io::ErrorKind::InvalidInput => E::Invalid,
        _ => E::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_errors_preserve_the_provider_fallback() {
        assert_eq!(
            code(&io::ErrorKind::PermissionDenied.into()),
            ErrorCode::NotPermitted
        );
        assert_eq!(code(&io::ErrorKind::Interrupted.into()), ErrorCode::Io);
    }
    #[cfg(unix)]
    #[test]
    fn native_errors_keep_access_and_capability_denial_distinct() {
        assert_eq!(
            code(&io::Error::from_raw_os_error(libc::EACCES)),
            ErrorCode::Access
        );
        assert_eq!(
            code(&io::Error::from_raw_os_error(libc::EPERM)),
            ErrorCode::NotPermitted
        );
        assert_eq!(
            code(&io::Error::from_raw_os_error(libc::EBADF)),
            ErrorCode::BadDescriptor
        );
    }
    #[cfg(windows)]
    #[test]
    fn native_windows_errors_keep_descriptor_and_path_meanings() {
        use windows_sys::Win32::Foundation::*;
        for (native, expected) in [
            (ERROR_INVALID_HANDLE, ErrorCode::BadDescriptor),
            (ERROR_INVALID_NAME, ErrorCode::NoEntry),
            (ERROR_PRIVILEGE_NOT_HELD, ErrorCode::NotPermitted),
            (ERROR_STOPPED_ON_SYMLINK, ErrorCode::Loop),
        ] {
            assert_eq!(code(&io::Error::from_raw_os_error(native as i32)), expected);
        }
    }
}
