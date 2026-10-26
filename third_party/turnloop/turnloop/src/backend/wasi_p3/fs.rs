//! `wasi:filesystem@0.3` binding surface for the shared WASI filesystem core.
//!
//! Experimental, like the rest of the 0.3 backend. Every import is lowered by hand
//! with its canonical layout (wasip3 0.8.0), and each raw call preserves the
//! caller's shadow-stack context (see `abi::preserving_stack`): generated bindings
//! trap on the pinned toolchain in debug builds once a call re-enters the guest.
//! A request blocks the agent on a private waitable set until its subtask, stream
//! or future completes. File bytes move directly between host streams and caller
//! memory; paths are borrowed. Only strings the host returns (directory entry and
//! link names, rare error text) are owned allocations.
use super::{
    abi::preserving_stack,
    wait_set::{WaitSet, subtask_drop},
};
use crate::{
    Result,
    backend::wasi_fs::{Api, error},
    fs::{FileMetadata, FileOptions, FileTime, FileType, TimeChange},
};
use std::{
    alloc::{Layout, dealloc},
    ptr,
};
use wasip3::{
    filesystem::types::{Descriptor, DirectoryEntry, ErrorCode},
    wit_future::FuturePayload,
    wit_stream::StreamPayload,
};

type Done = std::result::Result<(), ErrorCode>;
/// Canonical stream/future operation still pending.
const BLOCKED: u32 = u32::MAX;
const MAX_STREAM: usize = (1 << 28) - 1;
/// wasi-libc errno for each `error-code` discriminant; `other` (36) reads as EIO.
const ERRNO: [i32; 36] = [
    2, 7, 8, 10, 16, 19, 20, 22, 25, 26, 27, 28, 29, 31, 32, 34, 35, 37, 43, 44, 46, 48, 51, 54,
    55, 56, 58, 59, 60, 61, 63, 64, 69, 70, 74, 75,
];

#[link(wasm_import_module = "wasi:filesystem/types@0.3.0")]
unsafe extern "C" {
    #[link_name = "[async-lower][method]descriptor.open-at"]
    fn raw_open_at(params: *mut u8, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.stat"]
    fn raw_stat(handle: i32, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.stat-at"]
    fn raw_stat_at(handle: i32, flags: i32, path: *const u8, len: usize, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.sync"]
    fn raw_sync(handle: i32, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.sync-data"]
    fn raw_sync_data(handle: i32, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.set-size"]
    fn raw_set_size(handle: i32, size: i64, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.set-times"]
    fn raw_set_times(params: *mut u8, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.set-times-at"]
    fn raw_set_times_at(params: *mut u8, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.create-directory-at"]
    fn raw_create_directory_at(handle: i32, path: *const u8, len: usize, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.remove-directory-at"]
    fn raw_remove_directory_at(handle: i32, path: *const u8, len: usize, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.unlink-file-at"]
    fn raw_unlink_file_at(handle: i32, path: *const u8, len: usize, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.readlink-at"]
    fn raw_readlink_at(handle: i32, path: *const u8, len: usize, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.rename-at"]
    fn raw_rename_at(params: *mut u8, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.link-at"]
    fn raw_link_at(params: *mut u8, results: *mut u8) -> i32;
    #[link_name = "[async-lower][method]descriptor.symlink-at"]
    fn raw_symlink_at(params: *mut u8, results: *mut u8) -> i32;
    #[link_name = "[method]descriptor.read-via-stream"]
    fn raw_read_via_stream(handle: i32, offset: i64, results: *mut u8);
    #[link_name = "[method]descriptor.write-via-stream"]
    fn raw_write_via_stream(handle: i32, data: i32, offset: i64) -> i32;
    #[link_name = "[method]descriptor.append-via-stream"]
    fn raw_append_via_stream(handle: i32, data: i32) -> i32;
    #[link_name = "[method]descriptor.read-directory"]
    fn raw_read_directory(handle: i32, results: *mut u8);
}
#[link(wasm_import_module = "wasi:filesystem/preopens@0.3.0")]
unsafe extern "C" {
    #[link_name = "get-directories"]
    fn raw_get_directories(results: *mut u8);
}

thread_local! {
    // One private set: a request joins exactly one waitable, so every event is its own.
    static WAIT: WaitSet = WaitSet::new();
}
/// Block until the waitable's pending operation reports its packed result.
fn complete(handle: u32, code: u32) -> u32 {
    if code != BLOCKED {
        return code;
    }
    WAIT.with(|set| {
        set.join(handle);
        let code = loop {
            let (_, waitable, code) = set.step(true);
            if waitable == handle {
                break code;
            }
        };
        set.remove(handle);
        code
    })
}
/// Run an async-lowered import and block until it returns, then drop its subtask.
fn call(f: impl FnOnce() -> i32) {
    let packed = preserving_stack(f) as u32;
    let task = packed >> 4;
    if task == 0 {
        return;
    }
    if packed & 15 < 2 {
        WAIT.with(|set| {
            set.join(task);
            while !matches!(set.step(true), (_, waitable, code) if waitable == task && code >= 2) {}
            set.remove(task);
        });
    }
    // SAFETY: the subtask returned and left the private set.
    unsafe { subtask_drop(task) };
}

fn byte(area: *const u8, offset: usize) -> u8 {
    // SAFETY: callers pass an initialized canonical area covering `offset`.
    unsafe { area.add(offset).read() }
}
fn word(area: *const u8, offset: usize) -> u32 {
    // SAFETY: callers pass an initialized canonical area covering the word.
    unsafe { area.add(offset).cast::<u32>().read_unaligned() }
}
fn wide(area: *const u8, offset: usize) -> u64 {
    // SAFETY: callers pass an initialized canonical area covering the value.
    unsafe { area.add(offset).cast::<u64>().read_unaligned() }
}
fn put(area: *mut u8, offset: usize, bytes: &[u8]) {
    // SAFETY: callers pass writable parameter storage covering the value.
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), area.add(offset), bytes.len()) }
}
fn put_str(area: *mut u8, offset: usize, text: &str) {
    put(area, offset, &(text.as_ptr() as u32).to_le_bytes());
    put(area, offset + 4, &(text.len() as u32).to_le_bytes());
}
/// Free a canonical string the host allocated through this guest's allocator.
fn free_string(pointer: u32, len: u32) {
    if len != 0 {
        // SAFETY: the host lowered this owned list through cabi_realloc with align 1.
        unsafe {
            dealloc(
                pointer as *mut u8,
                Layout::from_size_align_unchecked(len as usize, 1),
            )
        };
    }
}
fn take_string(pointer: u32, len: u32) -> String {
    if len == 0 {
        return String::new();
    }
    // SAFETY: an owned, host-validated UTF-8 list allocated by the Rust heap.
    unsafe { String::from_raw_parts(pointer as *mut u8, len as usize, len as usize) }
}
/// Decode an error-code at `offset`, releasing `other` text.
fn error_at(area: *const u8, offset: usize) -> crate::Error {
    let code = byte(area, offset) as usize;
    if code >= ERRNO.len() {
        if byte(area, offset + 4) == 1 {
            free_string(word(area, offset + 8), word(area, offset + 12));
        }
        return error(29);
    }
    error(ERRNO[code])
}
fn unit(area: &[u32; 5]) -> Result<()> {
    let area = area.as_ptr().cast::<u8>();
    if byte(area, 0) == 0 {
        Ok(())
    } else {
        Err(error_at(area, 4))
    }
}
fn kind(tag: u8) -> FileType {
    match tag {
        0 => FileType::BlockDevice,
        1 => FileType::CharDevice,
        2 => FileType::Directory,
        3 => FileType::Fifo,
        4 => FileType::Symlink,
        5 => FileType::File,
        6 => FileType::Socket,
        _ => FileType::Unknown,
    }
}
fn stat_result(area: &[u64; 14]) -> Result<FileMetadata> {
    let area = area.as_ptr().cast::<u8>();
    if byte(area, 0) != 0 {
        return Err(error_at(area, 8));
    }
    let tag = byte(area, 8);
    if tag == 7 && byte(area, 12) == 1 {
        free_string(word(area, 16), word(area, 20));
    }
    let time = |at: usize| {
        (byte(area, at) == 1).then(|| FileTime {
            seconds: wide(area, at + 8) as i64,
            nanoseconds: word(area, at + 16),
        })
    };
    Ok(FileMetadata {
        kind: kind(tag),
        size: wide(area, 32),
        mode: None,
        device: None,
        inode: None,
        links: Some(wide(area, 24)),
        uid: None,
        gid: None,
        rdev: None,
        block_size: None,
        blocks: None,
        accessed: time(40),
        modified: time(64),
        changed: time(88),
        created: None,
    })
}
fn timestamp(area: *mut u8, offset: usize, change: TimeChange) {
    match change {
        TimeChange::Keep => put(area, offset, &[0]),
        TimeChange::Now => put(area, offset, &[1]),
        TimeChange::At(t) => {
            put(area, offset, &[2]);
            put(area, offset + 8, &t.seconds.to_le_bytes());
            put(area, offset + 16, &t.nanoseconds.to_le_bytes());
        }
    }
}
/// Wait for a byte stream's `future<result<_, error-code>>`, then drop it.
fn finish(future: u32) -> Result<()> {
    let mut area = [0u32; 5];
    let vtable = <Done as FuturePayload>::VTABLE;
    // SAFETY: owned future handle and a pinned 20-byte result area.
    let code =
        preserving_stack(|| unsafe { (vtable.start_read)(future, area.as_mut_ptr().cast()) });
    complete(future, code);
    // SAFETY: the value was delivered; the readable end is no longer needed.
    preserving_stack(|| unsafe { (vtable.drop_readable)(future) });
    unit(&area)
}
fn abandon(future: u32) {
    // SAFETY: owned, unread future handle.
    preserving_stack(|| unsafe { (<Done as FuturePayload>::VTABLE.drop_readable)(future) });
}
/// Write all bytes into a new stream consumed by `start`, returning the count written.
fn write_stream(bytes: &[u8], start: impl FnOnce(i32) -> i32) -> Result<usize> {
    let vtable = <u8 as StreamPayload>::VTABLE;
    // SAFETY: stream.new creates a fresh owned (readable, writable) pair.
    let pair = preserving_stack(|| unsafe { (vtable.new)() });
    let (readable, writable) = (pair as u32, (pair >> 32) as u32);
    let future = preserving_stack(|| start(readable as i32)) as u32;
    let mut written = 0;
    while written < bytes.len() {
        let rest = &bytes[written..];
        // SAFETY: owned writable end and initialized bytes borrowed until completion.
        let code = preserving_stack(|| unsafe {
            (vtable.start_write)(writable, rest.as_ptr(), rest.len().min(MAX_STREAM))
        });
        let code = complete(writable, code);
        written += (code >> 4) as usize;
        if code & 15 != 0 {
            break;
        }
    }
    // SAFETY: closing the owned writable end tells the host the data is complete.
    preserving_stack(|| unsafe { (vtable.drop_writable)(writable) });
    finish(future).map(|()| written)
}

pub(in crate::backend) struct P3;
pub(in crate::backend) struct Entries {
    stream: u32,
    future: Option<u32>,
}
impl Drop for Entries {
    fn drop(&mut self) {
        // SAFETY: owned directory stream handle with no pending read.
        preserving_stack(|| unsafe {
            (<DirectoryEntry as StreamPayload>::VTABLE.drop_readable)(self.stream)
        });
        if let Some(future) = self.future.take() {
            abandon(future);
        }
    }
}

impl Api for P3 {
    type Descriptor = Descriptor;
    type Appender = ();
    type Entries = Entries;
    fn preopens() -> Vec<(Descriptor, String)> {
        let mut area = [0u32; 2];
        // SAFETY: aligned result area for list<tuple<descriptor, string>>.
        preserving_stack(|| unsafe { raw_get_directories(area.as_mut_ptr().cast()) });
        let (list, count) = (area[0] as *const u8, area[1] as usize);
        let preopens = (0..count)
            .map(|i| {
                let at = i * 12;
                // SAFETY: each element transfers one owned descriptor handle.
                let descriptor = unsafe { Descriptor::from_handle(word(list, at)) };
                (
                    descriptor,
                    take_string(word(list, at + 4), word(list, at + 8)),
                )
            })
            .collect();
        if count != 0 {
            // SAFETY: the host allocated the element array with this layout.
            unsafe {
                dealloc(
                    list.cast_mut(),
                    Layout::from_size_align_unchecked(count * 12, 4),
                )
            };
        }
        preopens
    }
    fn open_at(
        dir: &Descriptor,
        path: &str,
        o: &FileOptions,
        directory: bool,
    ) -> Result<Descriptor> {
        let open = u8::from(o.create || o.exclusive)
            | (u8::from(directory) << 1)
            | (u8::from(o.exclusive) << 2)
            | (u8::from(o.truncate) << 3);
        let access = u8::from(o.read)
            | (u8::from(o.write || o.append) << 1)
            | (u8::from(o.sync) << 2)
            | (u8::from(o.data_sync) << 3);
        if access == 0 {
            return Err(crate::Error::new(crate::ErrorKind::InvalidInput));
        }
        let mut params = [0u32; 5];
        let p = params.as_mut_ptr().cast::<u8>();
        put(p, 0, &dir.handle().to_le_bytes());
        put(p, 4, &[u8::from(o.follow_symlinks)]);
        put_str(p, 8, path);
        put(p, 16, &[open, access]);
        let mut results = [0u32; 5];
        // SAFETY: pinned parameters (borrowing `path`) and results outlive the subtask.
        call(|| unsafe { raw_open_at(p, results.as_mut_ptr().cast()) });
        let r = results.as_ptr().cast::<u8>();
        if byte(r, 0) != 0 {
            return Err(error_at(r, 4));
        }
        // SAFETY: success transferred one owned descriptor handle.
        Ok(unsafe { Descriptor::from_handle(word(r, 4)) })
    }
    fn read(file: &Descriptor, output: &mut [u8], offset: u64) -> Result<(usize, bool)> {
        let mut handles = [0u32; 2];
        // SAFETY: owned result handles are written to the pinned area.
        preserving_stack(|| unsafe {
            raw_read_via_stream(
                file.handle() as i32,
                offset as i64,
                handles.as_mut_ptr().cast(),
            )
        });
        let (stream, future) = (handles[0], handles[1]);
        let vtable = <u8 as StreamPayload>::VTABLE;
        let (mut filled, mut eof) = (0, false);
        while filled < output.len() {
            let rest = &mut output[filled..];
            // SAFETY: owned readable end; the exclusive output region outlives completion.
            let code = preserving_stack(|| unsafe {
                (vtable.start_read)(stream, rest.as_mut_ptr(), rest.len().min(MAX_STREAM))
            });
            let code = complete(stream, code);
            filled += (code >> 4) as usize;
            if code & 15 != 0 {
                eof = true;
                break;
            }
        }
        // SAFETY: owned readable end with no pending operation.
        preserving_stack(|| unsafe { (vtable.drop_readable)(stream) });
        if eof {
            finish(future)?;
        } else {
            abandon(future);
        }
        Ok((filled, eof))
    }
    fn write(file: &Descriptor, bytes: &[u8], offset: u64) -> Result<usize> {
        write_stream(bytes, |readable| {
            // SAFETY: transfers the owned readable end; returns an owned future handle.
            unsafe { raw_write_via_stream(file.handle() as i32, readable, offset as i64) }
        })
    }
    fn append(file: &Descriptor, _: &mut Option<()>, bytes: &[u8]) -> Result<usize> {
        write_stream(bytes, |readable| {
            // SAFETY: transfers the owned readable end; returns an owned future handle.
            unsafe { raw_append_via_stream(file.handle() as i32, readable) }
        })
    }
    fn stat(file: &Descriptor) -> Result<FileMetadata> {
        let mut results = [0u64; 14];
        // SAFETY: pinned 112-byte result area outlives the subtask.
        call(|| unsafe { raw_stat(file.handle() as i32, results.as_mut_ptr().cast()) });
        stat_result(&results)
    }
    fn stat_at(dir: &Descriptor, path: &str, follow: bool) -> Result<FileMetadata> {
        let mut results = [0u64; 14];
        // SAFETY: borrowed path and pinned result area outlive the subtask.
        call(|| unsafe {
            raw_stat_at(
                dir.handle() as i32,
                i32::from(follow),
                path.as_ptr(),
                path.len(),
                results.as_mut_ptr().cast(),
            )
        });
        stat_result(&results)
    }
    fn sync(file: &Descriptor, data_only: bool) -> Result<()> {
        let mut results = [0u32; 5];
        // SAFETY: pinned result area outlives the subtask.
        call(|| unsafe {
            if data_only {
                raw_sync_data(file.handle() as i32, results.as_mut_ptr().cast())
            } else {
                raw_sync(file.handle() as i32, results.as_mut_ptr().cast())
            }
        });
        unit(&results)
    }
    fn set_size(file: &Descriptor, size: u64) -> Result<()> {
        let mut results = [0u32; 5];
        // SAFETY: pinned result area outlives the subtask.
        call(|| unsafe {
            raw_set_size(
                file.handle() as i32,
                size as i64,
                results.as_mut_ptr().cast(),
            )
        });
        unit(&results)
    }
    fn set_times(file: &Descriptor, accessed: TimeChange, modified: TimeChange) -> Result<()> {
        let mut params = [0u64; 7];
        let p = params.as_mut_ptr().cast::<u8>();
        put(p, 0, &file.handle().to_le_bytes());
        timestamp(p, 8, accessed);
        timestamp(p, 32, modified);
        let mut results = [0u32; 5];
        // SAFETY: pinned parameters and results outlive the subtask.
        call(|| unsafe { raw_set_times(p, results.as_mut_ptr().cast()) });
        unit(&results)
    }
    fn set_times_at(
        dir: &Descriptor,
        path: &str,
        follow: bool,
        accessed: TimeChange,
        modified: TimeChange,
    ) -> Result<()> {
        let mut params = [0u64; 8];
        let p = params.as_mut_ptr().cast::<u8>();
        put(p, 0, &dir.handle().to_le_bytes());
        put(p, 4, &[u8::from(follow)]);
        put_str(p, 8, path);
        timestamp(p, 16, accessed);
        timestamp(p, 40, modified);
        let mut results = [0u32; 5];
        // SAFETY: pinned parameters (borrowing `path`) and results outlive the subtask.
        call(|| unsafe { raw_set_times_at(p, results.as_mut_ptr().cast()) });
        unit(&results)
    }
    fn entries(dir: &Descriptor) -> Result<Entries> {
        let mut handles = [0u32; 2];
        // SAFETY: owned result handles are written to the pinned area.
        preserving_stack(|| unsafe {
            raw_read_directory(dir.handle() as i32, handles.as_mut_ptr().cast())
        });
        Ok(Entries {
            stream: handles[0],
            future: Some(handles[1]),
        })
    }
    fn next_entry(entries: &mut Entries) -> Result<Option<(FileType, String)>> {
        let vtable = <DirectoryEntry as StreamPayload>::VTABLE;
        loop {
            let Some(future) = entries.future else {
                return Ok(None);
            };
            let mut element = [0u32; 6];
            // SAFETY: owned readable end and one pinned 24-byte element slot.
            let code = preserving_stack(|| unsafe {
                (vtable.start_read)(entries.stream, element.as_mut_ptr().cast(), 1)
            });
            let code = complete(entries.stream, code);
            if code >> 4 == 1 {
                let e = element.as_ptr().cast::<u8>();
                let tag = byte(e, 0);
                if tag == 7 && byte(e, 4) == 1 {
                    free_string(word(e, 8), word(e, 12));
                }
                return Ok(Some((kind(tag), take_string(word(e, 16), word(e, 20)))));
            }
            if code & 15 != 0 {
                entries.future = None;
                finish(future)?;
                return Ok(None);
            }
        }
    }
    fn create_directory_at(dir: &Descriptor, path: &str) -> Result<()> {
        let mut results = [0u32; 5];
        // SAFETY: borrowed path and pinned result area outlive the subtask.
        call(|| unsafe {
            raw_create_directory_at(
                dir.handle() as i32,
                path.as_ptr(),
                path.len(),
                results.as_mut_ptr().cast(),
            )
        });
        unit(&results)
    }
    fn remove_directory_at(dir: &Descriptor, path: &str) -> Result<()> {
        let mut results = [0u32; 5];
        // SAFETY: borrowed path and pinned result area outlive the subtask.
        call(|| unsafe {
            raw_remove_directory_at(
                dir.handle() as i32,
                path.as_ptr(),
                path.len(),
                results.as_mut_ptr().cast(),
            )
        });
        unit(&results)
    }
    fn unlink_file_at(dir: &Descriptor, path: &str) -> Result<()> {
        let mut results = [0u32; 5];
        // SAFETY: borrowed path and pinned result area outlive the subtask.
        call(|| unsafe {
            raw_unlink_file_at(
                dir.handle() as i32,
                path.as_ptr(),
                path.len(),
                results.as_mut_ptr().cast(),
            )
        });
        unit(&results)
    }
    fn rename_at(dir: &Descriptor, path: &str, new_dir: &Descriptor, new_path: &str) -> Result<()> {
        let mut params = [0u32; 6];
        let p = params.as_mut_ptr().cast::<u8>();
        put(p, 0, &dir.handle().to_le_bytes());
        put_str(p, 4, path);
        put(p, 12, &new_dir.handle().to_le_bytes());
        put_str(p, 16, new_path);
        let mut results = [0u32; 5];
        // SAFETY: pinned parameters (borrowing both paths) and results outlive the subtask.
        call(|| unsafe { raw_rename_at(p, results.as_mut_ptr().cast()) });
        unit(&results)
    }
    fn link_at(dir: &Descriptor, path: &str, new_dir: &Descriptor, new_path: &str) -> Result<()> {
        let mut params = [0u32; 7];
        let p = params.as_mut_ptr().cast::<u8>();
        put(p, 0, &dir.handle().to_le_bytes());
        put(p, 4, &[0]);
        put_str(p, 8, path);
        put(p, 16, &new_dir.handle().to_le_bytes());
        put_str(p, 20, new_path);
        let mut results = [0u32; 5];
        // SAFETY: pinned parameters (borrowing both paths) and results outlive the subtask.
        call(|| unsafe { raw_link_at(p, results.as_mut_ptr().cast()) });
        unit(&results)
    }
    fn symlink_at(dir: &Descriptor, target: &str, path: &str) -> Result<()> {
        let mut params = [0u32; 5];
        let p = params.as_mut_ptr().cast::<u8>();
        put(p, 0, &dir.handle().to_le_bytes());
        put_str(p, 4, target);
        put_str(p, 12, path);
        let mut results = [0u32; 5];
        // SAFETY: pinned parameters (borrowing both strings) and results outlive the subtask.
        call(|| unsafe { raw_symlink_at(p, results.as_mut_ptr().cast()) });
        unit(&results)
    }
    fn readlink_at(dir: &Descriptor, path: &str) -> Result<String> {
        let mut results = [0u32; 5];
        // SAFETY: borrowed path and pinned result area outlive the subtask.
        call(|| unsafe {
            raw_readlink_at(
                dir.handle() as i32,
                path.as_ptr(),
                path.len(),
                results.as_mut_ptr().cast(),
            )
        });
        let r = results.as_ptr().cast::<u8>();
        if byte(r, 0) != 0 {
            return Err(error_at(r, 4));
        }
        Ok(take_string(word(r, 4), word(r, 8)))
    }
}
