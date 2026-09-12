//! ConPTY OS layer. Only owned OS handles and bytes cross worker threads.
//! The waiter closes the console while the reader continues draining its
//! final frame; ClosePseudoConsole must never run on the JavaScript thread.

use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};

use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::child_process::windows_fork::{command_line, environment_block, wide_nul};

pub(crate) struct PtySpawnRequest {
    pub file: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

pub(crate) struct PtyChild {
    pub pid: i32,
    pub master: Arc<PtySession>,
}

pub(crate) struct PtySession {
    console: Mutex<Console>,
    process: OwnedHandle,
    output: File,
    input: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
}

struct Console(HPCON);

impl Drop for Console {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { ClosePseudoConsole(self.0) };
        }
    }
}

fn pipe() -> io::Result<(File, File)> {
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // CreatePipe returned two distinct, non-inheritable handles.
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

struct Attributes {
    storage: Vec<usize>,
    initialized: bool,
}

impl Attributes {
    fn console(console: HPCON) -> io::Result<Self> {
        let mut bytes = 0;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes) };
        if bytes == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut list = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(list.ptr(), 1, 0, &mut bytes) } == 0 {
            return Err(io::Error::last_os_error());
        }
        list.initialized = true;
        if unsafe {
            UpdateProcThreadAttribute(
                list.ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const _,
                size_of::<HPCON>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }

    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { DeleteProcThreadAttributeList(self.ptr()) };
        }
    }
}

fn dimensions(cols: u16, rows: u16) -> io::Result<COORD> {
    if cols == 0 || rows == 0 || cols > i16::MAX as u16 || rows > i16::MAX as u16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid ConPTY dimensions",
        ));
    }
    Ok(COORD {
        X: cols as i16,
        Y: rows as i16,
    })
}

pub(crate) fn spawn_in_pty(req: &PtySpawnRequest) -> io::Result<PtyChild> {
    let size = dimensions(req.cols, req.rows)?;
    let mut command = Command::new(&req.file);
    command
        .args(&req.args)
        .env_clear()
        .envs(req.env.iter().cloned());
    let mut line = command_line(&command)?;
    let environment = environment_block(&command, true)?;
    let cwd = req.cwd.as_ref().map(|s| wide_nul(s.as_ref())).transpose()?;

    let (input_read, mut input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    let mut console = Console(0);
    let hr = unsafe {
        CreatePseudoConsole(
            size,
            input_read.as_raw_handle(),
            output_write.as_raw_handle(),
            0,
            &mut console.0,
        )
    };
    if hr < 0 {
        return Err(io::Error::other(format!(
            "CreatePseudoConsole failed: 0x{:08x}",
            hr as u32
        )));
    }
    let mut attrs = Attributes::console(console.0)?;
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    // Explicit null standard handles let ConPTY install its console handles.
    // Otherwise a host launched with redirected stdin can pass that EOF pipe
    // through to cmd.exe, which immediately exits instead of reading the PTY.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.lpAttributeList = attrs.ptr();
    let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
    if unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr().cast(),
            cwd.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    unsafe { CloseHandle(info.hThread) };
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
    // ConPTY holds its own references. Retaining these ends prevents EOF.
    drop(input_read);
    drop(output_write);
    let (input, pending) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for bytes in pending {
            if input_write.write_all(&bytes).is_err() {
                break;
            }
        }
    });
    Ok(PtyChild {
        pid: info.dwProcessId as i32,
        master: Arc::new(PtySession {
            console: Mutex::new(console),
            process,
            output: output_read,
            input: Mutex::new(Some(input)),
        }),
    })
}

pub(crate) fn read_pty(session: &PtySession, bytes: &mut [u8]) -> io::Result<usize> {
    (&session.output).read(bytes)
}

pub(crate) fn write_pty(session: &PtySession, bytes: &[u8]) -> bool {
    session
        .input
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|tx| tx.send(bytes.to_vec()).is_ok())
}

pub(crate) fn resize_pty(session: Arc<PtySession>, cols: u16, rows: u16) -> bool {
    let Ok(size) = dimensions(cols, rows) else {
        return false;
    };
    let console = session.console.lock().unwrap();
    console.0 != 0 && unsafe { ResizePseudoConsole(console.0, size) } >= 0
}

pub(crate) fn signal_pty(session: &PtySession, signal: i32) -> bool {
    let process = session.process.as_raw_handle();
    if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
        return false;
    }
    signal == 0 || unsafe { TerminateProcess(process, 1) } != 0
}

pub(crate) fn wait_child(session: Arc<PtySession>) -> (Option<i32>, Option<i32>) {
    let process = session.process.as_raw_handle();
    let mut code = 1;
    if unsafe { WaitForSingleObject(process, INFINITE) } == WAIT_OBJECT_0 {
        unsafe { GetExitCodeProcess(process, &mut code) };
    }
    session.input.lock().unwrap().take();
    let console = {
        let mut guard = session.console.lock().unwrap();
        std::mem::replace(&mut *guard, Console(0))
    };
    // Terminates any remaining attached descendants and flushes the final
    // output frame. The separate reader must remain active until EOF.
    drop(console);
    (Some(code as i32), None)
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
