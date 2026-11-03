//! Capability-scoped `wasi:filesystem` requests shared by the WASI 0.2 and 0.3
//! backends. A WASI agent has no threads, so the blocking pool is replaced by
//! bounded execution inside `poll`: each turn runs at most its event budget of
//! requests, then waits as usual. Requests on one handle run FIFO. Cancelling a
//! request that has not run completes it at once without touching its buffer.
//!
//! Paths resolve only against preopened directories: an absolute path uses the
//! preopen with the longest matching name, a relative path the preopen named `.`.
use crate::slots::{Slots, page_reserve};
use crate::{
    backend::{Event, Outcome},
    fs::{
        FileMetadata, FileOptions, FileType, FsOutput, FsRequest, FsTarget, TimeChange, put_record,
    },
    *,
};
use std::collections::VecDeque;

/// Binding surface of one `wasi:filesystem` version. Calls may block the agent.
pub(super) trait Api {
    type Descriptor;
    type Appender;
    type Entries;
    fn preopens() -> Vec<(Self::Descriptor, String)>;
    fn open_at(
        dir: &Self::Descriptor,
        path: &str,
        options: &FileOptions,
        directory: bool,
    ) -> Result<Self::Descriptor>;
    /// Positional read into `output`: (bytes, end of file).
    fn read(file: &Self::Descriptor, output: &mut [u8], offset: u64) -> Result<(usize, bool)>;
    fn write(file: &Self::Descriptor, bytes: &[u8], offset: u64) -> Result<usize>;
    fn append(
        file: &Self::Descriptor,
        appender: &mut Option<Self::Appender>,
        bytes: &[u8],
    ) -> Result<usize>;
    fn stat(file: &Self::Descriptor) -> Result<FileMetadata>;
    fn stat_at(dir: &Self::Descriptor, path: &str, follow: bool) -> Result<FileMetadata>;
    fn sync(file: &Self::Descriptor, data_only: bool) -> Result<()>;
    fn set_size(file: &Self::Descriptor, size: u64) -> Result<()>;
    fn set_times(file: &Self::Descriptor, accessed: TimeChange, modified: TimeChange)
    -> Result<()>;
    fn set_times_at(
        dir: &Self::Descriptor,
        path: &str,
        follow: bool,
        accessed: TimeChange,
        modified: TimeChange,
    ) -> Result<()>;
    fn entries(dir: &Self::Descriptor) -> Result<Self::Entries>;
    fn next_entry(entries: &mut Self::Entries) -> Result<Option<(FileType, String)>>;
    fn create_directory_at(dir: &Self::Descriptor, path: &str) -> Result<()>;
    fn remove_directory_at(dir: &Self::Descriptor, path: &str) -> Result<()>;
    fn unlink_file_at(dir: &Self::Descriptor, path: &str) -> Result<()>;
    fn rename_at(
        dir: &Self::Descriptor,
        path: &str,
        new_dir: &Self::Descriptor,
        new_path: &str,
    ) -> Result<()>;
    fn link_at(
        dir: &Self::Descriptor,
        path: &str,
        new_dir: &Self::Descriptor,
        new_path: &str,
    ) -> Result<()>;
    fn symlink_at(dir: &Self::Descriptor, target: &str, path: &str) -> Result<()>;
    fn readlink_at(dir: &Self::Descriptor, path: &str) -> Result<String>;
}

/// wasi-libc errno values (identical in WASI 0.2 and 0.3 error-code naming).
pub(super) mod errno {
    pub const EACCES: i32 = 2;
    pub const EAGAIN: i32 = 6;
    pub const EBADF: i32 = 8;
    pub const EEXIST: i32 = 20;
    pub const EINVAL: i32 = 28;
    pub const EISDIR: i32 = 31;
    pub const ENOENT: i32 = 44;
    pub const ENOMEM: i32 = 48;
    pub const ENOSPC: i32 = 51;
    pub const ENOTDIR: i32 = 54;
    pub const ENOTEMPTY: i32 = 55;
    pub const ENOTSUP: i32 = 58;
    pub const EPERM: i32 = 63;
}
/// Portable kind for a WASI errno, retaining the errno as the native code.
pub(super) fn error(code: i32) -> Error {
    use errno::*;
    let kind = match code {
        EACCES | EPERM => ErrorKind::PermissionDenied,
        EEXIST => ErrorKind::AlreadyExists,
        ENOENT => ErrorKind::NotFound,
        ENOTDIR => ErrorKind::NotADirectory,
        EISDIR => ErrorKind::IsADirectory,
        ENOTEMPTY => ErrorKind::DirectoryNotEmpty,
        EINVAL => ErrorKind::InvalidInput,
        ENOTSUP => ErrorKind::Unsupported,
        EAGAIN => ErrorKind::WouldBlock,
        ENOMEM | ENOSPC => ErrorKind::ResourceLimit,
        _ => ErrorKind::Other,
    };
    Error {
        kind,
        os: Some(code),
    }
}

enum Object<A: Api> {
    File {
        // Streams are children of their descriptor and must drop first.
        appender: Option<A::Appender>,
        descriptor: A::Descriptor,
        cursor: u64,
        append: bool,
    },
    Dir {
        entries: Option<A::Entries>,
        descriptor: A::Descriptor,
        pending: Option<(FileType, String)>,
        done: bool,
    },
    Closed,
}
struct Pending {
    op: OpId,
    handle: Option<Handle>,
    request: Option<FsRequest>,
    previous: Option<usize>,
    next: Option<usize>,
    waiting: bool,
}
pub(super) struct Files<A: Api> {
    preopens: Option<Vec<(A::Descriptor, String)>>,
    objects: Slots<Object<A>>,
    ops: Slots<Pending>,
    heads: Slots<usize>,
    tails: Slots<usize>,
    ready: VecDeque<usize>,
    waiting: VecDeque<usize>,
    cancelled: VecDeque<OpId>,
    pool: BufferPool,
}
impl<A: Api> Files<A> {
    pub fn new(config: &Config, pool: BufferPool) -> Self {
        Self {
            preopens: None,
            objects: Slots::new(config.max_handles),
            ops: Slots::new(config.max_operations),
            heads: Slots::new(config.max_handles),
            tails: Slots::new(config.max_handles),
            ready: VecDeque::with_capacity(page_reserve(config.max_operations)),
            waiting: VecDeque::with_capacity(page_reserve(config.max_operations)),
            cancelled: VecDeque::with_capacity(page_reserve(config.max_operations)),
            pool,
        }
    }
    pub fn submit(&mut self, op: OpId, handle: Option<Handle>, request: FsRequest) -> Result<()> {
        match &request {
            // No wasi:filesystem equivalent exists; reject before acceptance.
            FsRequest::RealPath { .. } | FsRequest::Chmod { .. } | FsRequest::Chown { .. } => {
                return Err(Error::new(ErrorKind::Unsupported));
            }
            FsRequest::Access { mode, .. } if *mode != AccessMode::default() => {
                return Err(Error::new(ErrorKind::Unsupported));
            }
            _ => {}
        }
        let i = op.index();
        if self.ops.get(i).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if let Some(h) = handle {
            let object = self
                .objects
                .get(h.index())
                .ok_or(Error::new(ErrorKind::InvalidInput))?;
            if request.opens() != object.is_none() {
                return Err(Error::new(ErrorKind::InvalidInput));
            }
        }
        let previous = handle.and_then(|h| self.tails[h.index()]);
        self.ops[i] = Some(Pending {
            op,
            handle,
            request: Some(request),
            previous,
            next: None,
            waiting: false,
        });
        if let Some(h) = handle {
            match previous {
                Some(tail) => self.ops[tail].as_mut().expect("FIFO tail").next = Some(i),
                None => self.heads[h.index()] = Some(i),
            }
            self.tails[h.index()] = Some(i);
        }
        if previous.is_none() {
            self.ready.push_back(i);
        }
        Ok(())
    }
    fn unlink(&mut self, i: usize) {
        let p = self.ops[i].as_mut().expect("linked request");
        let (previous, next, handle) = (p.previous.take(), p.next.take(), p.handle);
        let Some(h) = handle else { return };
        match previous {
            Some(q) => self.ops[q].as_mut().expect("predecessor").next = next,
            None => self.heads[h.index()] = next,
        }
        match next {
            Some(n) => {
                self.ops[n].as_mut().expect("successor").previous = previous;
                if previous.is_none() {
                    self.ready.push_back(n);
                }
            }
            None => self.tails[h.index()] = previous,
        }
    }
    pub fn cancel(&mut self, op: OpId) -> bool {
        let i = op.index();
        let Some(p) = self
            .ops
            .get(i)
            .and_then(Option::as_ref)
            .filter(|p| p.op == op)
        else {
            return false;
        };
        let (head, waiting) = (p.previous.is_none(), p.waiting);
        if head {
            if waiting {
                self.waiting.retain(|&w| w != i);
            } else {
                self.ready.retain(|&r| r != i);
            }
        }
        self.unlink(i);
        self.ops[i] = None;
        self.cancelled.push_back(op);
        true
    }
    pub fn has_work(&self) -> bool {
        !self.ready.is_empty()
            || !self.cancelled.is_empty()
            || (!self.waiting.is_empty() && self.pool.available())
    }
    pub fn release(&mut self, h: Handle) {
        if let Some(object) = self.objects.get_mut(h.index()) {
            *object = None;
        }
    }
    /// Run bounded ready requests, appending their terminal events.
    pub fn run<D>(&mut self, events: &mut Vec<Event<D>>) {
        while events.len() < events.capacity() {
            let Some(op) = self.cancelled.pop_front() else {
                break;
            };
            events.push(Event {
                op,
                terminal: true,
                result: Ok(Outcome::Cancelled),
            });
        }
        while self.pool.available() {
            let Some(i) = self.waiting.pop_front() else {
                break;
            };
            self.ops[i].as_mut().expect("waiting request").waiting = false;
            self.ready.push_back(i);
        }
        while events.len() < events.capacity() {
            let Some(i) = self.ready.pop_front() else {
                break;
            };
            let p = self.ops[i].as_mut().expect("ready request");
            let request = p.request.as_mut().expect("unrun request");
            let mut lease = None;
            if let Some(buffer) = request.read_buffer()
                && matches!(buffer, ReadBuf::Pooled)
            {
                let Some(mut acquired) = self.pool.acquire() else {
                    p.waiting = true;
                    self.waiting.push_back(i);
                    continue;
                };
                let bytes = acquired.writable();
                // SAFETY: the lease lives in this frame until the event owns it; the
                // request (and this region) is consumed before the event is pushed.
                *buffer = ReadBuf::Provided(unsafe {
                    IoBufMut::from_raw_parts(bytes.as_mut_ptr(), bytes.len())
                });
                lease = Some(acquired);
            }
            let (op, handle) = (p.op, p.handle);
            let request = p.request.take().expect("unrun request");
            if self.preopens.is_none() && needs_preopens(&request) {
                self.preopens = Some(A::preopens());
            }
            let preopens = self.preopens.as_deref().unwrap_or(&[]);
            let result = execute::<A>(preopens, &mut self.objects, handle, request);
            self.unlink(i);
            self.ops[i] = None;
            events.push(Event {
                op,
                terminal: true,
                result: result.map(|output| Outcome::Fs { output, lease }),
            });
        }
    }
}
fn needs_preopens(request: &FsRequest) -> bool {
    match request {
        FsRequest::Chmod { target, .. }
        | FsRequest::Chown { target, .. }
        | FsRequest::SetTimes { target, .. } => matches!(target, FsTarget::Path { .. }),
        _ => request.handle().is_none(),
    }
}
fn output(buffer: &mut ReadBuf) -> &mut [u8] {
    let ReadBuf::Provided(buffer) = buffer else {
        unreachable!("pooled buffers are provided before a request runs")
    };
    // SAFETY: an accepted request exclusively owns its output region while it runs.
    unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr(), buffer.len()) }
}
fn resolve<'a, D>(preopens: &'a [(D, String)], path: &'a FsPath) -> Result<(&'a D, &'a str)> {
    let text = path.text();
    let mut best: Option<(usize, &D, &str)> = None;
    for (descriptor, name) in preopens {
        let name = name.trim_end_matches('/');
        let rest = if let Some(absolute) = text.strip_prefix('/') {
            if name.is_empty() {
                Some(absolute)
            } else if let Some(rest) = name
                .strip_prefix('/')
                .and_then(|n| absolute.strip_prefix(n))
            {
                if rest.is_empty() {
                    Some("")
                } else {
                    rest.strip_prefix('/')
                }
            } else {
                None
            }
        } else if name == "." {
            Some(text.strip_prefix("./").unwrap_or(text))
        } else {
            None
        };
        if let Some(rest) = rest
            && best.is_none_or(|(length, ..)| name.len() > length)
        {
            best = Some((name.len(), descriptor, rest));
        }
    }
    let (_, descriptor, rest) = best.ok_or_else(|| error(errno::ENOENT))?;
    Ok((descriptor, if rest.is_empty() { "." } else { rest }))
}
fn execute<A: Api>(
    preopens: &[(A::Descriptor, String)],
    objects: &mut Slots<Object<A>>,
    handle: Option<Handle>,
    request: FsRequest,
) -> Result<FsOutput> {
    let object = handle.map(|h| &mut objects[h.index()]);
    let bad = || error(errno::EBADF);
    match request {
        FsRequest::Open { path, options } => {
            let (dir, rel) = resolve(preopens, &path)?;
            let descriptor = A::open_at(dir, rel, &options, false)?;
            *object.expect("open handle") = Some(Object::File {
                appender: None,
                descriptor,
                cursor: 0,
                append: options.append,
            });
            Ok(FsOutput::Opened)
        }
        FsRequest::OpenDir { path } => {
            let (dir, rel) = resolve(preopens, &path)?;
            let descriptor = A::open_at(dir, rel, &FileOptions::default(), true)?;
            *object.expect("open handle") = Some(Object::Dir {
                entries: None,
                descriptor,
                pending: None,
                done: false,
            });
            Ok(FsOutput::Opened)
        }
        FsRequest::Close { .. } => {
            let slot = object.expect("close handle");
            match slot {
                Some(Object::File { .. } | Object::Dir { .. }) => {
                    *slot = Some(Object::Closed);
                    Ok(FsOutput::Done)
                }
                _ => Err(bad()),
            }
        }
        FsRequest::Read {
            mut buffer, offset, ..
        } => {
            let Some(Some(Object::File {
                descriptor, cursor, ..
            })) = object
            else {
                return Err(bad());
            };
            let at = offset.unwrap_or(*cursor);
            let (n, _eof) = A::read(descriptor, output(&mut buffer), at)?;
            if offset.is_none() {
                *cursor += n as u64;
            }
            Ok(FsOutput::Read(n))
        }
        FsRequest::Write { buffer, offset, .. } => {
            let Some(Some(Object::File {
                appender,
                descriptor,
                cursor,
                append,
            })) = object
            else {
                return Err(bad());
            };
            let bytes = buffer.as_slice();
            if *append {
                return A::append(descriptor, appender, bytes).map(FsOutput::Wrote);
            }
            let n = A::write(descriptor, bytes, offset.unwrap_or(*cursor))?;
            if offset.is_none() {
                *cursor += n as u64;
            }
            Ok(FsOutput::Wrote(n))
        }
        FsRequest::Sync { data_only, .. } => {
            A::sync(descriptor(object).ok_or_else(bad)?, data_only).map(|()| FsOutput::Done)
        }
        FsRequest::Truncate { size, .. } => {
            A::set_size(descriptor(object).ok_or_else(bad)?, size).map(|()| FsOutput::Done)
        }
        FsRequest::Stat {
            path,
            follow_symlinks,
        } => {
            let (dir, rel) = resolve(preopens, &path)?;
            A::stat_at(dir, rel, follow_symlinks).map(FsOutput::Metadata)
        }
        FsRequest::Fstat { .. } => {
            A::stat(descriptor(object).ok_or_else(bad)?).map(FsOutput::Metadata)
        }
        FsRequest::ReadDir { mut buffer, .. } => {
            let Some(Some(Object::Dir {
                entries,
                descriptor,
                pending,
                done,
            })) = object
            else {
                return Err(bad());
            };
            let out = output(&mut buffer);
            let mut used = 0;
            loop {
                if pending.is_none() && !*done {
                    if entries.is_none() {
                        *entries = Some(A::entries(descriptor)?);
                    }
                    *pending = A::next_entry(entries.as_mut().expect("entry stream"))?;
                    *done = pending.is_none();
                }
                let Some((kind, name)) = pending.as_ref() else {
                    return Ok(FsOutput::Directory { n: used, eof: true });
                };
                if name == "." || name == ".." {
                    *pending = None;
                    continue;
                }
                if !put_record(out, &mut used, *kind as u8, name.as_bytes()) {
                    if used == 0 {
                        return Err(Error::new(ErrorKind::ResourceLimit));
                    }
                    return Ok(FsOutput::Directory {
                        n: used,
                        eof: false,
                    });
                }
                *pending = None;
            }
        }
        FsRequest::Mkdir { path, .. } => {
            let (dir, rel) = resolve(preopens, &path)?;
            A::create_directory_at(dir, rel).map(|()| FsOutput::Done)
        }
        FsRequest::Rmdir { path } => {
            let (dir, rel) = resolve(preopens, &path)?;
            A::remove_directory_at(dir, rel).map(|()| FsOutput::Done)
        }
        FsRequest::Unlink { path } => {
            let (dir, rel) = resolve(preopens, &path)?;
            A::unlink_file_at(dir, rel).map(|()| FsOutput::Done)
        }
        FsRequest::Rename { from, to } => {
            let (dir, rel) = resolve(preopens, &from)?;
            let (new_dir, new_rel) = resolve(preopens, &to)?;
            A::rename_at(dir, rel, new_dir, new_rel).map(|()| FsOutput::Done)
        }
        FsRequest::Link { existing, link } => {
            let (dir, rel) = resolve(preopens, &existing)?;
            let (new_dir, new_rel) = resolve(preopens, &link)?;
            A::link_at(dir, rel, new_dir, new_rel).map(|()| FsOutput::Done)
        }
        FsRequest::Symlink { target, link, .. } => {
            let (dir, rel) = resolve(preopens, &link)?;
            A::symlink_at(dir, target.text(), rel).map(|()| FsOutput::Done)
        }
        FsRequest::ReadLink { path, mut buffer } => {
            let (dir, rel) = resolve(preopens, &path)?;
            let content = A::readlink_at(dir, rel)?;
            let out = output(&mut buffer);
            let bytes = content.as_bytes();
            if bytes.len() > out.len() {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
            out[..bytes.len()].copy_from_slice(bytes);
            Ok(FsOutput::Bytes(bytes.len()))
        }
        FsRequest::Access { path, .. } => {
            let (dir, rel) = resolve(preopens, &path)?;
            A::stat_at(dir, rel, true).map(|_| FsOutput::Done)
        }
        FsRequest::SetTimes {
            target,
            accessed,
            modified,
        } => match target {
            FsTarget::Path {
                path,
                follow_symlinks,
            } => {
                let (dir, rel) = resolve(preopens, &path)?;
                A::set_times_at(dir, rel, follow_symlinks, accessed, modified)
                    .map(|()| FsOutput::Done)
            }
            FsTarget::File(_) => {
                A::set_times(descriptor(object).ok_or_else(bad)?, accessed, modified)
                    .map(|()| FsOutput::Done)
            }
        },
        FsRequest::CopyFile {
            from,
            to,
            exclusive,
        } => {
            let (dir, rel) = resolve(preopens, &from)?;
            let source = A::open_at(dir, rel, &FileOptions::default(), false)?;
            let (dir, rel) = resolve(preopens, &to)?;
            let target = A::open_at(
                dir,
                rel,
                &FileOptions {
                    read: false,
                    write: true,
                    create: true,
                    exclusive,
                    truncate: !exclusive,
                    ..FileOptions::default()
                },
                false,
            )?;
            let mut chunk = [0u8; 16 * 1024];
            let mut offset = 0;
            loop {
                let (n, eof) = A::read(&source, &mut chunk, offset)?;
                let mut written = 0;
                while written < n {
                    written += A::write(&target, &chunk[written..n], offset + written as u64)?;
                }
                offset += n as u64;
                if n == 0 || eof {
                    return Ok(FsOutput::Done);
                }
            }
        }
        FsRequest::RealPath { .. } | FsRequest::Chmod { .. } | FsRequest::Chown { .. } => {
            Err(Error::new(ErrorKind::Unsupported))
        }
    }
}
fn descriptor<A: Api>(object: Option<&mut Option<Object<A>>>) -> Option<&A::Descriptor> {
    match object? {
        Some(Object::File { descriptor, .. } | Object::Dir { descriptor, .. }) => Some(descriptor),
        _ => None,
    }
}
