use super::{Detached, Kind, Native, bool_result, invalid, os_error, port::owned, unsupported};
use crate::{ChildFdSource, ExitStatus, Notifier, ProcessSpec, ProcessStdio, Result, Signal};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString, c_void},
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{Console::*, JobObjects::*, Pipes::*, Threading::*},
    UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWDEFAULT},
};

fn job(limits: u32) -> Result<OwnedHandle> {
    // SAFETY: unnamed, non-inheritable job with unique ownership.
    let job = unsafe { owned(CreateJobObjectW(ptr::null(), ptr::null())) }?;
    // SAFETY: initialized C structure for the requested information class.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = limits;
    // SAFETY: live job and correctly sized initialized information structure.
    bool_result(unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            ptr::from_ref(&info).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    })?;
    Ok(job)
}

fn lifetime_job() -> Result<&'static OwnedHandle> {
    // Like libuv, retain one non-inheritable handle until process death, rather
    // than releasing a kill-on-close job when an individual leader exits. Silent
    // breakaway excludes grandchildren unless explicitly added by this process.
    // https://github.com/libuv/libuv/blob/v1.52.1/src/win/process.c
    static JOB: OnceLock<Result<OwnedHandle>> = OnceLock::new();
    JOB.get_or_init(|| {
        job(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_BREAKAWAY_OK
            | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION)
    })
    .as_ref()
    .map_err(|error| *error)
}

fn creation_flags(spec: &ProcessSpec) -> u32 {
    let mut flags = CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT;
    if spec.windows_hide
        && !spec
            .stdio
            .iter()
            .any(|stdio| matches!(stdio, ProcessStdio::Inherit | ProcessStdio::Handle(_)))
    {
        flags |= CREATE_NO_WINDOW;
    }
    if spec.detached {
        flags |= DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    }
    flags
}

fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(invalid());
    }
    value.push(0);
    Ok(value)
}
fn quote(value: &OsStr, out: &mut Vec<u16>) -> Result<()> {
    out.push(b'"' as u16);
    let mut slashes = 0;
    for unit in value.encode_wide() {
        if unit == 0 {
            return Err(invalid());
        }
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        slashes = 0;
        out.push(unit);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    Ok(())
}
fn program(spec: &ProcessSpec) -> Result<PathBuf> {
    let path = resolve_program(spec)?;
    // CreateProcess may dispatch batch files through cmd.exe, whose argument
    // parsing does not obey the MSVCRT quoting used below. Require an explicit shell.
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"))
    {
        return Err(invalid());
    }
    Ok(path)
}
fn resolve_program(spec: &ProcessSpec) -> Result<PathBuf> {
    let path = PathBuf::from(&spec.program);
    if path.is_absolute() || path.components().count() > 1 {
        return Ok(path);
    }
    let paths = spec
        .env
        .iter()
        .rev()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.clone())
        .or_else(|| {
            if spec.env_clear {
                None
            } else {
                std::env::var_os("PATH")
            }
        });
    if let Some(paths) = paths {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join(&path);
            if candidate.is_file() {
                return Ok(candidate);
            }
            if candidate.extension().is_none() {
                let executable = candidate.with_extension("exe");
                if executable.is_file() {
                    return Ok(executable);
                }
            }
        }
    }
    Ok(path)
}
pub(super) fn duplicate(handle: HANDLE, inherit: bool) -> Result<OwnedHandle> {
    let mut out = ptr::null_mut();
    // SAFETY: source belongs to this process; returned handle has independent ownership.
    bool_result(unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut out,
            0,
            i32::from(inherit),
            DUPLICATE_SAME_ACCESS,
        )
    })?;
    // SAFETY: successful DuplicateHandle transferred unique ownership.
    unsafe { owned(out) }.map_err(Into::into)
}
pub(super) fn null(access: u32) -> Result<OwnedHandle> {
    let name: Vec<u16> = "NUL\0".encode_utf16().collect();
    // SAFETY: valid device path; exclusive ownership of newly created handle.
    unsafe {
        owned(CreateFileW(
            name.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        ))
    }
    .map_err(Into::into)
}
pub(super) fn stdio(index: usize, inherit: bool) -> Result<OwnedHandle> {
    // SAFETY: query borrows the host's original standard handle, never closes it.
    let handle =
        unsafe { GetStdHandle([STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE][index]) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        let handle = null(read_or_write(index == 0))?;
        return duplicate(handle.as_raw_handle(), inherit);
    }
    duplicate(handle, inherit)
}
/// Which ends of a child pipe each side may use.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Direction {
    /// Child stdin: the parent writes, the child reads.
    ParentWrites,
    /// Child stdout/stderr and a one-way extra descriptor: the child writes.
    ParentReads,
    /// Both ends readable and writable, as Node's stdio pipes and IPC channel are.
    Duplex,
}
fn read_or_write(read: bool) -> u32 {
    if read { GENERIC_READ } else { GENERIC_WRITE }
}
fn pipe(direction: Direction) -> Result<(Detached, OwnedHandle)> {
    let (parent_access, child_access) = match direction {
        Direction::ParentWrites => (PIPE_ACCESS_OUTBOUND, GENERIC_READ),
        Direction::ParentReads => (PIPE_ACCESS_INBOUND, GENERIC_WRITE),
        Direction::Duplex => (PIPE_ACCESS_DUPLEX, GENERIC_READ | GENERIC_WRITE),
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = format!(
        r"\\.\pipe\turnloop-stdio-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let name = wide(OsStr::new(&name))?;
    // SAFETY: unique local-only pipe, parent uses overlapped I/O.
    let parent = unsafe {
        owned(CreateNamedPipeW(
            name.as_ptr(),
            FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE | parent_access,
            PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            65536,
            65536,
            0,
            ptr::null(),
        ))
    }?;
    // SAFETY: child end is synchronous so ordinary child stdio APIs work.
    let child = unsafe {
        owned(CreateFileW(
            name.as_ptr(),
            child_access,
            0,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        ))
    }?;
    // SAFETY: initialized inactive operation; the successful client open has already
    // connected this instance, so ERROR_PIPE_CONNECTED has no outstanding I/O.
    let mut overlapped = unsafe { std::mem::zeroed() };
    // SAFETY: live parent and stack output retained for this already-connected call.
    let ok = unsafe { ConnectNamedPipe(parent.as_raw_handle(), &mut overlapped) };
    if ok == 0 && os_error().os != Some(ERROR_PIPE_CONNECTED as i32) {
        return Err(os_error());
    }
    Ok((
        Detached::new(Native::Handle(parent), Kind::Pipe, false),
        child,
    ))
}
struct Attributes(Vec<usize>);
impl Attributes {
    fn new(handles: &[HANDLE]) -> Result<Self> {
        let mut bytes = 0;
        // SAFETY: documented sizing query, no output list yet.
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut bytes);
        }
        if bytes == 0 {
            return Err(os_error());
        }
        let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        // SAFETY: correctly sized and aligned opaque list storage.
        bool_result(unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 1, 0, &mut bytes)
        })?;
        let mut list = Self(storage);
        // SAFETY: inherited handles remain live and slice remains fixed through spawn.
        bool_result(unsafe {
            UpdateProcThreadAttribute(
                list.0.as_mut_ptr().cast(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
                ptr::null_mut(),
                ptr::null(),
            )
        })?;
        Ok(list)
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: successful initialization, uniquely owned allocation still live.
        unsafe {
            DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast());
        }
    }
}
struct Context {
    ready: AtomicBool,
    notifier: Notifier,
}
unsafe extern "system" fn exited(context: *mut c_void, _: bool) {
    // SAFETY: Arc allocation remains owned until the registered wait is joined.
    let context = unsafe { &*context.cast::<Context>() };
    context.ready.store(true, Ordering::Release);
    let _ = context.notifier.notify(); // NotFound is permitted during loop destruction.
}
pub(super) struct Child {
    process: OwnedHandle,
    thread: OwnedHandle,
    job: Option<OwnedHandle>,
    job_assigned: bool,
    // A successful request (or an observed exit code) precedes actual exit.
    // Only the wait callback/signaled handle may publish completion readiness.
    terminating: bool,
    context: Arc<Context>,
    wait: HANDLE,
    status: Option<ExitStatus>,
    #[cfg(test)]
    terminate_override: Option<fn(HANDLE, bool) -> Result<()>>,
    #[cfg(test)]
    exit_code_override: Option<fn(HANDLE) -> Result<u32>>,
    pub pid: u32,
}
impl Child {
    pub(super) fn ready(&self) -> bool {
        self.context.ready.load(Ordering::Acquire) || self.status.is_some()
    }
    pub(super) fn status(&mut self) -> Result<Option<ExitStatus>> {
        if !self.ready() {
            return Ok(None);
        }
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let code = self.exit_code()?;
        self.join()?;
        let status = ExitStatus {
            code: Some(code as i32),
            signal: None,
        };
        self.status = Some(status);
        Ok(Some(status))
    }
    fn join(&mut self) -> Result<()> {
        if !self.wait.is_null() {
            // SAFETY: called outside callback, joining before freeing callback state.
            bool_result(unsafe { UnregisterWaitEx(self.wait, INVALID_HANDLE_VALUE) })?;
            self.wait = ptr::null_mut();
        }
        Ok(())
    }
    pub(super) fn resume(&self) -> Result<()> {
        // SAFETY: owned suspended main thread; all resources/job/wait already installed.
        if unsafe { ResumeThread(self.thread.as_raw_handle()) } == u32::MAX {
            Err(os_error())
        } else {
            Ok(())
        }
    }
    pub(super) fn kill(&mut self, signal: Signal, group: bool) -> Result<()> {
        if self.status.is_some() || self.terminating {
            return Err(crate::Error::new(crate::ErrorKind::NotFound));
        }
        if signal != Signal::Kill {
            return Err(unsupported());
        }
        let result = self.terminate(group);
        if result.is_ok() {
            self.terminating = true;
        } else if let Err(error) = result
            && error.os == Some(ERROR_ACCESS_DENIED as i32)
        {
            // The exit code may be set before the process handle is signaled.
            // Preserve the original termination error if the query fails.
            let exiting = self
                .exit_code()
                .is_ok_and(|code| code != STILL_ACTIVE as u32);
            // SAFETY: owned identity; zero timeout queries actual exit without blocking.
            let exited =
                unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } == WAIT_OBJECT_0;
            if exited {
                self.context.ready.store(true, Ordering::Release);
            }
            if exiting || exited {
                self.terminating = true;
                return Err(crate::Error::new(crate::ErrorKind::NotFound));
            }
        }
        result
    }
    fn terminate(&self, group: bool) -> Result<()> {
        let handle = if group {
            self.job.as_ref().ok_or_else(invalid)?.as_raw_handle()
        } else {
            self.process.as_raw_handle()
        };
        #[cfg(test)]
        if let Some(terminate) = self.terminate_override {
            return terminate(handle, group);
        }
        if group {
            // SAFETY: exclusively owned process tree; no PID/handle reuse is possible.
            bool_result(unsafe { TerminateJobObject(handle, 1) })
        } else {
            // SAFETY: process handle pins the identity even if its PID is later reused.
            bool_result(unsafe { TerminateProcess(handle, 1) })
        }
    }
    fn exit_code(&self) -> Result<u32> {
        #[cfg(test)]
        if let Some(exit_code) = self.exit_code_override {
            return exit_code(self.process.as_raw_handle());
        }
        let mut code = 0;
        // SAFETY: owned process handle and writable exit-code output.
        bool_result(unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) })?;
        Ok(code)
    }
    pub(super) fn close(&mut self) -> Result<()> {
        if self.status.is_some() {
            return Ok(());
        }
        // A callback can lag behind actual exit. Closing that leader must not
        // become an implicit tree kill while grandchildren are still running.
        if self.ready() || self.exited_now() {
            self.status()?;
            return Ok(());
        }
        if self.terminating {
            return Ok(());
        }
        match self.kill(Signal::Kill, self.job.is_some()) {
            Err(error) if error.kind == crate::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
    fn exited_now(&self) -> bool {
        // SAFETY: owned process pins the identity; this is a nonblocking query.
        let exited =
            unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } == WAIT_OBJECT_0;
        if exited {
            self.context.ready.store(true, Ordering::Release);
        }
        exited
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        if self.status.is_none() && self.exited_now() && self.status().is_err() {
            std::process::abort();
        }
        if self.status.is_none() {
            if !self.terminating && self.job_assigned {
                let _ = self.kill(Signal::Kill, true);
            }
            if !self.terminating {
                // Also covers failed job assignment during suspended spawn.
                let _ = self.kill(Signal::Kill, false);
            }
            // SAFETY: teardown owns the process; a termination request is not
            // completion, so join the real exit before releasing its resources.
            unsafe {
                WaitForSingleObject(self.process.as_raw_handle(), INFINITE);
            }
        }
        if self.join().is_err() {
            std::process::abort();
        }
    }
}
/// C run-time descriptor flags, as every MSVCRT/UCRT program reads them back out
/// of the inherited-descriptor block at startup.
const FOPEN: u8 = 0x01;
const FPIPE: u8 = 0x08;
const FDEV: u8 = 0x40;

fn crt_flags(handle: HANDLE) -> u8 {
    // SAFETY: live inheritable handle; the query only classifies it.
    FOPEN
        | match unsafe { GetFileType(handle) } {
            FILE_TYPE_PIPE => FPIPE,
            FILE_TYPE_CHAR => FDEV,
            _ => 0,
        }
}

/// The inherited-descriptor block passed through `STARTUPINFOW.lpReserved2`.
///
/// This is how a child gets descriptor numbers at all on Windows, and it is the
/// same convention libuv and Node use, so `NODE_CHANNEL_FD=3` names the same
/// thing in a child here as it does under Node. The layout is the C run-time's:
/// a descriptor count, then one flag byte per descriptor, then one handle per
/// descriptor, all packed without padding, so every handle is written as bytes.
/// Unused numbers below the highest one in use are present and closed.
fn inherited_block(slots: &[Option<OwnedHandle>]) -> Result<Vec<u8>> {
    let count = slots.len();
    let width = size_of::<usize>();
    let bytes = size_of::<i32>() + count + count * width;
    // cbReserved2 is a u16; MAX_CHILD_FD keeps this far below the limit.
    if u16::try_from(bytes).is_err() {
        return Err(invalid());
    }
    let mut block = vec![0u8; bytes];
    block[..size_of::<i32>()].copy_from_slice(&(count as i32).to_ne_bytes());
    for (i, slot) in slots.iter().enumerate() {
        let (flags, handle) = match slot {
            Some(handle) => (
                crt_flags(handle.as_raw_handle()),
                handle.as_raw_handle() as usize,
            ),
            None => (0, INVALID_HANDLE_VALUE as usize),
        };
        block[size_of::<i32>() + i] = flags;
        let at = size_of::<i32>() + count + i * width;
        block[at..at + width].copy_from_slice(&handle.to_ne_bytes());
    }
    Ok(block)
}

/// A started child, the parent ends of its standard streams, and the parent ends
/// of its extra descriptors in `ProcessSpec::extra` order.
type Spawned = (Child, [Option<Detached>; 3], Vec<Option<Detached>>);

pub(super) fn spawn(
    spec: &ProcessSpec,
    existing: [Option<HANDLE>; 3],
    extra_sources: &[Option<HANDLE>],
    notifier: Notifier,
) -> Result<Spawned> {
    if spec.uid.is_some() || spec.gid.is_some() || spec.program.is_empty() {
        return Err(unsupported());
    }
    if spec.controlling_terminal {
        // Windows has no session/controlling-terminal concept to claim.
        return Err(unsupported());
    }
    let application = wide(program(spec)?.as_os_str())?;
    let mut parents = [None, None, None];
    let slot_count = spec
        .extra
        .iter()
        .map(|fd| fd.number as usize + 1)
        .max()
        .unwrap_or(0)
        .max(3);
    let mut slots: Vec<Option<OwnedHandle>> = (0..slot_count).map(|_| None).collect();
    for (i, option) in spec.stdio.iter().enumerate() {
        let handle = match option {
            ProcessStdio::Inherit => stdio(i, true)?,
            ProcessStdio::Null => {
                let handle = null(read_or_write(i == 0))?;
                duplicate(handle.as_raw_handle(), true)?
            }
            ProcessStdio::Handle(_) => duplicate(existing[i].ok_or_else(invalid)?, true)?,
            ProcessStdio::Pipe => {
                let (parent, child) = pipe(if i == 0 {
                    Direction::ParentWrites
                } else {
                    Direction::ParentReads
                })?;
                parents[i] = Some(parent);
                duplicate(child.as_raw_handle(), true)?
            }
        };
        slots[i] = Some(handle);
    }
    let mut extra_parents = Vec::with_capacity(spec.extra.len());
    for (i, fd) in spec.extra.iter().enumerate() {
        let (handle, parent) = match fd.source {
            ChildFdSource::Null => {
                let handle = null(GENERIC_READ | GENERIC_WRITE)?;
                (duplicate(handle.as_raw_handle(), true)?, None)
            }
            ChildFdSource::Handle(_) => (
                duplicate(
                    extra_sources
                        .get(i)
                        .copied()
                        .flatten()
                        .ok_or_else(invalid)?,
                    true,
                )?,
                None,
            ),
            ChildFdSource::Pipe => {
                let (parent, child) = pipe(Direction::ParentReads)?;
                (duplicate(child.as_raw_handle(), true)?, Some(parent))
            }
            ChildFdSource::Duplex => {
                let (parent, child) = pipe(Direction::Duplex)?;
                (duplicate(child.as_raw_handle(), true)?, Some(parent))
            }
        };
        let slot = slots.get_mut(fd.number as usize).ok_or_else(invalid)?;
        if slot.is_some() {
            return Err(invalid());
        }
        *slot = Some(handle);
        extra_parents.push(parent);
    }
    let handles: Vec<HANDLE> = slots
        .iter()
        .flatten()
        .map(AsRawHandle::as_raw_handle)
        .collect();
    let mut attributes = Attributes::new(&handles)?;
    let mut block = inherited_block(&slots)?;
    let mut command = Vec::new();
    quote(&spec.program, &mut command)?;
    for arg in &spec.args {
        command.push(b' ' as u16);
        quote(arg, &mut command)?;
    }
    command.push(0);
    if command.len() > 32767 {
        return Err(invalid());
    }
    let mut vars: BTreeMap<OsString, (OsString, OsString)> = BTreeMap::new();
    if !spec.env_clear {
        for (key, value) in std::env::vars_os() {
            vars.insert(key.to_ascii_uppercase(), (key, value));
        }
    }
    for (key, value) in &spec.env {
        if key.is_empty()
            || key
                .encode_wide()
                .any(|unit| unit == 0 || unit == b'=' as u16)
        {
            return Err(invalid());
        }
        vars.insert(key.to_ascii_uppercase(), (key.clone(), value.clone()));
    }
    let mut environment = Vec::new();
    for (_, (key, value)) in vars {
        let mut entry = key;
        entry.push("=");
        entry.push(value);
        environment.extend(wide(&entry)?);
    }
    if environment.is_empty() {
        environment.push(0);
    }
    environment.push(0);
    let cwd = spec
        .cwd
        .as_ref()
        .map(|path| wide(path.as_os_str()))
        .transpose()?;
    // SAFETY: initialized C startup and result structures with correct cb.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    startup.StartupInfo.wShowWindow = if spec.windows_hide {
        SW_HIDE
    } else {
        SW_SHOWDEFAULT
    } as u16;
    let standard = std::array::from_fn::<_, 3, _>(|i| {
        slots[i]
            .as_ref()
            .map_or(ptr::null_mut(), AsRawHandle::as_raw_handle)
    });
    startup.StartupInfo.hStdInput = standard[0];
    startup.StartupInfo.hStdOutput = standard[1];
    startup.StartupInfo.hStdError = standard[2];
    // The child's own descriptor numbers, including 0..2, come from here.
    startup.StartupInfo.cbReserved2 = block.len() as u16;
    startup.StartupInfo.lpReserved2 = block.as_mut_ptr();
    startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
    let job = if spec.new_process_group || spec.detached {
        // Explicit tree control is separate from parent lifetime. Releasing this
        // job after a normal leader exit must leave its descendants running.
        Some(job(JOB_OBJECT_LIMIT_BREAKAWAY_OK)?)
    } else {
        None
    };
    let lifetime = if spec.detached {
        None
    } else {
        Some(lifetime_job()?)
    };
    // SAFETY: plain writable process output structure.
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: explicit application and quoted writable command line; environment,
    // directory, attribute list, inherited-descriptor block and exactly the
    // listed inherited handles all stay live across this call.
    bool_result(unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            creation_flags(spec),
            environment.as_ptr().cast(),
            cwd.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        )
    })?;
    // SAFETY: successful creation transfers these two independent handle owners.
    let mut child = unsafe {
        Child {
            process: owned(info.hProcess)?,
            thread: owned(info.hThread)?,
            job,
            job_assigned: false,
            terminating: false,
            context: Arc::new(Context {
                ready: AtomicBool::new(false),
                notifier,
            }),
            wait: ptr::null_mut(),
            status: None,
            #[cfg(test)]
            terminate_override: None,
            #[cfg(test)]
            exit_code_override: None,
            pid: info.dwProcessId,
        }
    };
    if let Some(job) = lifetime {
        // SAFETY: child is suspended and both handles are owned. Like libuv,
        // tolerate host job restrictions, without claiming assignment succeeded.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.process.as_raw_handle()) }
            == 0
        {
            let error = os_error();
            if error.os != Some(ERROR_ACCESS_DENIED as i32) {
                return Err(error);
            }
        }
    }
    if let Some(job) = &child.job {
        // SAFETY: child still suspended, so no descendant can escape job assignment.
        bool_result(unsafe {
            AssignProcessToJobObject(job.as_raw_handle(), child.process.as_raw_handle())
        })?;
        child.job_assigned = true;
    }
    // SAFETY: initialized callback context and owned child precede registration;
    // immediate exits are safe and Drop joins the one-shot callback.
    bool_result(unsafe {
        RegisterWaitForSingleObject(
            &mut child.wait,
            child.process.as_raw_handle(),
            Some(exited),
            Arc::as_ptr(&child.context).cast(),
            INFINITE,
            WT_EXECUTEONLYONCE,
        )
    })?;
    Ok((child, parents, extra_parents))
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn detached_children_skip_lifetime_job_but_keep_explicit_group_kill() {
        let driver = crate::Loop::new(crate::Config::default()).expect("notifier owner");
        let lifetime = lifetime_job().expect("lifetime job");
        let mut checked = 0;
        for detached in [false, true] {
            let mut spec = ProcessSpec::new(std::env::current_exe().expect("executable"));
            spec.args.push("--list".into());
            spec.stdio = [ProcessStdio::Null; 3];
            spec.windows_hide = true;
            spec.detached = detached;
            let (mut child, _, _) =
                spawn(&spec, [None; 3], &[], driver.notifier()).expect("suspended child");
            let mut member = -1;
            assert_ne!(
                // SAFETY: both owned live handles and writable membership output.
                unsafe {
                    IsProcessInJob(
                        child.process.as_raw_handle(),
                        lifetime.as_raw_handle(),
                        &mut member,
                    )
                },
                0
            );
            assert_eq!(member, i32::from(!detached));
            assert_unsignaled(&mut child);
            if detached {
                assert!(child.job_assigned);
                child.kill(Signal::Kill, true).expect("detached group kill");
            } else {
                child
                    .kill(Signal::Kill, false)
                    .expect("ordinary child kill");
            }
            assert_eq!(
                // SAFETY: owned child process and bounded wait for actual termination.
                unsafe { WaitForSingleObject(child.process.as_raw_handle(), 5000) },
                WAIT_OBJECT_0
            );
            checked += 1;
        }
        assert_eq!(checked, 2);
    }

    thread_local! {
        static TERMINATIONS: Cell<[usize; 2]> = const { Cell::new([0; 2]) };
        static EXIT_QUERIES: Cell<usize> = const { Cell::new(0) };
    }

    fn count_termination(group: bool) {
        TERMINATIONS.with(|count| {
            let mut calls = count.get();
            calls[usize::from(group)] += 1;
            count.set(calls);
        });
    }

    fn denied(_: HANDLE, group: bool) -> Result<()> {
        count_termination(group);
        Err(std::io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32).into())
    }

    fn suspended(driver: &crate::Loop, group: bool) -> Child {
        let mut spec = ProcessSpec::new(std::env::current_exe().expect("test executable"));
        spec.windows_hide = true;
        spec.args.push("--list".into());
        spec.stdio = [ProcessStdio::Null; 3];
        spec.new_process_group = group;
        spawn(&spec, [None; 3], &[], driver.notifier())
            .expect("suspended child")
            .0
    }

    // Fault injection never kills this suspended child. Restore real teardown on
    // assertion failure as well as success, so a failed test cannot leak/hang it.
    struct InjectedChild(Child);
    impl Drop for InjectedChild {
        fn drop(&mut self) {
            self.0.terminate_override = None;
            self.0.exit_code_override = None;
            self.0.terminating = false;
        }
    }

    fn assert_unsignaled(child: &mut Child) {
        assert_eq!(
            // SAFETY: owned process handle, nonblocking liveness query.
            unsafe { WaitForSingleObject(child.process.as_raw_handle(), 0) },
            WAIT_TIMEOUT
        );
        assert!(!child.ready(), "termination request must not publish exit");
        assert_eq!(child.status().expect("pending exit"), None);
    }

    #[test]
    fn terminating_process_without_callback_does_not_repeat_kill_or_complete_early() {
        let driver = crate::Loop::new(crate::Config::default()).expect("notifier owner");
        let mut checked = 0;
        for group in [false, true] {
            for access_denied in [false, true] {
                let mut fixture = InjectedChild(suspended(&driver, group));
                let child = &mut fixture.0;
                child.join().expect("remove callback while suspended");
                assert_unsignaled(child);
                TERMINATIONS.with(|count| count.set([0; 2]));
                EXIT_QUERIES.with(|count| count.set(0));
                child.terminate_override = Some(if access_denied {
                    denied
                } else {
                    |_, group| {
                        count_termination(group);
                        Ok(())
                    }
                });
                child.exit_code_override = Some(|_| {
                    EXIT_QUERIES.with(|count| count.set(count.get() + 1));
                    Ok(1)
                });
                // Model the OS boundary deterministically: success or access
                // denied with an exit code, while the real handle is unsignaled.
                // Suspending a child alone cannot hold Windows in this window.
                let result = child.kill(Signal::Kill, group);
                if access_denied {
                    assert_eq!(
                        result.expect_err("termination underway").kind,
                        crate::ErrorKind::NotFound
                    );
                } else {
                    result.expect("termination accepted");
                }
                assert!(child.terminating);
                assert_unsignaled(child);
                for repeat_group in [false, true] {
                    assert_eq!(
                        child
                            .kill(Signal::Kill, repeat_group)
                            .expect_err("repeat kill")
                            .kind,
                        crate::ErrorKind::NotFound
                    );
                }
                child.close().expect("close while terminating");
                child.close().expect("repeat close while terminating");
                assert_unsignaled(child);
                assert_eq!(
                    TERMINATIONS.with(Cell::get),
                    if group { [0, 1] } else { [1, 0] }
                );
                assert_eq!(EXIT_QUERIES.with(Cell::get), usize::from(access_denied));
                checked += 1;
            }
        }
        assert_eq!(checked, 4);
    }

    #[test]
    fn termination_errors_keep_their_identity_and_allow_retry() {
        let driver = crate::Loop::new(crate::Config::default()).expect("notifier owner");
        let mut checked = 0;
        for group in [false, true] {
            for query_fails in [false, true] {
                let mut fixture = InjectedChild(suspended(&driver, group));
                let child = &mut fixture.0;
                child.join().expect("unregister suspended child");
                child.terminate_override = Some(denied);
                if query_fails {
                    child.exit_code_override = Some(|_| {
                        Err(std::io::Error::from_raw_os_error(ERROR_INVALID_HANDLE as i32).into())
                    });
                } // Otherwise GetExitCodeProcess really returns STILL_ACTIVE.
                TERMINATIONS.with(|count| count.set([0; 2]));
                let original: crate::Error =
                    std::io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32).into();
                assert_eq!(child.kill(Signal::Kill, group), Err(original));
                assert_eq!(child.close(), Err(original));
                assert!(!child.terminating);
                assert_unsignaled(child);
                assert_eq!(
                    TERMINATIONS.with(Cell::get),
                    if group { [0, 2] } else { [2, 0] }
                );
                // A different termination error must not be hidden by an exit query.
                child.terminate_override = Some(|_, group| {
                    count_termination(group);
                    Err(std::io::Error::from_raw_os_error(ERROR_INVALID_HANDLE as i32).into())
                });
                child.exit_code_override = Some(|_| panic!("query after unrelated error"));
                let original: crate::Error =
                    std::io::Error::from_raw_os_error(ERROR_INVALID_HANDLE as i32).into();
                assert_eq!(child.kill(Signal::Kill, group), Err(original));
                assert_eq!(child.close(), Err(original));
                assert!(!child.terminating);
                assert_unsignaled(child);
                assert_eq!(
                    TERMINATIONS.with(Cell::get),
                    if group { [0, 4] } else { [4, 0] }
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 4);
    }

    #[test]
    fn drop_joins_termination_without_repeating_it_and_handles_unassigned_jobs() {
        let driver = crate::Loop::new(crate::Config::default()).expect("notifier owner");
        let mut checked = 0;
        for (group, kill_first, unassigned_job) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, true),
        ] {
            let mut child = suspended(&driver, group);
            if unassigned_job {
                // Reproduce spawn unwinding before job assignment succeeds.
                // SAFETY: unnamed owned job, never assigned this suspended child.
                child.job = Some(
                    unsafe { owned(CreateJobObjectW(ptr::null(), ptr::null())) }
                        .expect("unassigned job"),
                );
                assert!(!child.job_assigned);
            }
            let wait = child.process.try_clone().expect("duplicate process");
            TERMINATIONS.with(|count| count.set([0; 2]));
            child.terminate_override = Some(|handle, group| {
                count_termination(group);
                // SAFETY: wrapper receives the same owned process/job as the real call.
                bool_result(unsafe {
                    if group {
                        TerminateJobObject(handle, 1)
                    } else {
                        TerminateProcess(handle, 1)
                    }
                })
            });
            if kill_first {
                child.kill(Signal::Kill, group).expect("initial kill");
            }
            drop(child);
            assert_eq!(
                TERMINATIONS.with(Cell::get),
                if group { [0, 1] } else { [1, 0] }
            );
            assert_eq!(
                // SAFETY: owned duplicate survives Child::drop; nonblocking exit query.
                unsafe { WaitForSingleObject(wait.as_raw_handle(), 0) },
                WAIT_OBJECT_0
            );
            checked += 1;
        }
        assert_eq!(checked, 5);
    }

    #[test]
    fn exited_process_without_callback_maps_kill_and_close() {
        let driver = crate::Loop::new(crate::Config::default()).expect("notifier owner");
        for close in [false, true] {
            // Listing the unit-test binary exits normally without running any
            // nested tests and needs no external fixture or shell argument rules.
            let mut spec = ProcessSpec::new(std::env::current_exe().expect("test executable"));
            spec.windows_hide = true;
            spec.args.push("--list".into());
            spec.stdio = [ProcessStdio::Null; 3];
            let (mut child, _, _) =
                spawn(&spec, [None; 3], &[], driver.notifier()).expect("suspended child");
            // The child cannot exit while suspended. Removing its wait now forces
            // the exact callback-lag window, independent of thread-pool scheduling.
            child.join().expect("unregister before resume");
            let wait = child.process.try_clone().expect("duplicate process");
            child.resume().expect("resume child");
            assert_eq!(
                // SAFETY: owned duplicate and bounded wait, with no callback installed.
                unsafe { WaitForSingleObject(wait.as_raw_handle(), 10_000) },
                WAIT_OBJECT_0
            );
            assert!(!child.ready(), "the exit callback must not have run");
            if close {
                child
                    .close()
                    .expect("close signaled process without callback");
            } else {
                assert_eq!(
                    child
                        .kill(Signal::Kill, false)
                        .expect_err("already exited")
                        .kind,
                    crate::ErrorKind::NotFound
                );
            }
            assert!(
                child.ready(),
                "termination failure observed the signaled handle"
            );
            assert_eq!(
                child.status().expect("exit status").expect("exited").code,
                Some(0)
            );
        }
    }
}
