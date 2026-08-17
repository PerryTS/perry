//! Windows process creation for `child_process.fork()`.
//!
//! Rust's `std::process::Command` only populates the three Win32 standard
//! handles. Node/libuv additionally passes fd 3 (or the selected IPC slot) in
//! the Microsoft CRT descriptor table at `STARTUPINFO.lpReserved2`. This module
//! mirrors that ABI so `NODE_CHANNEL_FD` resolves in both Node and Perry child
//! processes.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, GetLastError, SetHandleInformation, DUPLICATE_SAME_ACCESS,
    ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileType, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_TYPE_CHAR, FILE_TYPE_PIPE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, CreatePipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
    InitializeProcThreadAttributeList, UpdateProcThreadAttribute, CREATE_NEW_PROCESS_GROUP,
    CREATE_UNICODE_ENVIRONMENT, DETACHED_PROCESS, EXTENDED_STARTUPINFO_PRESENT,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

use super::ipc_transport::IpcStream;
use super::CpStdio;

const FOPEN: u8 = 0x01;
const FPIPE: u8 = 0x08;
const FDEV: u8 = 0x40;

static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

pub(super) struct WindowsForkChild {
    pub(super) process: OwnedHandle,
    pub(super) pid: u32,
    pub(super) stdin: Option<File>,
    pub(super) stdout: Option<File>,
    pub(super) stderr: Option<File>,
}

pub(super) fn spawn(
    command: &Command,
    stdio: &[CpStdio],
    ipc_fd: usize,
    clear_environment: bool,
    detached: bool,
) -> io::Result<(WindowsForkChild, IpcStream)> {
    let count = stdio.len().max(ipc_fd + 1).max(3);
    if count > u8::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows child stdio supports at most 255 descriptors",
        ));
    }

    let mut child_handles: Vec<Option<OwnedHandle>> =
        std::iter::repeat_with(|| None).take(count).collect();
    let mut parent_stdin = None;
    let mut parent_stdout = None;
    let mut parent_stderr = None;

    for (fd, slot) in child_handles.iter_mut().enumerate().take(3) {
        if fd == ipc_fd {
            continue;
        }
        let kind = stdio.get(fd).copied().unwrap_or(CpStdio::Pipe);
        let (child, parent) = child_stdio(fd, kind)?;
        *slot = Some(child);
        match fd {
            0 => parent_stdin = parent,
            1 => parent_stdout = parent,
            2 => parent_stderr = parent,
            _ => unreachable!(),
        }
    }

    let (parent_ipc, child_ipc) = create_ipc_pair()?;
    child_handles[ipc_fd] = Some(child_ipc);

    // Descriptors above stderr are otherwise ignored today. Keep their CRT
    // slots invalid, matching libuv's UV_IGNORE representation.
    let mut crt = crt_descriptor_table(&child_handles);
    let inherited: Vec<HANDLE> = child_handles
        .iter()
        .filter_map(|handle| handle.as_ref().map(|h| h.as_raw_handle() as HANDLE))
        .collect();
    let mut attributes = AttributeList::with_handles(&inherited)?;

    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.cbReserved2 = crt.len().try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "CRT descriptor table is too large",
        )
    })?;
    startup.StartupInfo.lpReserved2 = crt.as_mut_ptr();
    startup.StartupInfo.hStdInput = raw_handle(&child_handles[0]);
    startup.StartupInfo.hStdOutput = raw_handle(&child_handles[1]);
    startup.StartupInfo.hStdError = raw_handle(&child_handles[2]);
    startup.lpAttributeList = attributes.as_mut_ptr();

    let mut command_line = command_line(command)?;
    let environment = environment_block(command, clear_environment)?;
    let cwd = command
        .get_current_dir()
        .map(|path| wide_nul(path.as_os_str()))
        .transpose()?;
    let cwd_ptr = cwd
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr());
    let mut creation_flags = CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT;
    if detached {
        creation_flags |= DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    }
    let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            creation_flags,
            environment.as_ptr().cast(),
            cwd_ptr,
            &startup.StartupInfo,
            &mut info,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }

    unsafe {
        CloseHandle(info.hThread);
    }
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess as RawHandle) };
    Ok((
        WindowsForkChild {
            process,
            pid: info.dwProcessId,
            stdin: parent_stdin,
            stdout: parent_stdout,
            stderr: parent_stderr,
        },
        parent_ipc,
    ))
}

fn child_stdio(fd: usize, kind: CpStdio) -> io::Result<(OwnedHandle, Option<File>)> {
    match kind {
        CpStdio::Pipe => create_stdio_pipe(fd == 0),
        CpStdio::Inherit => duplicate_std_handle(fd)
            .or_else(|_| open_nul(fd == 0))
            .map(|handle| (handle, None)),
        CpStdio::Fd(source) => duplicate_fd(source)
            .or_else(|_| open_nul(fd == 0))
            .map(|handle| (handle, None)),
        CpStdio::Ignore => open_nul(fd == 0).map(|handle| (handle, None)),
    }
}

fn inheritable_attributes() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    }
}

fn create_stdio_pipe(child_reads: bool) -> io::Result<(OwnedHandle, Option<File>)> {
    let attributes = inheritable_attributes();
    let mut read = INVALID_HANDLE_VALUE;
    let mut write = INVALID_HANDLE_VALUE;
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let (child, parent) = if child_reads {
        (read, write)
    } else {
        (write, read)
    };
    if unsafe { SetHandleInformation(parent, HANDLE_FLAG_INHERIT, 0) } == 0 {
        unsafe {
            CloseHandle(read);
            CloseHandle(write);
        }
        return Err(io::Error::last_os_error());
    }
    let child = unsafe { OwnedHandle::from_raw_handle(child as RawHandle) };
    let parent = unsafe { File::from_raw_handle(parent as RawHandle) };
    Ok((child, Some(parent)))
}

fn duplicate_std_handle(fd: usize) -> io::Result<OwnedHandle> {
    let which = match fd {
        0 => STD_INPUT_HANDLE,
        1 => STD_OUTPUT_HANDLE,
        _ => STD_ERROR_HANDLE,
    };
    let handle = unsafe { GetStdHandle(which) };
    duplicate_handle(handle)
}

fn duplicate_fd(fd: i32) -> io::Result<OwnedHandle> {
    let handle = unsafe { libc::get_osfhandle(fd) } as HANDLE;
    duplicate_handle(handle)
}

fn duplicate_handle(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE || handle as isize == -2 {
        return Err(io::Error::from_raw_os_error(6));
    }
    let mut duplicate = INVALID_HANDLE_VALUE;
    let process = unsafe { GetCurrentProcess() };
    let ok = unsafe {
        DuplicateHandle(
            process,
            handle,
            process,
            &mut duplicate,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(duplicate as RawHandle) })
    }
}

fn open_nul(readable: bool) -> io::Result<OwnedHandle> {
    let name = wide_nul(OsStr::new("NUL"))?;
    let attributes = inheritable_attributes();
    let access = if readable {
        GENERIC_READ
    } else {
        GENERIC_WRITE
    };
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &attributes,
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
    }
}

fn create_ipc_pair() -> io::Result<(IpcStream, OwnedHandle)> {
    let id = NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed);
    let name = format!(r"\\.\pipe\perry-ipc-{}-{id}", std::process::id());
    let wide_name = wide_nul(OsStr::new(&name))?;
    let server = unsafe {
        CreateNamedPipeW(
            wide_name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            65_536,
            65_536,
            0,
            std::ptr::null(),
        )
    };
    if server == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    let attributes = inheritable_attributes();
    let client = unsafe {
        CreateFileW(
            wide_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            &attributes,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if client == INVALID_HANDLE_VALUE {
        unsafe {
            CloseHandle(server);
        }
        return Err(io::Error::last_os_error());
    }
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        unsafe {
            CloseHandle(server);
            CloseHandle(client);
        }
        return Err(io::Error::last_os_error());
    }
    let mut overlapped = OVERLAPPED {
        hEvent: event,
        ..Default::default()
    };
    let connected = unsafe { ConnectNamedPipe(server, &mut overlapped) };
    let connect_error = if connected != 0 {
        None
    } else {
        match unsafe { GetLastError() } {
            ERROR_PIPE_CONNECTED => None,
            ERROR_IO_PENDING => {
                let mut transferred = 0u32;
                if unsafe { GetOverlappedResult(server, &overlapped, &mut transferred, 1) } != 0 {
                    None
                } else {
                    Some(io::Error::last_os_error())
                }
            }
            _ => Some(io::Error::last_os_error()),
        }
    };
    unsafe {
        CloseHandle(event);
    }
    if let Some(error) = connect_error {
        unsafe {
            CloseHandle(server);
            CloseHandle(client);
        }
        return Err(error);
    }

    let parent = unsafe { IpcStream::from_raw_handle(server, true, true) };
    let child = unsafe { OwnedHandle::from_raw_handle(client as RawHandle) };
    Ok((parent, child))
}

fn raw_handle(handle: &Option<OwnedHandle>) -> HANDLE {
    handle
        .as_ref()
        .map_or(INVALID_HANDLE_VALUE, |h| h.as_raw_handle() as HANDLE)
}

fn crt_descriptor_table(handles: &[Option<OwnedHandle>]) -> Vec<u8> {
    let handle_size = size_of::<HANDLE>();
    let mut table = vec![0xff; size_of::<u32>() + handles.len() + handle_size * handles.len()];
    table[..size_of::<u32>()].copy_from_slice(&(handles.len() as u32).to_ne_bytes());
    let handle_base = size_of::<u32>() + handles.len();
    for (fd, handle) in handles.iter().enumerate() {
        let Some(handle) = handle else {
            table[size_of::<u32>() + fd] = 0;
            continue;
        };
        let raw = handle.as_raw_handle() as isize;
        table[size_of::<u32>() + fd] = crt_flags(raw as HANDLE);
        let start = handle_base + fd * handle_size;
        table[start..start + handle_size].copy_from_slice(&raw.to_ne_bytes());
    }
    table
}

fn crt_flags(handle: HANDLE) -> u8 {
    match unsafe { GetFileType(handle) } {
        FILE_TYPE_PIPE => FOPEN | FPIPE,
        FILE_TYPE_CHAR => FOPEN | FDEV,
        _ => FOPEN,
    }
}

struct AttributeList {
    storage: Vec<usize>,
    initialized: bool,
}

impl AttributeList {
    fn with_handles(handles: &[HANDLE]) -> io::Result<Self> {
        let mut bytes = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let words = bytes.div_ceil(size_of::<usize>());
        let mut list = Self {
            storage: vec![0usize; words],
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(list.as_mut_ptr(), 1, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        list.initialized = true;
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.as_mut_ptr());
            }
        }
    }
}

fn command_line(command: &Command) -> io::Result<Vec<u16>> {
    let mut out = Vec::new();
    append_quoted(&mut out, command.get_program())?;
    for arg in command.get_args() {
        out.push(b' ' as u16);
        append_quoted(&mut out, arg)?;
    }
    out.push(0);
    Ok(out)
}

fn append_quoted(out: &mut Vec<u16>, value: &OsStr) -> io::Result<()> {
    let encoded: Vec<u16> = value.encode_wide().collect();
    if encoded.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "argument contains NUL",
        ));
    }
    let quote = encoded.is_empty()
        || encoded
            .iter()
            .any(|c| *c == b' ' as u16 || *c == b'\t' as u16 || *c == b'"' as u16);
    if !quote {
        out.extend_from_slice(&encoded);
        return Ok(());
    }

    out.push(b'"' as u16);
    let mut slashes = 0usize;
    for c in encoded {
        if c == b'\\' as u16 {
            slashes += 1;
        } else if c == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
            out.push(c);
            slashes = 0;
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
            out.push(c);
            slashes = 0;
        }
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    Ok(())
}

fn environment_block(command: &Command, clear: bool) -> io::Result<Vec<u16>> {
    let mut values: BTreeMap<String, (OsString, OsString)> = BTreeMap::new();
    if !clear {
        for (key, value) in std::env::vars_os() {
            values.insert(key.to_string_lossy().to_uppercase(), (key, value));
        }
    }
    for (key, value) in command.get_envs() {
        let folded = key.to_string_lossy().to_uppercase();
        match value {
            Some(value) => {
                values.insert(folded, (key.to_os_string(), value.to_os_string()));
            }
            None => {
                values.remove(&folded);
            }
        }
    }

    let mut block = Vec::new();
    for (_, (key, value)) in values {
        let key: Vec<u16> = key.encode_wide().collect();
        let value: Vec<u16> = value.encode_wide().collect();
        if key.contains(&0) || value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "environment contains NUL",
            ));
        }
        block.extend_from_slice(&key);
        block.push(b'=' as u16);
        block.extend_from_slice(&value);
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn wide_nul(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "value contains NUL",
        ));
    }
    wide.push(0);
    Ok(wide)
}
