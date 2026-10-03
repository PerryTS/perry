//! Windows child ownership shared by std launches and launches with custom argv0.
//! Only the latter use CreateProcessW: std cannot select argv[0] independently.
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::ExitStatusExt;
use std::process::{Command, ExitStatus};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, TerminateProcess, WaitForSingleObject, INFINITE,
};

use super::{windows_fork, CpStdio};

pub(super) struct Child {
    process: Process,
    pub(super) stdin: Option<File>,
    pub(super) stdout: Option<File>,
    pub(super) stderr: Option<File>,
}

enum Process {
    Std(std::process::Child),
    Native { handle: OwnedHandle, pid: u32 },
}

impl AsRawHandle for Child {
    fn as_raw_handle(&self) -> RawHandle {
        match &self.process {
            Process::Std(child) => child.as_raw_handle(),
            Process::Native { handle, .. } => handle.as_raw_handle(),
        }
    }
}

impl Child {
    pub(super) fn id(&self) -> u32 {
        match &self.process {
            Process::Std(child) => child.id(),
            Process::Native { pid, .. } => *pid,
        }
    }

    fn native_wait(&self, timeout: u32) -> io::Result<Option<ExitStatus>> {
        let raw = self.as_raw_handle().cast();
        // SAFETY: this child owns the live process handle throughout both calls.
        match unsafe { WaitForSingleObject(raw, timeout) } {
            0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(raw, &mut code) } == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(Some(ExitStatus::from_raw(code)))
                }
            }
            258 => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match &mut self.process {
            Process::Std(child) => child.try_wait(),
            Process::Native { .. } => self.native_wait(0),
        }
    }

    pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.stdin.take());
        match &mut self.process {
            Process::Std(child) => child.wait(),
            Process::Native { .. } => self
                .native_wait(INFINITE)?
                .ok_or_else(|| io::Error::other("process wait completed without an exit status")),
        }
    }

    pub(super) fn kill(&mut self) -> io::Result<()> {
        match &mut self.process {
            Process::Std(child) => child.kill(),
            Process::Native { handle, .. } => {
                // SAFETY: the owned process handle cannot be reused during this call.
                if unsafe { TerminateProcess(handle.as_raw_handle().cast(), 1) } == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            }
        }
    }
}

pub(super) fn spawn(
    command: &mut Command,
    stdio: &[CpStdio],
    argv0: Option<&str>,
    clear_environment: bool,
    detached: bool,
) -> io::Result<Child> {
    if let Some(argv0) = argv0 {
        let (child, _) = windows_fork::spawn(
            command,
            stdio,
            None,
            Some(argv0),
            clear_environment,
            detached,
        )?;
        Ok(Child {
            process: Process::Native {
                handle: child.process,
                pid: child.pid,
            },
            stdin: child.stdin,
            stdout: child.stdout,
            stderr: child.stderr,
        })
    } else {
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().map(|p| File::from(OwnedHandle::from(p)));
        let stdout = child
            .stdout
            .take()
            .map(|p| File::from(OwnedHandle::from(p)));
        let stderr = child
            .stderr
            .take()
            .map(|p| File::from(OwnedHandle::from(p)));
        Ok(Child {
            process: Process::Std(child),
            stdin,
            stdout,
            stderr,
        })
    }
}
