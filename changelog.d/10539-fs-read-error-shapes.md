**Buffer-mode `fs` reads throw on failure, and read-stream errors carry Node's
`code`/`errno`/`syscall`/`path`** (#10452, #10451).

`fs.readFileSync(path)`, `readFileSync(path, {})`, `fs.promises.readFile(path)`
and `import { readFile } from "node:fs/promises"` returned or resolved
`null`/`undefined` for a missing file. `try { cfg = readFileSync(optional) }
catch { defaults }` took the wrong branch, and `.catch(e => e.code === "ENOENT")`
never ran. The `'utf8'` and callback forms did report the error.

Every `readFile` form reads through `read_file_bytes_with_options`, which folded
each failure into `None`. The Buffer entry points
(`js_fs_read_file_binary{,_options}`) turned that into a null `BufferHeader`, so
the value was `null` or `undefined` depending on the call shape. The string
entry point re-read the file to get an `io::Error` back and always reported it
as `open`. The reader now returns the OS error and the syscall that failed, and
every form reports it the way Node does: ENOENT or EACCES as `open '<path>'`,
and a directory as `EISDIR: illegal operation on a directory, read` with no
path. Before, a directory read returned a Buffer or reported `open`. The
callback form now reads once. Its old `stat` pre-check missed directories, so a
directory gave `(null, undefined)` in Buffer mode and a synchronous throw in
string mode. `FileHandle.readFile()` now rejects when the read fails.

`fs.createReadStream` open and read failures emitted a plain `Error` with only
the Rust message (`No such file or directory (os error 2)`) and no own
properties. The write side already stored a Node-shaped `error_value` (#9493).
The read side now does too. The constructor's open failure is held as the OS
error until the stream state is registered (the registry is the GC root), then
turned into the error value. The pump's read failures are stored the same way.
`fs.writeFile(dest, failingReadStream)` also rejects with that error now. The
stream error helpers moved to `fs/stream/stream_errors.rs` so `stream.rs` stays
under the 2000-line cap.

fs error messages used Rust's `Display` text. They now use libuv's description
of the errno (`ENOENT: no such file or directory, open '<path>'`), as Node
does, for every error built by `build_fs_error_value*`.

Validation: `test_gap_10452_fs_read_error_shapes` covers sync, callback and
promise reads (with and without an encoding), `FileHandle.readFile`, and read
and write streams, for a missing file and a directory, plus successful-read
controls. It matches Node byte for byte and fails on the parent commit. EACCES
was checked by hand as an unprivileged user. New `fs::errors` unit tests cover
the libuv text and the open-vs-read failure shape. User-space instructions for
50k `readFileSync` calls on a small file: Buffer form −0.6 %, UTF-8 form −9.5 %
(it no longer decodes the path twice per call).

Consuming a failed read stream through `fs.promises.writeFile` reported the
missing fd (`EBADF: bad file descriptor, read`) rather than the failure the
constructor stored; the consumer now returns the stored Node-shaped value
before it tries to read.

On Windows `io::Error::raw_os_error()` is a Win32 error code, not an errno, so
keying `code`/`errno`/message on it reported an `errno` of -2 where Node reports
libuv's -4058, with Rust's message text. `win32_error_to_uv` ports libuv's
`uv_translate_sys_error` (`src/win/error.c`, v1.52.1) for the filesystem arms and
`UV_WINDOWS_ERRNOS` holds libuv's Windows error numbers and messages;
`io_error_code`/`io_error_errno` consult them under `cfg(windows)`. The mapping
is pure and stays compiled under `cfg(test)`, so its unit tests run on every
host — Windows itself was not run.
