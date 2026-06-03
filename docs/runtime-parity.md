# Node.js + Bun Runtime API Parity Reference

A leaf-level inventory of the public API surface of Node.js and Bun. Every function, class, method, property, event, and constant is listed as a separate row so it can be used as a gap-analysis baseline against any TypeScript runtime.

This document is **not Perry-specific** — it does not say which APIs Perry implements. It says which APIs Node and Bun ship, so a reader can compare against Perry (or any other implementation) externally. Perry-specific implementation status is reconciled in `docs/runtime-parity-gaps.md`.

## Versions referenced

- **Node.js** — 22 LTS / 24 current (some 26-current APIs marked in Notes).
- **Bun** — 1.3.x. Bun's compatibility status is sourced from <https://bun.sh/docs/runtime/nodejs-apis> and the per-API Bun docs at <https://bun.sh/docs/api/>.

## Legend

| Symbol | Meaning |
|--------|---------|
| `✓` | Implemented and broadly spec-compatible |
| `⚠` | Partial — stub, divergent behavior, missing options, or status uncertain |
| `✗` | Not implemented (no compatible API) |

## Sources

Each module page links to the authoritative Node.js docs (`https://nodejs.org/api/<module>.html`) and Bun's compatibility table. Where Bun's docs are silent on a specific newer-Node API, Notes flag it as "status uncertain."

## Structure

The reference is split into three parts in this single file:

### Part 1 — Node core modules (batch 1)

`node:fs` · `node:fs/promises` · `node:path` (incl. `posix` / `win32`) · `node:http` · `node:https` · `node:http2` · `node:net` · `node:tls` · `node:dgram` · `node:dns` (incl. `dns/promises`) · `node:crypto` · `node:stream` (incl. `stream/promises` / `stream/web` / `stream/consumers`) · `node:buffer`

### Part 2 — Node core modules (batch 2)

`node:events` · `node:util` · `node:os` · `node:process` (and global `process`) · `node:child_process` · `node:cluster` · `node:worker_threads` · `node:zlib` · `node:querystring` · `node:url` · `node:vm` · `node:async_hooks` · `node:perf_hooks` · `node:timers` (incl. `timers/promises`) · `node:tty` · `node:v8` · `node:assert` (incl. `assert/strict`) · `node:console` · `node:module` · `node:string_decoder` · `node:readline` (incl. `readline/promises`) · `node:repl` · `node:trace_events` · `node:inspector` (incl. `inspector/promises`) · `node:diagnostics_channel` · `node:wasi` · `node:test` (incl. `test/reporters` / `test/mock`) · `node:sqlite` · `node:punycode` (deprecated) · `node:domain` (deprecated) · `node:sys` (alias)

### Part 3 — Web / global APIs and Bun-only APIs

**Web / Global APIs** (implemented by both runtimes): timers, microtasks, `structuredClone`, `atob` / `btoa`, `fetch`, `URL` / `URLSearchParams`, WHATWG Fetch (`Headers` / `Request` / `Response` / `Blob` / `File` / `FormData`), Streams (`ReadableStream` / `WritableStream` / `TransformStream` + readers / controllers / queuing strategies / `CompressionStream` / `DecompressionStream`), `TextEncoder` / `TextDecoder` (+ stream variants), DOM events (`Event` / `EventTarget` / `CustomEvent` / `MessageEvent` / `ErrorEvent` / `CloseEvent`), `MessageChannel` / `MessagePort` / `BroadcastChannel`, `WebSocket`, `AbortController` / `AbortSignal`, WebCrypto (`crypto.getRandomValues`, `crypto.randomUUID`, full `SubtleCrypto`, `CryptoKey`), `performance` + `PerformanceObserver`, `console`.

**Bun-only APIs**: `Bun.serve` + `Server` + `ServerWebSocket`, `Bun.file` / `Bun.write` / `FileSink`, `Bun.spawn` / `spawnSync` / `Subprocess`, `Bun.listen` / `Bun.connect` / `Bun.udpSocket`, `Bun.password` / `Bun.hash` / `Bun.CryptoHasher`, `Bun.dns`, `Bun.Glob`, `Bun.semver`, `Bun.color`, `Bun.s3`, `Bun.SQL`, `Bun.RedisClient`, `bun:sqlite`, `bun:ffi`, `bun:test`, `bun:jsc`, `Bun.FileSystemRouter`, `HTMLRewriter`, `Bun.$` (Shell), `Bun.Cookie` / `Bun.CookieMap`, `Bun.Transpiler`, `Bun.build` / `Bun.plugin`, `import.meta.*` extensions, misc utilities.

---

## Node.js + Bun Runtime API Parity Inventory — Part 1

Bun column reflects [bun.sh/docs/runtime/nodejs-apis](https://bun.sh/docs/runtime/nodejs-apis) (Node v23 baseline). Marks: ✓ supported, ✗ not implemented, ⚠ partial / documented gap.

---

### node:fs

#### Functions (Callback API)
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `fs.access(path[, mode], callback)` | ✓ | ✓ |  |
| `fs.appendFile(path, data[, options], callback)` | ✓ | ✓ |  |
| `fs.chmod(path, mode, callback)` | ✓ | ✓ |  |
| `fs.chown(path, uid, gid, callback)` | ✓ | ✓ |  |
| `fs.close(fd[, callback])` | ✓ | ✓ |  |
| `fs.copyFile(src, dest[, mode], callback)` | ✓ | ✓ |  |
| `fs.cp(src, dest[, options], callback)` | ✓ | ✓ |  |
| `fs.createReadStream(path[, options])` | ✓ | ✓ |  |
| `fs.createWriteStream(path[, options])` | ✓ | ✓ |  |
| `fs.exists(path, callback)` | ✓ | ✓ | Deprecated — use `fs.access` |
| `fs.fchmod(fd, mode, callback)` | ✓ | ✓ |  |
| `fs.fchown(fd, uid, gid, callback)` | ✓ | ✓ |  |
| `fs.fdatasync(fd, callback)` | ✓ | ✓ |  |
| `fs.fstat(fd[, options], callback)` | ✓ | ✓ |  |
| `fs.fsync(fd, callback)` | ✓ | ✓ |  |
| `fs.ftruncate(fd[, len], callback)` | ✓ | ✓ |  |
| `fs.futimes(fd, atime, mtime, callback)` | ✓ | ✓ |  |
| `fs.glob(pattern[, options], callback)` | ✓ | ✓ |  |
| `fs.lchmod(path, mode, callback)` | ✓ | ✓ |  |
| `fs.lchown(path, uid, gid, callback)` | ✓ | ✓ |  |
| `fs.link(existingPath, newPath, callback)` | ✓ | ✓ |  |
| `fs.lstat(path[, options], callback)` | ✓ | ✓ |  |
| `fs.lutimes(path, atime, mtime, callback)` | ✓ | ✓ |  |
| `fs.mkdir(path[, options], callback)` | ✓ | ✓ |  |
| `fs.mkdtemp(prefix[, options], callback)` | ✓ | ✓ |  |
| `fs.open(path[, flags[, mode]], callback)` | ✓ | ✓ |  |
| `fs.openAsBlob(path[, options])` | ✓ | ✓ |  |
| `fs.opendir(path[, options], callback)` | ✓ | ✓ |  |
| `fs.read(fd, buffer, offset, length, position, callback)` | ✓ | ✓ |  |
| `fs.read(fd[, options], callback)` | ✓ | ✓ |  |
| `fs.read(fd, buffer[, options], callback)` | ✓ | ✓ |  |
| `fs.readdir(path[, options], callback)` | ✓ | ✓ |  |
| `fs.readFile(path[, options], callback)` | ✓ | ✓ |  |
| `fs.readlink(path[, options], callback)` | ✓ | ✓ |  |
| `fs.readv(fd, buffers[, position], callback)` | ✓ | ✓ |  |
| `fs.realpath(path[, options], callback)` | ✓ | ✓ |  |
| `fs.realpath.native(path[, options], callback)` | ✓ | ✓ |  |
| `fs.rename(oldPath, newPath, callback)` | ✓ | ✓ |  |
| `fs.rm(path[, options], callback)` | ✓ | ✓ |  |
| `fs.rmdir(path[, options], callback)` | ✓ | ✓ |  |
| `fs.stat(path[, options], callback)` | ✓ | ✓ |  |
| `fs.statfs(path[, options], callback)` | ✓ | ✓ |  |
| `fs.symlink(target, path[, type], callback)` | ✓ | ✓ |  |
| `fs.truncate(path[, len], callback)` | ✓ | ✓ |  |
| `fs.unlink(path, callback)` | ✓ | ✓ |  |
| `fs.unwatchFile(filename[, listener])` | ✓ | ✓ |  |
| `fs.utimes(path, atime, mtime, callback)` | ✓ | ✓ |  |
| `fs.watch(filename[, options][, listener])` | ✓ | ✓ |  |
| `fs.watchFile(filename[, options], listener)` | ✓ | ✓ |  |
| `fs.write(fd, buffer, offset[, length[, position]], callback)` | ✓ | ✓ |  |
| `fs.write(fd, buffer[, options], callback)` | ✓ | ✓ |  |
| `fs.write(fd, string[, position[, encoding]], callback)` | ✓ | ✓ |  |
| `fs.writeFile(file, data[, options], callback)` | ✓ | ✓ |  |
| `fs.writev(fd, buffers[, position], callback)` | ✓ | ✓ |  |

#### Functions (Sync API)
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `fs.accessSync(path[, mode])` | ✓ | ✓ |  |
| `fs.appendFileSync(path, data[, options])` | ✓ | ✓ |  |
| `fs.chmodSync(path, mode)` | ✓ | ✓ |  |
| `fs.chownSync(path, uid, gid)` | ✓ | ✓ |  |
| `fs.closeSync(fd)` | ✓ | ✓ |  |
| `fs.copyFileSync(src, dest[, mode])` | ✓ | ✓ |  |
| `fs.cpSync(src, dest[, options])` | ✓ | ✓ |  |
| `fs.existsSync(path)` | ✓ | ✓ |  |
| `fs.fchmodSync(fd, mode)` | ✓ | ✓ |  |
| `fs.fchownSync(fd, uid, gid)` | ✓ | ✓ |  |
| `fs.fdatasyncSync(fd)` | ✓ | ✓ |  |
| `fs.fstatSync(fd[, options])` | ✓ | ✓ |  |
| `fs.fsyncSync(fd)` | ✓ | ✓ |  |
| `fs.ftruncateSync(fd[, len])` | ✓ | ✓ |  |
| `fs.futimesSync(fd, atime, mtime)` | ✓ | ✓ |  |
| `fs.globSync(pattern[, options])` | ✓ | ✓ |  |
| `fs.lchmodSync(path, mode)` | ✓ | ✓ |  |
| `fs.lchownSync(path, uid, gid)` | ✓ | ✓ |  |
| `fs.linkSync(existingPath, newPath)` | ✓ | ✓ |  |
| `fs.lstatSync(path[, options])` | ✓ | ✓ |  |
| `fs.lutimesSync(path, atime, mtime)` | ✓ | ✓ |  |
| `fs.mkdirSync(path[, options])` | ✓ | ✓ |  |
| `fs.mkdtempSync(prefix[, options])` | ✓ | ✓ |  |
| `fs.mkdtempDisposableSync(prefix[, options])` | ✓ | ⚠ | Newer Node v22+ Disposable API |
| `fs.opendirSync(path[, options])` | ✓ | ✓ |  |
| `fs.openSync(path[, flags[, mode]])` | ✓ | ✓ |  |
| `fs.readdirSync(path[, options])` | ✓ | ✓ |  |
| `fs.readFileSync(path[, options])` | ✓ | ✓ |  |
| `fs.readlinkSync(path[, options])` | ✓ | ✓ |  |
| `fs.readSync(fd, buffer, offset, length[, position])` | ✓ | ✓ |  |
| `fs.readSync(fd, buffer[, options])` | ✓ | ✓ |  |
| `fs.readvSync(fd, buffers[, position])` | ✓ | ✓ |  |
| `fs.realpathSync(path[, options])` | ✓ | ✓ |  |
| `fs.realpathSync.native(path[, options])` | ✓ | ✓ |  |
| `fs.renameSync(oldPath, newPath)` | ✓ | ✓ |  |
| `fs.rmdirSync(path[, options])` | ✓ | ✓ |  |
| `fs.rmSync(path[, options])` | ✓ | ✓ |  |
| `fs.statSync(path[, options])` | ✓ | ✓ |  |
| `fs.statfsSync(path[, options])` | ✓ | ✓ |  |
| `fs.symlinkSync(target, path[, type])` | ✓ | ✓ |  |
| `fs.truncateSync(path[, len])` | ✓ | ✓ |  |
| `fs.unlinkSync(path)` | ✓ | ✓ |  |
| `fs.utimesSync(path, atime, mtime)` | ✓ | ✓ |  |
| `fs.writeFileSync(file, data[, options])` | ✓ | ✓ |  |
| `fs.writeSync(fd, buffer, offset[, length[, position]])` | ✓ | ✓ |  |
| `fs.writeSync(fd, buffer[, options])` | ✓ | ✓ |  |
| `fs.writeSync(fd, string[, position[, encoding]])` | ✓ | ✓ |  |
| `fs.writevSync(fd, buffers[, position])` | ✓ | ✓ |  |

#### Exported Helpers And Aliases
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `fs.FileReadStream` | ✓ | ✓ | Alias of `fs.ReadStream` |
| `fs.FileWriteStream` | ✓ | ✓ | Alias of `fs.WriteStream` |
| `fs._toUnixTimestamp(value)` | ✓ | ⚠ | Exported helper; function name is `toUnixTimestamp` |

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class fs.Dir` | ✓ | ✓ |  |
| `dir.path` | ✓ | ✓ |  |
| `dir.close()` (Promise) | ✓ | ✓ |  |
| `dir.close(callback)` | ✓ | ✓ |  |
| `dir.closeSync()` | ✓ | ✓ |  |
| `dir.read()` (Promise) | ✓ | ✓ |  |
| `dir.read(callback)` | ✓ | ✓ |  |
| `dir.readSync()` | ✓ | ✓ |  |
| `dir[Symbol.asyncIterator]()` | ✓ | ✓ |  |
| `dir[Symbol.asyncDispose]()` | ✓ | ✓ |  |
| `dir[Symbol.dispose]()` | ✓ | ✓ |  |
| `class fs.Dirent` | ✓ | ✓ |  |
| `dirent.isBlockDevice()` | ✓ | ✓ |  |
| `dirent.isCharacterDevice()` | ✓ | ✓ |  |
| `dirent.isDirectory()` | ✓ | ✓ |  |
| `dirent.isFIFO()` | ✓ | ✓ |  |
| `dirent.isFile()` | ✓ | ✓ |  |
| `dirent.isSocket()` | ✓ | ✓ |  |
| `dirent.isSymbolicLink()` | ✓ | ✓ |  |
| `dirent.name` | ✓ | ✓ |  |
| `dirent.parentPath` | ✓ | ✓ |  |
| `class fs.FSWatcher` | ✓ | ✓ |  |
| `watcher.close()` | ✓ | ✓ |  |
| `watcher.ref()` | ✓ | ✓ |  |
| `watcher.unref()` | ✓ | ✓ |  |
| `class fs.StatWatcher` | ✓ | ✓ |  |
| `statWatcher.ref()` | ✓ | ✓ |  |
| `statWatcher.unref()` | ✓ | ✓ |  |
| `class fs.ReadStream` | ✓ | ✓ |  |
| `readStream.bytesRead` | ✓ | ✓ |  |
| `readStream.path` | ✓ | ✓ |  |
| `readStream.pending` | ✓ | ✓ |  |
| `class fs.WriteStream` | ✓ | ✓ |  |
| `writeStream.bytesWritten` | ✓ | ✓ |  |
| `writeStream.path` | ✓ | ✓ |  |
| `writeStream.pending` | ✓ | ✓ |  |
| `writeStream.close([callback])` | ✓ | ✓ |  |
| `class fs.Stats` | ✓ | ✓ |  |
| `stats.isBlockDevice()` | ✓ | ✓ |  |
| `stats.isCharacterDevice()` | ✓ | ✓ |  |
| `stats.isDirectory()` | ✓ | ✓ |  |
| `stats.isFIFO()` | ✓ | ✓ |  |
| `stats.isFile()` | ✓ | ✓ |  |
| `stats.isSocket()` | ✓ | ✓ |  |
| `stats.isSymbolicLink()` | ✓ | ✓ |  |
| `stats.dev` | ✓ | ✓ |  |
| `stats.ino` | ✓ | ✓ |  |
| `stats.mode` | ✓ | ✓ |  |
| `stats.nlink` | ✓ | ✓ |  |
| `stats.uid` | ✓ | ✓ |  |
| `stats.gid` | ✓ | ✓ |  |
| `stats.rdev` | ✓ | ✓ |  |
| `stats.size` | ✓ | ✓ |  |
| `stats.blksize` | ✓ | ✓ |  |
| `stats.blocks` | ✓ | ✓ |  |
| `stats.atimeMs` / `stats.atime` / `stats.atimeNs` | ✓ | ✓ |  |
| `stats.mtimeMs` / `stats.mtime` / `stats.mtimeNs` | ✓ | ✓ |  |
| `stats.ctimeMs` / `stats.ctime` / `stats.ctimeNs` | ✓ | ✓ |  |
| `stats.birthtimeMs` / `stats.birthtime` / `stats.birthtimeNs` | ✓ | ✓ |  |
| `class fs.StatFs` | ✓ | ✓ |  |
| `statfs.bavail` / `bfree` / `blocks` / `bsize` / `frsize` / `ffree` / `files` / `type` | ✓ | ✓ |  |
| `class fs.Utf8Stream` | ✓ | ✗ | New v26.1.0 — not yet in Bun |
| `utf8Stream.write/end/destroy/flush/flushSync/reopen/[Symbol.dispose]` | ✓ | ✗ |  |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| FSWatcher | `'change'` | ✓ | ✓ |  |
| FSWatcher | `'close'` | ✓ | ✓ |  |
| FSWatcher | `'error'` | ✓ | ✓ |  |
| ReadStream | `'close'` | ✓ | ✓ |  |
| ReadStream | `'open'` | ✓ | ✓ |  |
| ReadStream | `'ready'` | ✓ | ✓ |  |
| WriteStream | `'close'` | ✓ | ✓ |  |
| WriteStream | `'open'` | ✓ | ✓ |  |
| WriteStream | `'ready'` | ✓ | ✓ |  |

#### Constants
| Constant | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `fs.constants.F_OK` | ✓ | ✓ |  |
| `fs.constants.R_OK` | ✓ | ✓ |  |
| `fs.constants.W_OK` | ✓ | ✓ |  |
| `fs.constants.X_OK` | ✓ | ✓ |  |
| `fs.constants.O_RDONLY` | ✓ | ✓ |  |
| `fs.constants.O_WRONLY` | ✓ | ✓ |  |
| `fs.constants.O_RDWR` | ✓ | ✓ |  |
| `fs.constants.O_CREAT` | ✓ | ✓ |  |
| `fs.constants.O_EXCL` | ✓ | ✓ |  |
| `fs.constants.O_TRUNC` | ✓ | ✓ |  |
| `fs.constants.O_APPEND` | ✓ | ✓ |  |
| `fs.constants.O_DIRECTORY` | ✓ | ✓ |  |
| `fs.constants.O_NOATIME` | ✓ | ✓ | Linux only |
| `fs.constants.O_NOFOLLOW` | ✓ | ✓ |  |
| `fs.constants.O_SYNC` | ✓ | ✓ |  |
| `fs.constants.O_DSYNC` | ✓ | ✓ |  |
| `fs.constants.O_SYMLINK` | ✓ | ✓ |  |
| `fs.constants.O_DIRECT` | ✓ | ✓ |  |
| `fs.constants.O_NONBLOCK` | ✓ | ✓ |  |
| `fs.constants.S_IFMT` | ✓ | ✓ |  |
| `fs.constants.S_IFREG` | ✓ | ✓ |  |
| `fs.constants.S_IFDIR` | ✓ | ✓ |  |
| `fs.constants.S_IFCHR` | ✓ | ✓ |  |
| `fs.constants.S_IFBLK` | ✓ | ✓ |  |
| `fs.constants.S_IFIFO` | ✓ | ✓ |  |
| `fs.constants.S_IFLNK` | ✓ | ✓ |  |
| `fs.constants.S_IFSOCK` | ✓ | ✓ |  |
| `fs.constants.S_IRWXU` / `S_IRUSR` / `S_IWUSR` / `S_IXUSR` | ✓ | ✓ |  |
| `fs.constants.S_IRWXG` / `S_IRGRP` / `S_IWGRP` / `S_IXGRP` | ✓ | ✓ |  |
| `fs.constants.S_IRWXO` / `S_IROTH` / `S_IWOTH` / `S_IXOTH` | ✓ | ✓ |  |
| `fs.constants.COPYFILE_EXCL` | ✓ | ✓ |  |
| `fs.constants.COPYFILE_FICLONE` | ✓ | ✓ |  |
| `fs.constants.COPYFILE_FICLONE_FORCE` | ✓ | ✓ |  |

---

### node:fs/promises

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `fsPromises.access(path[, mode])` | ✓ | ✓ |  |
| `fsPromises.appendFile(path, data[, options])` | ✓ | ✓ |  |
| `fsPromises.chmod(path, mode)` | ✓ | ✓ |  |
| `fsPromises.chown(path, uid, gid)` | ✓ | ✓ |  |
| `fsPromises.copyFile(src, dest[, mode])` | ✓ | ✓ |  |
| `fsPromises.cp(src, dest[, options])` | ✓ | ✓ |  |
| `fsPromises.glob(pattern[, options])` | ✓ | ✓ |  |
| `fsPromises.lchmod(path, mode)` | ✓ | ✓ |  |
| `fsPromises.lchown(path, uid, gid)` | ✓ | ✓ |  |
| `fsPromises.link(existingPath, newPath)` | ✓ | ✓ |  |
| `fsPromises.lstat(path[, options])` | ✓ | ✓ |  |
| `fsPromises.lutimes(path, atime, mtime)` | ✓ | ✓ |  |
| `fsPromises.mkdir(path[, options])` | ✓ | ✓ |  |
| `fsPromises.mkdtemp(prefix[, options])` | ✓ | ✓ |  |
| `fsPromises.mkdtempDisposable(prefix[, options])` | ✓ | ⚠ | Newer Node v22+ Disposable API |
| `fsPromises.open(path, flags[, mode])` | ✓ | ✓ |  |
| `fsPromises.opendir(path[, options])` | ✓ | ✓ |  |
| `fsPromises.readdir(path[, options])` | ✓ | ✓ |  |
| `fsPromises.readFile(path[, options])` | ✓ | ✓ |  |
| `fsPromises.readlink(path[, options])` | ✓ | ✓ |  |
| `fsPromises.realpath(path[, options])` | ✓ | ✓ |  |
| `fsPromises.rename(oldPath, newPath)` | ✓ | ✓ |  |
| `fsPromises.rm(path[, options])` | ✓ | ✓ |  |
| `fsPromises.rmdir(path[, options])` | ✓ | ✓ |  |
| `fsPromises.stat(path[, options])` | ✓ | ✓ |  |
| `fsPromises.statfs(path[, options])` | ✓ | ✓ |  |
| `fsPromises.symlink(target, path[, type])` | ✓ | ✓ |  |
| `fsPromises.truncate(path[, len])` | ✓ | ✓ |  |
| `fsPromises.unlink(path)` | ✓ | ✓ |  |
| `fsPromises.utimes(path, atime, mtime)` | ✓ | ✓ |  |
| `fsPromises.watch(filename[, options])` | ✓ | ✓ | AsyncIterable |
| `fsPromises.writeFile(file, data[, options])` | ✓ | ✓ |  |
| `fsPromises.constants` | ✓ | ✓ |  |

#### FileHandle Class
| Method / Property | Node.js | Bun | Notes |
|-------------------|---------|-----|-------|
| `class FileHandle` | ✓ | ✓ |  |
| `filehandle.fd` | ✓ | ✓ |  |
| `filehandle.appendFile(data[, options])` | ✓ | ✓ |  |
| `filehandle.chmod(mode)` | ✓ | ✓ |  |
| `filehandle.chown(uid, gid)` | ✓ | ✓ |  |
| `filehandle.close()` | ✓ | ✓ |  |
| `filehandle.createReadStream([options])` | ✓ | ✓ |  |
| `filehandle.createWriteStream([options])` | ✓ | ✓ |  |
| `filehandle.datasync()` | ✓ | ✓ |  |
| `filehandle.pull([...transforms][, options])` | ✓ | ⚠ | Experimental in Node; Perry supports no-transform source iteration |
| `filehandle.pullSync([...transforms][, options])` | ✓ | ⚠ | Experimental in Node; Perry supports no-transform source iteration |
| `filehandle.read(buffer, offset, length, position)` | ✓ | ✓ |  |
| `filehandle.read([options])` | ✓ | ✓ |  |
| `filehandle.read(buffer[, options])` | ✓ | ✓ |  |
| `filehandle.readableWebStream([options])` | ✓ | ✓ |  |
| `filehandle.readFile(options)` | ✓ | ✓ |  |
| `filehandle.readLines([options])` | ✓ | ✓ |  |
| `filehandle.readv(buffers[, position])` | ✓ | ✓ |  |
| `filehandle.stat([options])` | ✓ | ✓ |  |
| `filehandle.sync()` | ✓ | ✓ |  |
| `filehandle.truncate(len)` | ✓ | ✓ |  |
| `filehandle.utimes(atime, mtime)` | ✓ | ✓ |  |
| `filehandle.write(buffer, offset[, length[, position]])` | ✓ | ✓ |  |
| `filehandle.write(buffer[, options])` | ✓ | ✓ |  |
| `filehandle.write(string[, position[, encoding]])` | ✓ | ✓ |  |
| `filehandle.writeFile(data, options)` | ✓ | ✓ |  |
| `filehandle.writev(buffers[, position])` | ✓ | ✓ |  |
| `filehandle.writer([options])` | ✓ | ⚠ | Newer API; Perry supports direct FileHandle writes |
| `filehandle[Symbol.asyncDispose]()` | ✓ | ✓ |  |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| FileHandle | `'close'` | ✓ | ✓ |  |

---

### node:path

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `path.basename(path[, suffix])` | ✓ | ✓ |  |
| `path.dirname(path)` | ✓ | ✓ |  |
| `path.extname(path)` | ✓ | ✓ |  |
| `path.format(pathObject)` | ✓ | ✓ |  |
| `path.isAbsolute(path)` | ✓ | ✓ |  |
| `path.join([...paths])` | ✓ | ✓ |  |
| `path.matchesGlob(path, pattern)` | ✓ | ✓ |  |
| `path.normalize(path)` | ✓ | ✓ |  |
| `path.parse(path)` | ✓ | ✓ |  |
| `path.relative(from, to)` | ✓ | ✓ |  |
| `path.resolve([...paths])` | ✓ | ✓ |  |
| `path.toNamespacedPath(path)` | ✓ | ✓ |  |

#### Properties
| Property | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `path.delimiter` | ✓ | ✓ | `:` POSIX, `;` Win32 |
| `path.sep` | ✓ | ✓ | `/` POSIX, `\` Win32 |
| `path.posix` | ✓ | ✓ | POSIX implementation namespace |
| `path.win32` | ✓ | ✓ | Win32 implementation namespace |

#### node:path/posix
Available via `require('node:path/posix')` or `path.posix`. Exposes the same surface (all functions + `delimiter`, `sep`) with POSIX semantics regardless of host OS. Node.js ✓ / Bun ✓.

#### node:path/win32
Available via `require('node:path/win32')` or `path.win32`. Exposes the same surface with Windows semantics regardless of host OS. Node.js ✓ / Bun ✓.

---

### node:http

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `http.createServer([options][, requestListener])` | ✓ | ✓ |  |
| `http.get(options[, callback])` | ✓ | ✓ |  |
| `http.get(url[, options][, callback])` | ✓ | ✓ |  |
| `http.request(options[, callback])` | ✓ | ⚠ | Bun: outgoing client request body buffered (not streamed) |
| `http.request(url[, options][, callback])` | ✓ | ⚠ | See above |
| `http.validateHeaderName(name[, label])` | ✓ | ✓ |  |
| `http.validateHeaderValue(name, value)` | ✓ | ✓ |  |
| `http.setMaxIdleHTTPParsers(max)` | ✓ | ✓ |  |
| `http.setGlobalProxyFromEnv([proxyEnv])` | ✓ | ⚠ | Newer Node API |

#### Module Properties
| Property | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `http.METHODS` | ✓ | ✓ |  |
| `http.STATUS_CODES` | ✓ | ✓ |  |
| `http.globalAgent` | ✓ | ✓ |  |
| `http.maxHeaderSize` | ✓ | ✓ |  |

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class http.Agent` | ✓ | ✓ |  |
| `new Agent([options])` | ✓ | ✓ |  |
| `agent.createConnection(options[, callback])` | ✓ | ✓ |  |
| `agent.keepSocketAlive(socket)` | ✓ | ✓ |  |
| `agent.reuseSocket(socket, request)` | ✓ | ✓ |  |
| `agent.destroy()` | ✓ | ✓ |  |
| `agent.getName([options])` | ✓ | ✓ |  |
| `agent.freeSockets` | ✓ | ✓ |  |
| `agent.maxFreeSockets` | ✓ | ✓ |  |
| `agent.maxSockets` | ✓ | ✓ |  |
| `agent.maxTotalSockets` | ✓ | ✓ |  |
| `agent.requests` | ✓ | ✓ |  |
| `agent.sockets` | ✓ | ✓ |  |
| `class http.ClientRequest` | ✓ | ✓ |  |
| `request.abort()` | ✓ | ✓ | Deprecated |
| `request.cork()` | ✓ | ✓ |  |
| `request.end([data[, encoding]][, callback])` | ✓ | ✓ |  |
| `request.destroy([error])` | ✓ | ✓ |  |
| `request.flushHeaders()` | ✓ | ✓ |  |
| `request.getHeader(name)` | ✓ | ✓ |  |
| `request.getHeaderNames()` | ✓ | ✓ |  |
| `request.getHeaders()` | ✓ | ✓ |  |
| `request.getRawHeaderNames()` | ✓ | ✓ |  |
| `request.hasHeader(name)` | ✓ | ✓ |  |
| `request.removeHeader(name)` | ✓ | ✓ |  |
| `request.setHeader(name, value)` | ✓ | ✓ |  |
| `request.setNoDelay([noDelay])` | ✓ | ✓ |  |
| `request.setSocketKeepAlive([enable][, initialDelay])` | ✓ | ✓ |  |
| `request.setTimeout(timeout[, callback])` | ✓ | ✓ |  |
| `request.uncork()` | ✓ | ✓ |  |
| `request.write(chunk[, encoding][, callback])` | ✓ | ⚠ | Body buffered, not streamed in Bun |
| `request.aborted` | ✓ | ✓ | Deprecated |
| `request.connection` | ✓ | ✓ | Deprecated |
| `request.destroyed` | ✓ | ✓ |  |
| `request.finished` | ✓ | ✓ | Deprecated |
| `request.host` | ✓ | ✓ |  |
| `request.maxHeadersCount` | ✓ | ✓ |  |
| `request.method` | ✓ | ✓ |  |
| `request.path` | ✓ | ✓ |  |
| `request.protocol` | ✓ | ✓ |  |
| `request.reusedSocket` | ✓ | ✓ |  |
| `request.socket` | ✓ | ✓ |  |
| `request.writableEnded` | ✓ | ✓ |  |
| `request.writableFinished` | ✓ | ✓ |  |
| `class http.Server` | ✓ | ✓ |  |
| `server.close([callback])` | ✓ | ✓ |  |
| `server.closeAllConnections()` | ✓ | ✓ |  |
| `server.closeIdleConnections()` | ✓ | ✓ |  |
| `server.listen()` | ✓ | ✓ |  |
| `server.setTimeout([msecs][, callback])` | ✓ | ✓ |  |
| `server[Symbol.asyncDispose]()` | ✓ | ✓ |  |
| `server.headersTimeout` | ✓ | ✓ |  |
| `server.keepAliveTimeout` | ✓ | ✓ |  |
| `server.keepAliveTimeoutBuffer` | ✓ | ✓ |  |
| `server.listening` | ✓ | ✓ |  |
| `server.maxHeadersCount` | ✓ | ✓ |  |
| `server.maxRequestsPerSocket` | ✓ | ✓ |  |
| `server.requestTimeout` | ✓ | ✓ |  |
| `server.timeout` | ✓ | ✓ |  |
| `class http.ServerResponse` | ✓ | ✓ |  |
| `response.addTrailers(headers)` | ✓ | ✓ |  |
| `response.cork()` | ✓ | ✓ |  |
| `response.end([data[, encoding]][, callback])` | ✓ | ✓ |  |
| `response.flushHeaders()` | ✓ | ✓ |  |
| `response.getHeader(name)` | ✓ | ✓ |  |
| `response.getHeaderNames()` | ✓ | ✓ |  |
| `response.getHeaders()` | ✓ | ✓ |  |
| `response.hasHeader(name)` | ✓ | ✓ |  |
| `response.removeHeader(name)` | ✓ | ✓ |  |
| `response.setHeader(name, value)` | ✓ | ✓ |  |
| `response.setTimeout(msecs[, callback])` | ✓ | ✓ |  |
| `response.uncork()` | ✓ | ✓ |  |
| `response.write(chunk[, encoding][, callback])` | ✓ | ✓ |  |
| `response.writeContinue()` | ✓ | ✓ |  |
| `response.writeEarlyHints(hints[, callback])` | ✓ | ✓ |  |
| `response.writeHead(statusCode[, statusMessage][, headers])` | ✓ | ✓ |  |
| `response.writeProcessing()` | ✓ | ✓ |  |
| `response.connection` | ✓ | ✓ | Deprecated |
| `response.finished` | ✓ | ✓ | Deprecated |
| `response.headersSent` | ✓ | ✓ |  |
| `response.req` | ✓ | ✓ |  |
| `response.sendDate` | ✓ | ✓ |  |
| `response.socket` | ✓ | ✓ |  |
| `response.statusCode` | ✓ | ✓ |  |
| `response.statusMessage` | ✓ | ✓ |  |
| `response.strictContentLength` | ✓ | ✓ |  |
| `response.writableEnded` | ✓ | ✓ |  |
| `response.writableFinished` | ✓ | ✓ |  |
| `class http.IncomingMessage` | ✓ | ✓ |  |
| `message.destroy([error])` | ✓ | ✓ |  |
| `message.setTimeout(msecs[, callback])` | ✓ | ✓ |  |
| `message.aborted` | ✓ | ✓ |  |
| `message.complete` | ✓ | ✓ |  |
| `message.connection` | ✓ | ✓ |  |
| `message.headers` | ✓ | ✓ |  |
| `message.headersDistinct` | ✓ | ✓ |  |
| `message.httpVersion` | ✓ | ✓ |  |
| `message.method` | ✓ | ✓ |  |
| `message.rawHeaders` | ✓ | ✓ |  |
| `message.rawTrailers` | ✓ | ✓ |  |
| `message.signal` | ✓ | ✓ |  |
| `message.socket` | ✓ | ✓ |  |
| `message.statusCode` | ✓ | ✓ |  |
| `message.statusMessage` | ✓ | ✓ |  |
| `message.trailers` | ✓ | ✓ |  |
| `message.trailersDistinct` | ✓ | ✓ |  |
| `message.url` | ✓ | ✓ |  |
| `class http.OutgoingMessage` | ✓ | ✓ |  |
| `outgoingMessage.addTrailers(headers)` | ✓ | ✓ |  |
| `outgoingMessage.appendHeader(name, value)` | ✓ | ✓ |  |
| `outgoingMessage.cork()` | ✓ | ✓ |  |
| `outgoingMessage.destroy([error])` | ✓ | ✓ |  |
| `outgoingMessage.end(chunk[, encoding][, callback])` | ✓ | ✓ |  |
| `outgoingMessage.flushHeaders()` | ✓ | ✓ |  |
| `outgoingMessage.getHeader(name)` | ✓ | ✓ |  |
| `outgoingMessage.getHeaderNames()` | ✓ | ✓ |  |
| `outgoingMessage.getHeaders()` | ✓ | ✓ |  |
| `outgoingMessage.hasHeader(name)` | ✓ | ✓ |  |
| `outgoingMessage.pipe()` | ✓ | ✓ |  |
| `outgoingMessage.removeHeader(name)` | ✓ | ✓ |  |
| `outgoingMessage.setHeader(name, value)` | ✓ | ✓ |  |
| `outgoingMessage.setHeaders(headers)` | ✓ | ✓ |  |
| `outgoingMessage.setTimeout(msecs[, callback])` | ✓ | ✓ |  |
| `outgoingMessage.uncork()` | ✓ | ✓ |  |
| `outgoingMessage.write(chunk[, encoding][, callback])` | ✓ | ✓ |  |
| `outgoingMessage.connection` | ✓ | ✓ |  |
| `outgoingMessage.headersSent` | ✓ | ✓ |  |
| `outgoingMessage.socket` | ✓ | ✓ |  |
| `outgoingMessage.writableCorked` | ✓ | ✓ |  |
| `outgoingMessage.writableEnded` | ✓ | ✓ |  |
| `outgoingMessage.writableFinished` | ✓ | ✓ |  |
| `outgoingMessage.writableHighWaterMark` | ✓ | ✓ |  |
| `outgoingMessage.writableLength` | ✓ | ✓ |  |
| `outgoingMessage.writableObjectMode` | ✓ | ✓ |  |
| `class http.WebSocket` | ✓ | ✓ | Recent addition |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| ClientRequest | `'abort'` | ✓ | ✓ | Deprecated |
| ClientRequest | `'close'` | ✓ | ✓ |  |
| ClientRequest | `'connect'` | ✓ | ✓ |  |
| ClientRequest | `'continue'` | ✓ | ✓ |  |
| ClientRequest | `'finish'` | ✓ | ✓ |  |
| ClientRequest | `'information'` | ✓ | ✓ |  |
| ClientRequest | `'response'` | ✓ | ✓ |  |
| ClientRequest | `'socket'` | ✓ | ✓ |  |
| ClientRequest | `'timeout'` | ✓ | ✓ |  |
| ClientRequest | `'upgrade'` | ✓ | ✓ |  |
| Server | `'checkContinue'` | ✓ | ✓ |  |
| Server | `'checkExpectation'` | ✓ | ✓ |  |
| Server | `'clientError'` | ✓ | ✓ |  |
| Server | `'close'` | ✓ | ✓ |  |
| Server | `'connect'` | ✓ | ✓ |  |
| Server | `'connection'` | ✓ | ✓ |  |
| Server | `'dropRequest'` | ✓ | ✓ |  |
| Server | `'request'` | ✓ | ✓ |  |
| Server | `'upgrade'` | ✓ | ✓ |  |
| ServerResponse | `'close'` | ✓ | ✓ |  |
| ServerResponse | `'finish'` | ✓ | ✓ |  |
| IncomingMessage | `'aborted'` | ✓ | ✓ |  |
| IncomingMessage | `'close'` | ✓ | ✓ |  |
| OutgoingMessage | `'drain'` | ✓ | ✓ |  |
| OutgoingMessage | `'finish'` | ✓ | ✓ |  |
| OutgoingMessage | `'prefinish'` | ✓ | ✓ |  |

---

### node:https

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `https.createServer([options][, requestListener])` | ✓ | ✓ |  |
| `https.get(options[, callback])` | ✓ | ✓ |  |
| `https.get(url[, options][, callback])` | ✓ | ✓ |  |
| `https.request(options[, callback])` | ✓ | ✓ |  |
| `https.request(url[, options][, callback])` | ✓ | ✓ |  |

#### Module Properties
| Property | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `https.globalAgent` | ✓ | ⚠ | Agent not always used per Bun docs |

#### Classes
| Class / Method / Property / Event | Node.js | Bun | Notes |
|-----------------------------------|---------|-----|-------|
| `class https.Agent` | ✓ | ⚠ | Implemented but not always used |
| `new https.Agent([options])` | ✓ | ⚠ |  |
| `agent.createConnection(options[, callback])` | ✓ | ⚠ |  |
| `agent.keepSocketAlive(socket)` | ✓ | ⚠ |  |
| `agent.reuseSocket(socket, request)` | ✓ | ⚠ |  |
| `agent.destroy()` | ✓ | ⚠ |  |
| `agent.getName([options])` | ✓ | ⚠ |  |
| `class https.Server` (extends `tls.Server`) | ✓ | ✓ |  |
| `server.close([callback])` | ✓ | ✓ |  |
| `server.closeAllConnections()` | ✓ | ✓ |  |
| `server.closeIdleConnections()` | ✓ | ✓ |  |
| `server.listen()` | ✓ | ✓ |  |
| `server.setTimeout([msecs][, callback])` | ✓ | ✓ |  |
| `server.headersTimeout` | ✓ | ✓ |  |
| `server.maxHeadersCount` | ✓ | ✓ |  |
| `server.requestTimeout` | ✓ | ✓ |  |
| `server.timeout` | ✓ | ✓ |  |
| `server.keepAliveTimeout` | ✓ | ✓ |  |
| `server.keepAliveTimeoutBuffer` | ✓ | ✓ |  |
| `server[Symbol.asyncDispose]()` | ✓ | ✓ |  |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| Agent | `'keylog'` | ✓ | ⚠ |  |

---

### node:http2

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `http2.createServer([options][, onRequestHandler])` | ✓ | ⚠ | Missing `options.allowHTTP1` |
| `http2.createSecureServer(options[, onRequestHandler])` | ✓ | ⚠ | Missing `options.allowHTTP1`, `options.enableConnectProtocol` |
| `http2.connect(authority[, options][, listener])` | ✓ | ✓ |  |
| `http2.getDefaultSettings()` | ✓ | ✓ |  |
| `http2.getPackedSettings([settings])` | ✓ | ✓ |  |
| `http2.getUnpackedSettings(buf)` | ✓ | ✓ |  |
| `http2.performServerHandshake(socket[, options])` | ✓ | ⚠ |  |

#### Module Properties
| Property | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `http2.constants` | ✓ | ✓ |  |
| `http2.sensitiveHeaders` (Symbol) | ✓ | ✓ |  |

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class Http2Session` | ✓ | ✓ |  |
| `session.alpnProtocol` | ✓ | ✓ |  |
| `session.close([callback])` | ✓ | ✓ |  |
| `session.closed` | ✓ | ✓ |  |
| `session.connecting` | ✓ | ✓ |  |
| `session.destroy([error][, code])` | ✓ | ✓ |  |
| `session.destroyed` | ✓ | ✓ |  |
| `session.encrypted` | ✓ | ✓ |  |
| `session.goaway([code[, lastStreamID[, opaqueData]]])` | ✓ | ✓ |  |
| `session.localSettings` | ✓ | ✓ |  |
| `session.originSet` | ✓ | ✓ |  |
| `session.pendingSettingsAck` | ✓ | ✓ |  |
| `session.ping([payload, ]callback)` | ✓ | ✓ |  |
| `session.ref()` | ✓ | ✓ |  |
| `session.remoteSettings` | ✓ | ✓ |  |
| `session.setLocalWindowSize(windowSize)` | ✓ | ✓ |  |
| `session.setTimeout(msecs, callback)` | ✓ | ✓ |  |
| `session.socket` | ✓ | ✓ |  |
| `session.state` | ✓ | ✓ |  |
| `session.settings([settings][, callback])` | ✓ | ✓ |  |
| `session.type` | ✓ | ✓ |  |
| `session.unref()` | ✓ | ✓ |  |
| `class ServerHttp2Session` (extends Http2Session) | ✓ | ⚠ | Missing ALTSVC extension |
| `serverSession.altsvc(alt, originOrStream)` | ✓ | ✗ | Not in Bun |
| `serverSession.origin(...origins)` | ✓ | ⚠ |  |
| `class ClientHttp2Session` (extends Http2Session) | ✓ | ✓ |  |
| `clientSession.request(headers[, options])` | ✓ | ✓ |  |
| `class Http2Stream` | ✓ | ✓ |  |
| `stream.aborted` | ✓ | ✓ |  |
| `stream.bufferSize` | ✓ | ✓ |  |
| `stream.close(code[, callback])` | ✓ | ✓ |  |
| `stream.closed` | ✓ | ✓ |  |
| `stream.destroyed` | ✓ | ✓ |  |
| `stream.endAfterHeaders` | ✓ | ✓ |  |
| `stream.id` | ✓ | ✓ |  |
| `stream.pending` | ✓ | ✓ |  |
| `stream.priority(options)` | ✓ | ✓ | Deprecated |
| `stream.rstCode` | ✓ | ✓ |  |
| `stream.sentHeaders` | ✓ | ✓ |  |
| `stream.sentInfoHeaders` | ✓ | ✓ |  |
| `stream.sentTrailers` | ✓ | ✓ |  |
| `stream.session` | ✓ | ✓ |  |
| `stream.setTimeout(msecs, callback)` | ✓ | ✓ |  |
| `stream.state` | ✓ | ✓ |  |
| `stream.sendTrailers(headers)` | ✓ | ✓ |  |
| `class ClientHttp2Stream` (extends Http2Stream) | ✓ | ✓ |  |
| `class ServerHttp2Stream` (extends Http2Stream) | ✓ | ✓ |  |
| `serverStream.additionalHeaders(headers)` | ✓ | ✓ |  |
| `serverStream.headersSent` | ✓ | ✓ |  |
| `serverStream.pushAllowed` | ✓ | ✓ |  |
| `serverStream.pushStream(headers[, options], callback)` | ✓ | ✗ | Not in Bun |
| `serverStream.respond([headers[, options]])` | ✓ | ✓ |  |
| `serverStream.respondWithFD(fd[, headers[, options]])` | ✓ | ✓ |  |
| `serverStream.respondWithFile(path[, headers[, options]])` | ✓ | ✓ |  |
| `class Http2Server` | ✓ | ✓ |  |
| `http2Server.close([callback])` | ✓ | ✓ |  |
| `http2Server[Symbol.asyncDispose]()` | ✓ | ✓ |  |
| `http2Server.setTimeout([msecs][, callback])` | ✓ | ✓ |  |
| `http2Server.timeout` | ✓ | ✓ |  |
| `http2Server.updateSettings([settings])` | ✓ | ✓ |  |
| `class Http2SecureServer` (extends Http2Server) | ✓ | ✓ |  |
| `class Http2ServerRequest` | ✓ | ✓ |  |
| `request.aborted` | ✓ | ✓ |  |
| `request.authority` | ✓ | ✓ |  |
| `request.complete` | ✓ | ✓ |  |
| `request.connection` | ✓ | ✓ |  |
| `request.destroy([error])` | ✓ | ✓ |  |
| `request.headers` | ✓ | ✓ |  |
| `request.httpVersion` | ✓ | ✓ |  |
| `request.method` | ✓ | ✓ |  |
| `request.rawHeaders` | ✓ | ✓ |  |
| `request.rawTrailers` | ✓ | ✓ |  |
| `request.scheme` | ✓ | ✓ |  |
| `request.setTimeout(msecs, callback)` | ✓ | ✓ |  |
| `request.socket` | ✓ | ✓ |  |
| `request.stream` | ✓ | ✓ |  |
| `request.trailers` | ✓ | ✓ |  |
| `request.url` | ✓ | ✓ |  |
| `class Http2ServerResponse` | ✓ | ✓ |  |
| `response.addTrailers(headers)` | ✓ | ✓ |  |
| `response.appendHeader(name, value)` | ✓ | ✓ |  |
| `response.connection` | ✓ | ✓ |  |
| `response.createPushResponse(headers, callback)` | ✓ | ✗ | Not in Bun |
| `response.end([data[, encoding]][, callback])` | ✓ | ✓ |  |
| `response.finished` | ✓ | ✓ |  |
| `response.getHeader(name)` | ✓ | ✓ |  |
| `response.getHeaderNames()` | ✓ | ✓ |  |
| `response.getHeaders()` | ✓ | ✓ |  |
| `response.hasHeader(name)` | ✓ | ✓ |  |
| `response.headersSent` | ✓ | ✓ |  |
| `response.removeHeader(name)` | ✓ | ✓ |  |
| `response.req` | ✓ | ✓ |  |
| `response.sendDate` | ✓ | ✓ |  |
| `response.setHeader(name, value)` | ✓ | ✓ |  |
| `response.setTimeout(msecs[, callback])` | ✓ | ✓ |  |
| `response.socket` | ✓ | ✓ |  |
| `response.statusCode` | ✓ | ✓ |  |
| `response.statusMessage` | ✓ | ✓ |  |
| `response.stream` | ✓ | ✓ |  |
| `response.writableEnded` | ✓ | ✓ |  |
| `response.write(chunk[, encoding][, callback])` | ✓ | ✓ |  |
| `response.writeContinue()` | ✓ | ✓ |  |
| `response.writeEarlyHints(hints)` | ✓ | ✓ |  |
| `response.writeHead(statusCode[, statusMessage][, headers])` | ✓ | ✓ |  |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| Http2Session | `'close'` | ✓ | ✓ |  |
| Http2Session | `'connect'` | ✓ | ✓ |  |
| Http2Session | `'error'` | ✓ | ✓ |  |
| Http2Session | `'frameError'` | ✓ | ✓ |  |
| Http2Session | `'goaway'` | ✓ | ✓ |  |
| Http2Session | `'localSettings'` | ✓ | ✓ |  |
| Http2Session | `'ping'` | ✓ | ✓ |  |
| Http2Session | `'remoteSettings'` | ✓ | ✓ |  |
| Http2Session | `'stream'` | ✓ | ✓ |  |
| Http2Session | `'timeout'` | ✓ | ✓ |  |
| ClientHttp2Session | `'altsvc'` | ✓ | ✗ | Bun missing ALTSVC |
| ClientHttp2Session | `'origin'` | ✓ | ⚠ |  |
| Http2Stream | `'aborted'` | ✓ | ✓ |  |
| Http2Stream | `'close'` | ✓ | ✓ |  |
| Http2Stream | `'error'` | ✓ | ✓ |  |
| Http2Stream | `'frameError'` | ✓ | ✓ |  |
| Http2Stream | `'ready'` | ✓ | ✓ |  |
| Http2Stream | `'timeout'` | ✓ | ✓ |  |
| Http2Stream | `'trailers'` | ✓ | ✓ |  |
| Http2Stream | `'wantTrailers'` | ✓ | ✓ |  |
| ClientHttp2Stream | `'continue'` | ✓ | ✓ |  |
| ClientHttp2Stream | `'headers'` | ✓ | ✓ |  |
| ClientHttp2Stream | `'push'` | ✓ | ✗ |  |
| ClientHttp2Stream | `'response'` | ✓ | ✓ |  |
| Http2Server | `'checkContinue'` | ✓ | ✓ |  |
| Http2Server | `'connection'` | ✓ | ✓ |  |
| Http2Server | `'request'` | ✓ | ✓ |  |
| Http2Server | `'session'` | ✓ | ✓ |  |
| Http2Server | `'sessionError'` | ✓ | ✓ |  |
| Http2Server | `'stream'` | ✓ | ✓ |  |
| Http2Server | `'timeout'` | ✓ | ✓ |  |
| Http2SecureServer | `'unknownProtocol'` | ✓ | ✓ |  |
| Http2ServerRequest | `'aborted'` | ✓ | ✓ |  |
| Http2ServerRequest | `'close'` | ✓ | ✓ |  |
| Http2ServerResponse | `'close'` | ✓ | ✓ |  |
| Http2ServerResponse | `'finish'` | ✓ | ✓ |  |

---

### node:net

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `net.connect(options[, connectListener])` | ✓ | ✓ |  |
| `net.connect(path[, connectListener])` | ✓ | ✓ |  |
| `net.connect(port[, host][, connectListener])` | ✓ | ✓ |  |
| `net.createConnection(options[, connectListener])` | ✓ | ✓ |  |
| `net.createConnection(path[, connectListener])` | ✓ | ✓ |  |
| `net.createConnection(port[, host][, connectListener])` | ✓ | ✓ |  |
| `net.createServer([options][, connectionListener])` | ✓ | ✓ |  |
| `net.isIP(input)` | ✓ | ✓ |  |
| `net.isIPv4(input)` | ✓ | ✓ |  |
| `net.isIPv6(input)` | ✓ | ✓ |  |
| `net.getDefaultAutoSelectFamily()` | ✓ | ✓ |  |
| `net.setDefaultAutoSelectFamily(value)` | ✓ | ✓ |  |
| `net.getDefaultAutoSelectFamilyAttemptTimeout()` | ✓ | ✓ |  |
| `net.setDefaultAutoSelectFamilyAttemptTimeout(value)` | ✓ | ✓ |  |

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class net.BlockList` | ✓ | ✓ |  |
| `new net.BlockList()` | ✓ | ✓ |  |
| `blockList.addAddress(address[, type])` | ✓ | ✓ |  |
| `blockList.addRange(start, end[, type])` | ✓ | ✓ |  |
| `blockList.addSubnet(net, prefix[, type])` | ✓ | ✓ |  |
| `blockList.check(address[, type])` | ✓ | ✓ |  |
| `blockList.fromJSON(value)` | ✓ | ✓ |  |
| `blockList.toJSON()` | ✓ | ✓ |  |
| `blockList.rules` | ✓ | ✓ |  |
| `BlockList.isBlockList(value)` | ✓ | ✓ |  |
| `class net.SocketAddress` | ✓ | ✓ |  |
| `new net.SocketAddress([options])` | ✓ | ✓ |  |
| `socketAddress.address` | ✓ | ✓ |  |
| `socketAddress.family` | ✓ | ✓ |  |
| `socketAddress.flowlabel` | ✓ | ✓ |  |
| `socketAddress.port` | ✓ | ✓ |  |
| `SocketAddress.parse(input)` | ✓ | ✓ |  |
| `class net.Server` | ✓ | ✓ |  |
| `new net.Server([options][, connectionListener])` | ✓ | ✓ |  |
| `server.address()` | ✓ | ✓ |  |
| `server.close([callback])` | ✓ | ✓ |  |
| `server.getConnections(callback)` | ✓ | ✓ |  |
| `server.listen(handle[, backlog][, callback])` | ✓ | ✓ |  |
| `server.listen(options[, callback])` | ✓ | ✓ |  |
| `server.listen(path[, backlog][, callback])` | ✓ | ✓ |  |
| `server.listen([port[, host[, backlog]]][, callback])` | ✓ | ✓ |  |
| `server.ref()` | ✓ | ✓ |  |
| `server.unref()` | ✓ | ✓ |  |
| `server[Symbol.asyncDispose]()` | ✓ | ✓ |  |
| `server.listening` | ✓ | ✓ |  |
| `server.maxConnections` | ✓ | ✓ |  |
| `server.dropMaxConnection` | ✓ | ⚠ |  |
| `class net.Socket` | ✓ | ✓ |  |
| `new net.Socket([options])` | ✓ | ✓ |  |
| `socket.address()` | ✓ | ✓ |  |
| `socket.connect(options[, connectListener])` | ✓ | ✓ |  |
| `socket.connect(path[, connectListener])` | ✓ | ✓ |  |
| `socket.connect(port[, host][, connectListener])` | ✓ | ✓ |  |
| `socket.destroy([error])` | ✓ | ✓ |  |
| `socket.destroySoon()` | ✓ | ✓ |  |
| `socket.end([data[, encoding]][, callback])` | ✓ | ✓ |  |
| `socket.pause()` | ✓ | ✓ |  |
| `socket.ref()` | ✓ | ✓ |  |
| `socket.resetAndDestroy()` | ✓ | ✓ |  |
| `socket.resume()` | ✓ | ✓ |  |
| `socket.setEncoding([encoding])` | ✓ | ✓ |  |
| `socket.setKeepAlive([enable][, initialDelay])` | ✓ | ✓ |  |
| `socket.setNoDelay([noDelay])` | ✓ | ✓ |  |
| `socket.setTimeout(timeout[, callback])` | ✓ | ✓ |  |
| `socket.getTypeOfService()` | ✓ | ⚠ | Newer API |
| `socket.setTypeOfService(tos)` | ✓ | ⚠ | Newer API |
| `socket.unref()` | ✓ | ✓ |  |
| `socket.write(data[, encoding][, callback])` | ✓ | ✓ |  |
| `socket.autoSelectFamilyAttemptedAddresses` | ✓ | ✓ |  |
| `socket.bufferSize` | ✓ | ✓ | Deprecated |
| `socket.bytesRead` | ✓ | ✓ |  |
| `socket.bytesWritten` | ✓ | ✓ |  |
| `socket.connecting` | ✓ | ✓ |  |
| `socket.destroyed` | ✓ | ✓ |  |
| `socket.localAddress` | ✓ | ✓ |  |
| `socket.localPort` | ✓ | ✓ |  |
| `socket.localFamily` | ✓ | ✓ |  |
| `socket.pending` | ✓ | ✓ |  |
| `socket.remoteAddress` | ✓ | ✓ |  |
| `socket.remoteFamily` | ✓ | ✓ |  |
| `socket.remotePort` | ✓ | ✓ |  |
| `socket.readyState` | ✓ | ✓ |  |
| `socket.timeout` | ✓ | ✓ |  |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| Server | `'close'` | ✓ | ✓ |  |
| Server | `'connection'` | ✓ | ✓ |  |
| Server | `'error'` | ✓ | ✓ |  |
| Server | `'listening'` | ✓ | ✓ |  |
| Server | `'drop'` | ✓ | ✓ |  |
| Socket | `'close'` | ✓ | ✓ |  |
| Socket | `'connect'` | ✓ | ✓ |  |
| Socket | `'connectionAttempt'` | ✓ | ✓ |  |
| Socket | `'connectionAttemptFailed'` | ✓ | ✓ |  |
| Socket | `'connectionAttemptTimeout'` | ✓ | ✓ |  |
| Socket | `'data'` | ✓ | ✓ |  |
| Socket | `'drain'` | ✓ | ✓ |  |
| Socket | `'end'` | ✓ | ✓ |  |
| Socket | `'error'` | ✓ | ✓ |  |
| Socket | `'lookup'` | ✓ | ✓ |  |
| Socket | `'ready'` | ✓ | ✓ |  |
| Socket | `'timeout'` | ✓ | ✓ |  |

---

### node:tls

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `tls.checkServerIdentity(hostname, cert)` | ✓ | ✓ |  |
| `tls.connect(options[, callback])` | ✓ | ✓ |  |
| `tls.connect(port[, host][, options][, callback])` | ✓ | ✓ |  |
| `tls.connect(path[, options][, callback])` | ✓ | ✓ |  |
| `tls.createSecureContext([options])` | ✓ | ✓ |  |
| `tls.createSecurePair([context][, isServer][, requestCert][, rejectUnauthorized][, options])` | ✓ | ✗ | Missing in Bun (per Bun docs) |
| `tls.createServer([options][, secureConnectionListener])` | ✓ | ✓ |  |
| `tls.setDefaultCACertificates(certs)` | ✓ | ⚠ |  |
| `tls.getCACertificates([type])` | ✓ | ⚠ |  |
| `tls.getCiphers()` | ✓ | ✓ |  |

#### Module Properties / Constants
| Constant / Property | Node.js | Bun | Notes |
|--------------------|---------|-----|-------|
| `tls.DEFAULT_ECDH_CURVE` | ✓ | ✓ |  |
| `tls.DEFAULT_MAX_VERSION` | ✓ | ✓ |  |
| `tls.DEFAULT_MIN_VERSION` | ✓ | ✓ |  |
| `tls.DEFAULT_CIPHERS` | ✓ | ✓ |  |
| `tls.rootCertificates` | ✓ | ✓ |  |
| `tls.CLIENT_RENEG_LIMIT` | ✓ | ✓ |  |
| `tls.CLIENT_RENEG_WINDOW` | ✓ | ✓ |  |

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class tls.Server` (extends `net.Server`) | ✓ | ✓ |  |
| `server.addContext(hostname, context)` | ✓ | ✓ |  |
| `server.address()` | ✓ | ✓ |  |
| `server.close([callback])` | ✓ | ✓ |  |
| `server.getTicketKeys()` | ✓ | ✓ |  |
| `server.listen()` | ✓ | ✓ |  |
| `server.setSecureContext(options)` | ✓ | ✓ |  |
| `server.setTicketKeys(keys)` | ✓ | ✓ |  |
| `class tls.TLSSocket` (extends `net.Socket`) | ✓ | ✓ |  |
| `new tls.TLSSocket(socket[, options])` | ✓ | ✓ |  |
| `tlsSocket.authorized` | ✓ | ✓ |  |
| `tlsSocket.authorizationError` | ✓ | ✓ |  |
| `tlsSocket.encrypted` | ✓ | ✓ |  |
| `tlsSocket.localAddress` | ✓ | ✓ |  |
| `tlsSocket.localPort` | ✓ | ✓ |  |
| `tlsSocket.remoteAddress` | ✓ | ✓ |  |
| `tlsSocket.remoteFamily` | ✓ | ✓ |  |
| `tlsSocket.remotePort` | ✓ | ✓ |  |
| `tlsSocket.address()` | ✓ | ✓ |  |
| `tlsSocket.disableRenegotiation()` | ✓ | ✓ |  |
| `tlsSocket.enableTrace()` | ✓ | ✓ |  |
| `tlsSocket.exportKeyingMaterial(length, label[, context])` | ✓ | ✓ |  |
| `tlsSocket.getCertificate()` | ✓ | ✓ |  |
| `tlsSocket.getCipher()` | ✓ | ✓ |  |
| `tlsSocket.getEphemeralKeyInfo()` | ✓ | ✓ |  |
| `tlsSocket.getFinished()` | ✓ | ✓ |  |
| `tlsSocket.getPeerCertificate([detailed])` | ✓ | ✓ |  |
| `tlsSocket.getPeerFinished()` | ✓ | ✓ |  |
| `tlsSocket.getPeerX509Certificate()` | ✓ | ✓ |  |
| `tlsSocket.getProtocol()` | ✓ | ✓ |  |
| `tlsSocket.getSession()` | ✓ | ✓ |  |
| `tlsSocket.getSharedSigalgs()` | ✓ | ✓ |  |
| `tlsSocket.getTLSTicket()` | ✓ | ✓ |  |
| `tlsSocket.getX509Certificate()` | ✓ | ✓ |  |
| `tlsSocket.isSessionReused()` | ✓ | ✓ |  |
| `tlsSocket.renegotiate(options, callback)` | ✓ | ✓ |  |
| `tlsSocket.setKeyCert(context)` | ✓ | ⚠ | Newer API |
| `tlsSocket.setMaxSendFragment(size)` | ✓ | ✓ |  |
| `class tls.SecureContext` | ✓ | ✓ |  |
| `class tls.CryptoStream` | ✓ | ✗ | Deprecated; replaced by TLSSocket |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| Server | `'connection'` | ✓ | ✓ |  |
| Server | `'keylog'` | ✓ | ✓ |  |
| Server | `'newSession'` | ✓ | ✓ |  |
| Server | `'OCSPRequest'` | ✓ | ✓ |  |
| Server | `'resumeSession'` | ✓ | ✓ |  |
| Server | `'secureConnection'` | ✓ | ✓ |  |
| Server | `'tlsClientError'` | ✓ | ✓ |  |
| TLSSocket | `'keylog'` | ✓ | ✓ |  |
| TLSSocket | `'OCSPResponse'` | ✓ | ✓ |  |
| TLSSocket | `'secure'` | ✓ | ✓ |  |
| TLSSocket | `'secureConnect'` | ✓ | ✓ |  |
| TLSSocket | `'session'` | ✓ | ✓ |  |

---

### node:dgram

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `dgram.createSocket(options[, callback])` | ✓ | ✓ |  |
| `dgram.createSocket(type[, callback])` | ✓ | ✓ |  |

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class dgram.Socket` (extends EventEmitter) | ✓ | ✓ |  |
| `socket.addMembership(multicastAddress[, multicastInterface])` | ✓ | ✓ |  |
| `socket.addSourceSpecificMembership(sourceAddress, groupAddress[, multicastInterface])` | ✓ | ✓ |  |
| `socket.address()` | ✓ | ✓ |  |
| `socket.bind([port][, address][, callback])` | ✓ | ✓ |  |
| `socket.bind(options[, callback])` | ✓ | ✓ |  |
| `socket.close([callback])` | ✓ | ✓ |  |
| `socket.connect(port[, address][, callback])` | ✓ | ✓ |  |
| `socket.disconnect()` | ✓ | ✓ |  |
| `socket.dropMembership(multicastAddress[, multicastInterface])` | ✓ | ✓ |  |
| `socket.dropSourceSpecificMembership(sourceAddress, groupAddress[, multicastInterface])` | ✓ | ✓ |  |
| `socket.getRecvBufferSize()` | ✓ | ✓ |  |
| `socket.getSendBufferSize()` | ✓ | ✓ |  |
| `socket.getSendQueueSize()` | ✓ | ✓ |  |
| `socket.getSendQueueCount()` | ✓ | ✓ |  |
| `socket.ref()` | ✓ | ✓ |  |
| `socket.remoteAddress()` | ✓ | ✓ |  |
| `socket.send(msg[, offset, length][, port][, address][, callback])` | ✓ | ✓ |  |
| `socket.setBroadcast(flag)` | ✓ | ✓ |  |
| `socket.setMulticastInterface(multicastInterface)` | ✓ | ✓ |  |
| `socket.setMulticastLoopback(flag)` | ✓ | ✓ |  |
| `socket.setMulticastTTL(ttl)` | ✓ | ✓ |  |
| `socket.setRecvBufferSize(size)` | ✓ | ✓ |  |
| `socket.setSendBufferSize(size)` | ✓ | ✓ |  |
| `socket.setTTL(ttl)` | ✓ | ✓ |  |
| `socket.unref()` | ✓ | ✓ |  |
| `socket[Symbol.asyncDispose]()` | ✓ | ✓ |  |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| Socket | `'close'` | ✓ | ✓ |  |
| Socket | `'connect'` | ✓ | ✓ |  |
| Socket | `'error'` | ✓ | ✓ |  |
| Socket | `'listening'` | ✓ | ✓ |  |
| Socket | `'message'` | ✓ | ✓ |  |

---

### node:dns

#### Functions (Callback API)
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `dns.lookup(hostname[, options], callback)` | ✓ | ✓ |  |
| `dns.lookupService(address, port, callback)` | ✓ | ✓ |  |
| `dns.resolve(hostname[, rrtype], callback)` | ✓ | ✓ |  |
| `dns.resolve4(hostname[, options], callback)` | ✓ | ✓ |  |
| `dns.resolve6(hostname[, options], callback)` | ✓ | ✓ |  |
| `dns.resolveAny(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveCaa(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveCname(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveMx(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveNaptr(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveNs(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolvePtr(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveSoa(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveSrv(hostname, callback)` | ✓ | ✓ |  |
| `dns.resolveTlsa(hostname, callback)` | ✓ | ⚠ | Newer Node API |
| `dns.resolveTxt(hostname, callback)` | ✓ | ✓ |  |
| `dns.reverse(ip, callback)` | ✓ | ✓ |  |
| `dns.getServers()` | ✓ | ✓ |  |
| `dns.setServers(servers)` | ✓ | ✓ |  |
| `dns.setDefaultResultOrder(order)` | ✓ | ✓ |  |
| `dns.getDefaultResultOrder()` | ✓ | ✓ |  |

#### Classes
| Class / Method | Node.js | Bun | Notes |
|----------------|---------|-----|-------|
| `class dns.Resolver` | ✓ | ✓ |  |
| `new dns.Resolver([options])` | ✓ | ✓ |  |
| `resolver.cancel()` | ✓ | ✓ |  |
| `resolver.setLocalAddress([ipv4][, ipv6])` | ✓ | ✓ |  |
| `resolver.getServers()` | ✓ | ✓ |  |
| `resolver.setServers(servers)` | ✓ | ✓ |  |
| Resolver inherits all `resolve*` / `reverse` methods | ✓ | ✓ |  |

#### Constants / Error Codes
| Constant | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `dns.ADDRCONFIG` | ✓ | ✓ |  |
| `dns.V4MAPPED` | ✓ | ✓ |  |
| `dns.ALL` | ✓ | ✓ |  |
| `dns.NODATA` | ✓ | ✓ |  |
| `dns.FORMERR` | ✓ | ✓ |  |
| `dns.SERVFAIL` | ✓ | ✓ |  |
| `dns.NOTFOUND` | ✓ | ✓ |  |
| `dns.NOTIMP` | ✓ | ✓ |  |
| `dns.REFUSED` | ✓ | ✓ |  |
| `dns.BADQUERY` | ✓ | ✓ |  |
| `dns.BADNAME` | ✓ | ✓ |  |
| `dns.BADFAMILY` | ✓ | ✓ |  |
| `dns.BADRESP` | ✓ | ✓ |  |
| `dns.CONNREFUSED` | ✓ | ✓ |  |
| `dns.TIMEOUT` | ✓ | ✓ |  |
| `dns.EOF` | ✓ | ✓ |  |
| `dns.FILE` | ✓ | ✓ |  |
| `dns.NOMEM` | ✓ | ✓ |  |
| `dns.DESTRUCTION` | ✓ | ✓ |  |
| `dns.BADSTR` | ✓ | ✓ |  |
| `dns.BADFLAGS` | ✓ | ✓ |  |
| `dns.NONAME` | ✓ | ✓ |  |
| `dns.BADHINTS` | ✓ | ✓ |  |
| `dns.NOTINITIALIZED` | ✓ | ✓ |  |
| `dns.LOADIPHLPAPI` | ✓ | ✓ |  |
| `dns.ADDRGETNETWORKPARAMS` | ✓ | ✓ |  |
| `dns.CANCELLED` | ✓ | ✓ |  |

---

### node:dns/promises

Available via `require('node:dns/promises')` or `dns.promises`.

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `dnsPromises.lookup(hostname[, options])` | ✓ | ✓ |  |
| `dnsPromises.lookupService(address, port)` | ✓ | ✓ |  |
| `dnsPromises.resolve(hostname[, rrtype])` | ✓ | ✓ |  |
| `dnsPromises.resolve4(hostname[, options])` | ✓ | ✓ |  |
| `dnsPromises.resolve6(hostname[, options])` | ✓ | ✓ |  |
| `dnsPromises.resolveAny(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveCaa(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveCname(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveMx(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveNaptr(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveNs(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolvePtr(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveSoa(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveSrv(hostname)` | ✓ | ✓ |  |
| `dnsPromises.resolveTlsa(hostname)` | ✓ | ⚠ | Newer Node API |
| `dnsPromises.resolveTxt(hostname)` | ✓ | ✓ |  |
| `dnsPromises.reverse(ip)` | ✓ | ✓ |  |
| `dnsPromises.getServers()` | ✓ | ✓ |  |
| `dnsPromises.setServers(servers)` | ✓ | ✓ |  |
| `dnsPromises.setDefaultResultOrder(order)` | ✓ | ✓ |  |
| `dnsPromises.getDefaultResultOrder()` | ✓ | ✓ |  |

#### Classes
| Class / Method | Node.js | Bun | Notes |
|----------------|---------|-----|-------|
| `class dnsPromises.Resolver` | ✓ | ✓ | Same surface as `dns.Resolver` but resolve* methods return Promises |

---

### node:crypto

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `crypto.checkPrime(candidate[, options], callback)` | ✓ | ✓ | Buffer/ArrayBuffer-compatible and BigInt prime parity fixture |
| `crypto.checkPrimeSync(candidate[, options])` | ✓ | ✓ | Buffer/ArrayBuffer-compatible and BigInt prime parity fixture |
| `crypto.createCipheriv(algorithm, key, iv[, options])` | ✓ | ✓ |  |
| `crypto.createDecipheriv(algorithm, key, iv[, options])` | ✓ | ✓ |  |
| `crypto.createDiffieHellman(prime[, primeEncoding][, generator][, generatorEncoding])` | ✓ | ✓ |  |
| `crypto.createDiffieHellman(primeLength[, generator])` | ✓ | ✓ |  |
| `crypto.createDiffieHellmanGroup(name)` | ✓ | ✓ |  |
| `crypto.createECDH(curveName)` | ✓ | ✓ |  |
| `crypto.createHash(algorithm[, options])` | ✓ | ✓ |  |
| `crypto.createHmac(algorithm, key[, options])` | ✓ | ✓ |  |
| `crypto.createPrivateKey(key)` | ✓ | ✓ |  |
| `crypto.createPublicKey(key)` | ✓ | ✓ |  |
| `crypto.createSecretKey(key[, encoding])` | ✓ | ✓ |  |
| `crypto.createSign(algorithm[, options])` | ✓ | ✓ |  |
| `crypto.createVerify(algorithm[, options])` | ✓ | ✓ |  |
| `crypto.diffieHellman(options[, callback])` | ✓ | ✓ |  |
| `crypto.generateKey(type, options, callback)` | ✓ | ✓ |  |
| `crypto.generateKeySync(type, options)` | ✓ | ✓ |  |
| `crypto.generateKeyPair(type, options, callback)` | ✓ | ✓ |  |
| `crypto.generateKeyPairSync(type, options)` | ✓ | ✓ |  |
| `crypto.generatePrime(size[, options], callback)` | ✓ | ✓ | Buffer/ArrayBuffer-compatible and BigInt prime parity fixture |
| `crypto.generatePrimeSync(size[, options])` | ✓ | ✓ | Buffer/ArrayBuffer-compatible and BigInt prime parity fixture |
| `crypto.getCipherInfo(nameOrNid[, options])` | ✓ | ✓ |  |
| `crypto.getCiphers()` | ✓ | ✓ |  |
| `crypto.getCurves()` | ✓ | ✓ |  |
| `crypto.getDiffieHellman(groupName)` | ✓ | ✓ |  |
| `crypto.getFips()` | ✓ | ✓ |  |
| `crypto.getHashes()` | ✓ | ✓ |  |
| `crypto.getRandomValues(typedArray)` | ✓ | ✓ |  |
| `crypto.hash(algorithm, data[, options])` | ✓ | ✓ |  |
| `crypto.hkdf(digest, ikm, salt, info, keylen, callback)` | ✓ | ✓ |  |
| `crypto.hkdfSync(digest, ikm, salt, info, keylen)` | ✓ | ✓ |  |
| `crypto.pbkdf2(password, salt, iterations, keylen, digest, callback)` | ✓ | ✓ |  |
| `crypto.pbkdf2Sync(password, salt, iterations, keylen, digest)` | ✓ | ✓ |  |
| `crypto.privateDecrypt(privateKey, buffer)` | ✓ | ✓ |  |
| `crypto.privateEncrypt(privateKey, buffer)` | ✓ | ✓ |  |
| `crypto.publicDecrypt(key, buffer)` | ✓ | ✓ |  |
| `crypto.publicEncrypt(key, buffer)` | ✓ | ✓ |  |
| `crypto.randomBytes(size[, callback])` | ✓ | ✓ |  |
| `crypto.randomFill(buffer[, offset][, size], callback)` | ✓ | ✓ |  |
| `crypto.randomFillSync(buffer[, offset][, size])` | ✓ | ✓ |  |
| `crypto.randomInt([min, ]max[, callback])` | ✓ | ✓ |  |
| `crypto.randomUUID([options])` | ✓ | ✓ |  |
| `crypto.randomUUIDv7([options])` | ✓ | ⚠ | Newer Node API |
| `crypto.scrypt(password, salt, keylen[, options], callback)` | ✓ | ✓ |  |
| `crypto.scryptSync(password, salt, keylen[, options])` | ✓ | ✓ |  |
| `crypto.secureHeapUsed()` | ✓ | ✗ | Missing in Bun |
| `crypto.setEngine(engine[, flags])` | ✓ | ✗ | Missing in Bun |
| `crypto.setFips(bool)` | ✓ | ✗ | Missing in Bun |
| `crypto.sign(algorithm, data, key[, callback])` | ✓ | ✓ |  |
| `crypto.timingSafeEqual(a, b)` | ✓ | ✓ |  |
| `crypto.verify(algorithm, data, key, signature[, callback])` | ✓ | ✓ |  |
| `crypto.argon2(algorithm, parameters, callback)` | ✓ | ⚠ | New v22+ |
| `crypto.argon2Sync(algorithm, parameters)` | ✓ | ⚠ | New v22+ |
| `crypto.encapsulate(key[, callback])` | ✓ | ⚠ | New PQC API |
| `crypto.decapsulate(key, ciphertext[, callback])` | ✓ | ⚠ | New PQC API |

#### Module Properties
| Property | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `crypto.constants` | ✓ | ✓ |  |
| `crypto.fips` | ✓ | ✓ |  |
| `crypto.subtle` | ✓ | ✓ |  |
| `crypto.webcrypto` | ✓ | ✓ |  |

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class crypto.Certificate` | ✓ | ✓ | Legacy |
| `Certificate.exportChallenge(spkac[, encoding])` | ✓ | ✓ |  |
| `Certificate.exportPublicKey(spkac[, encoding])` | ✓ | ✓ |  |
| `Certificate.verifySpkac(spkac[, encoding])` | ✓ | ✓ |  |
| `class crypto.Cipher` | ✓ | ✓ |  |
| `cipher.final([outputEncoding])` | ✓ | ✓ |  |
| `cipher.getAuthTag()` | ✓ | ✓ |  |
| `cipher.setAAD(buffer[, options])` | ✓ | ✓ |  |
| `cipher.setAutoPadding([autoPadding])` | ✓ | ✓ |  |
| `cipher.update(data[, inputEncoding][, outputEncoding])` | ✓ | ✓ |  |
| `class crypto.Decipher` | ✓ | ✓ |  |
| `decipher.final([outputEncoding])` | ✓ | ✓ |  |
| `decipher.setAAD(buffer[, options])` | ✓ | ✓ |  |
| `decipher.setAuthTag(buffer[, encoding])` | ✓ | ✓ |  |
| `decipher.setAutoPadding([autoPadding])` | ✓ | ✓ |  |
| `decipher.update(data[, inputEncoding][, outputEncoding])` | ✓ | ✓ |  |
| `class crypto.DiffieHellman` | ✓ | ✓ |  |
| `dh.computeSecret(otherPublicKey[, inputEncoding][, outputEncoding])` | ✓ | ✓ |  |
| `dh.generateKeys([encoding])` | ✓ | ✓ |  |
| `dh.getGenerator([encoding])` | ✓ | ✓ |  |
| `dh.getPrime([encoding])` | ✓ | ✓ |  |
| `dh.getPrivateKey([encoding])` | ✓ | ✓ |  |
| `dh.getPublicKey([encoding])` | ✓ | ✓ |  |
| `dh.setPrivateKey(privateKey[, encoding])` | ✓ | ✓ |  |
| `dh.setPublicKey(publicKey[, encoding])` | ✓ | ✓ |  |
| `dh.verifyError` | ✓ | ✓ |  |
| `class crypto.DiffieHellmanGroup` | ✓ | ✓ |  |
| `class crypto.ECDH` | ✓ | ✓ |  |
| `ECDH.convertKey(key, curve[, inputEncoding[, outputEncoding[, format]]])` | ✓ | ✓ |  |
| `ecdh.computeSecret(otherPublicKey[, inputEncoding][, outputEncoding])` | ✓ | ✓ |  |
| `ecdh.generateKeys([encoding[, format]])` | ✓ | ✓ |  |
| `ecdh.getPrivateKey([encoding])` | ✓ | ✓ |  |
| `ecdh.getPublicKey([encoding][, format])` | ✓ | ✓ |  |
| `ecdh.setPrivateKey(privateKey[, encoding])` | ✓ | ✓ |  |
| `ecdh.setPublicKey(publicKey[, encoding])` | ✓ | ✓ |  |
| `class crypto.Hash` | ✓ | ✓ |  |
| `hash.copy([options])` | ✓ | ✓ |  |
| `hash.digest([encoding])` | ✓ | ✓ |  |
| `hash.update(data[, inputEncoding])` | ✓ | ✓ |  |
| `class crypto.Hmac` | ✓ | ✓ |  |
| `hmac.digest([encoding])` | ✓ | ✓ |  |
| `hmac.update(data[, inputEncoding])` | ✓ | ✓ |  |
| `class crypto.KeyObject` | ✓ | ✓ |  |
| `KeyObject.from(key)` | ✓ | ✓ |  |
| `keyObject.asymmetricKeyDetails` | ✓ | ✓ |  |
| `keyObject.asymmetricKeyType` | ✓ | ✓ |  |
| `keyObject.equals(otherKeyObject)` | ✓ | ✓ |  |
| `keyObject.export([options])` | ✓ | ✓ |  |
| `keyObject.symmetricKeySize` | ✓ | ✓ |  |
| `keyObject.toCryptoKey(algorithm, extractable, keyUsages)` | ✓ | ⚠ | Newer API |
| `keyObject.type` | ✓ | ✓ |  |
| `class crypto.Sign` | ✓ | ✓ |  |
| `sign.sign(privateKey[, outputEncoding])` | ✓ | ✓ |  |
| `sign.update(data[, inputEncoding])` | ✓ | ✓ |  |
| `class crypto.Verify` | ✓ | ✓ |  |
| `verify.update(data[, inputEncoding])` | ✓ | ✓ |  |
| `verify.verify(key, signature[, signatureEncoding])` | ✓ | ✓ |  |
| `class crypto.X509Certificate` | ✓ | ✓ |  |
| `new X509Certificate(buffer)` | ✓ | ✓ |  |
| `x509.ca` | ✓ | ✓ |  |
| `x509.checkEmail(email[, options])` | ✓ | ✓ |  |
| `x509.checkHost(name[, options])` | ✓ | ✓ |  |
| `x509.checkIP(ip)` | ✓ | ✓ |  |
| `x509.checkIssued(otherCert)` | ✓ | ✓ |  |
| `x509.checkPrivateKey(privateKey)` | ✓ | ✓ |  |
| `x509.fingerprint` | ✓ | ✓ |  |
| `x509.fingerprint256` | ✓ | ✓ |  |
| `x509.fingerprint512` | ✓ | ✓ |  |
| `x509.infoAccess` | ✓ | ✓ |  |
| `x509.issuer` | ✓ | ✓ |  |
| `x509.issuerCertificate` | ✓ | ✓ |  |
| `x509.keyUsage` | ✓ | ✓ |  |
| `x509.publicKey` | ✓ | ✓ |  |
| `x509.raw` | ✓ | ✓ |  |
| `x509.serialNumber` | ✓ | ✓ |  |
| `x509.signatureAlgorithm` | ✓ | ✓ |  |
| `x509.signatureAlgorithmOid` | ✓ | ✓ |  |
| `x509.subject` | ✓ | ✓ |  |
| `x509.subjectAltName` | ✓ | ✓ |  |
| `x509.toJSON()` | ✓ | ✓ |  |
| `x509.toLegacyObject()` | ✓ | ✓ |  |
| `x509.toString()` | ✓ | ✓ |  |
| `x509.validFrom` | ✓ | ✓ |  |
| `x509.validFromDate` | ✓ | ✓ |  |
| `x509.validTo` | ✓ | ✓ |  |
| `x509.validToDate` | ✓ | ✓ |  |
| `x509.verify(publicKey)` | ✓ | ✓ |  |

#### Webcrypto Namespace
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `crypto.webcrypto.subtle.*` (SubtleCrypto) | ✓ | ✓ |  |
| `crypto.webcrypto.CryptoKey` | ✓ | ✓ |  |
| `crypto.webcrypto.getRandomValues(typedArray)` | ✓ | ✓ |  |
| `crypto.webcrypto.randomUUID()` | ✓ | ✓ |  |

---

### node:stream

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `stream.pipeline(source[, ...transforms], destination, callback)` | ✓ | ✓ |  |
| `stream.pipeline(streams, callback)` | ✓ | ✓ |  |
| `stream.finished(stream[, options], callback)` | ✓ | ✓ |  |
| `stream.compose(...streams)` | ✓ | ✓ |  |
| `stream.isReadable(stream)` | ✓ | ✓ |  |
| `stream.isWritable(stream)` | ✓ | ✓ |  |
| `stream.isErrored(stream)` | ✓ | ✓ |  |
| `stream.getDefaultHighWaterMark(objectMode)` | ✓ | ✓ |  |
| `stream.setDefaultHighWaterMark(objectMode, value)` | ✓ | ✓ |  |
| `stream.addAbortSignal(signal, stream)` | ✓ | ✓ |  |
| `stream.duplexPair([options])` | ✓ | ✓ |  |
| `stream.Readable.from(iterable[, options])` | ✓ | ✓ |  |
| `stream.Readable.fromWeb(readableStream[, options])` | ✓ | ✓ |  |
| `stream.Readable.toWeb(streamReadable[, options])` | ✓ | ✓ |  |
| `stream.Readable.isDisturbed(stream)` | ✓ | ✓ |  |
| `stream.Writable.fromWeb(writableStream[, options])` | ✓ | ✓ |  |
| `stream.Writable.toWeb(streamWritable)` | ✓ | ✓ |  |
| `stream.Duplex.from(src)` | ✓ | ✓ |  |
| `stream.Duplex.fromWeb(pair[, options])` | ✓ | ✓ |  |
| `stream.Duplex.toWeb(streamDuplex[, options])` | ✓ | ✓ |  |

#### Classes — stream.Readable
| Method / Property | Node.js | Bun | Notes |
|-------------------|---------|-----|-------|
| `class stream.Readable` | ✓ | ✓ |  |
| `readable.read([size])` | ✓ | ✓ |  |
| `readable.pause()` | ✓ | ✓ |  |
| `readable.resume()` | ✓ | ✓ |  |
| `readable.pipe(destination[, options])` | ✓ | ✓ |  |
| `readable.unpipe([destination])` | ✓ | ✓ |  |
| `readable.unshift(chunk[, encoding])` | ✓ | ✓ |  |
| `readable.setEncoding(encoding)` | ✓ | ✓ |  |
| `readable.isPaused()` | ✓ | ✓ |  |
| `readable.destroy([error])` | ✓ | ✓ |  |
| `readable.wrap(stream)` | ✓ | ✓ |  |
| `readable.compose(stream[, options])` | ✓ | ✓ |  |
| `readable.iterator([options])` | ✓ | ✓ |  |
| `readable.map(fn[, options])` | ✓ | ✓ |  |
| `readable.filter(fn[, options])` | ✓ | ✓ |  |
| `readable.forEach(fn[, options])` | ✓ | ✓ |  |
| `readable.toArray([options])` | ✓ | ✓ |  |
| `readable.some(fn[, options])` | ✓ | ✓ |  |
| `readable.find(fn[, options])` | ✓ | ✓ |  |
| `readable.every(fn[, options])` | ✓ | ✓ |  |
| `readable.flatMap(fn[, options])` | ✓ | ✓ |  |
| `readable.drop(limit[, options])` | ✓ | ✓ |  |
| `readable.take(limit[, options])` | ✓ | ✓ |  |
| `readable.reduce(fn[, initial[, options]])` | ✓ | ✓ |  |
| `readable.push(chunk[, encoding])` | ✓ | ✓ |  |
| `readable._read(size)` (implementer) | ✓ | ✓ |  |
| `readable._construct(callback)` (implementer) | ✓ | ✓ |  |
| `readable._destroy(err, callback)` (implementer) | ✓ | ✓ |  |
| `readable.readable` | ✓ | ✓ |  |
| `readable.readableFlowing` | ✓ | ✓ |  |
| `readable.readableLength` | ✓ | ✓ |  |
| `readable.readableHighWaterMark` | ✓ | ✓ |  |
| `readable.readableAborted` | ✓ | ✓ |  |
| `readable.readableDidRead` | ✓ | ✓ |  |
| `readable.readableEncoding` | ✓ | ✓ |  |
| `readable.readableEnded` | ✓ | ✓ |  |
| `readable.readableObjectMode` | ✓ | ✓ |  |
| `readable.errored` | ✓ | ✓ |  |
| `readable.destroyed` | ✓ | ✓ |  |
| `readable.closed` | ✓ | ✓ |  |
| `readable[Symbol.asyncIterator]()` | ✓ | ✓ |  |
| `readable[Symbol.asyncDispose]()` | ✓ | ✓ |  |

#### Classes — stream.Writable
| Method / Property | Node.js | Bun | Notes |
|-------------------|---------|-----|-------|
| `class stream.Writable` | ✓ | ✓ |  |
| `writable.write(chunk[, encoding][, callback])` | ✓ | ✓ |  |
| `writable.end([chunk[, encoding]][, callback])` | ✓ | ✓ |  |
| `writable.cork()` | ✓ | ✓ |  |
| `writable.uncork()` | ✓ | ✓ |  |
| `writable.destroy([error])` | ✓ | ✓ |  |
| `writable.setDefaultEncoding(encoding)` | ✓ | ✓ |  |
| `writable._write(chunk, encoding, callback)` (implementer) | ✓ | ✓ |  |
| `writable._writev(chunks, callback)` (implementer) | ✓ | ✓ |  |
| `writable._construct(callback)` (implementer) | ✓ | ✓ |  |
| `writable._destroy(err, callback)` (implementer) | ✓ | ✓ |  |
| `writable._final(callback)` (implementer) | ✓ | ✓ |  |
| `writable.writable` | ✓ | ✓ |  |
| `writable.writableLength` | ✓ | ✓ |  |
| `writable.writableHighWaterMark` | ✓ | ✓ |  |
| `writable.writableNeedDrain` | ✓ | ✓ |  |
| `writable.writableObjectMode` | ✓ | ✓ |  |
| `writable.writableEnded` | ✓ | ✓ |  |
| `writable.writableFinished` | ✓ | ✓ |  |
| `writable.writableCorked` | ✓ | ✓ |  |
| `writable.writableAborted` | ✓ | ✓ |  |
| `writable.errored` | ✓ | ✓ |  |
| `writable.destroyed` | ✓ | ✓ |  |
| `writable.closed` | ✓ | ✓ |  |
| `writable[Symbol.asyncDispose]()` | ✓ | ✓ |  |

#### Classes — stream.Duplex / Transform / PassThrough
| Class / Method | Node.js | Bun | Notes |
|----------------|---------|-----|-------|
| `class stream.Duplex` | ✓ | ✓ | Combines Readable + Writable |
| `new stream.Duplex([options])` | ✓ | ✓ |  |
| `duplex.allowHalfOpen` | ✓ | ✓ |  |
| `class stream.Transform` | ✓ | ✓ |  |
| `new stream.Transform([options])` | ✓ | ✓ |  |
| `transform.destroy([error])` | ✓ | ✓ |  |
| `transform._transform(chunk, encoding, callback)` (implementer) | ✓ | ✓ |  |
| `transform._flush(callback)` (implementer) | ✓ | ✓ |  |
| `class stream.PassThrough` | ✓ | ✓ |  |

#### Events
| Class | Event | Node.js | Bun | Notes |
|-------|-------|---------|-----|-------|
| Readable | `'close'` | ✓ | ✓ |  |
| Readable | `'data'` | ✓ | ✓ |  |
| Readable | `'end'` | ✓ | ✓ |  |
| Readable | `'error'` | ✓ | ✓ |  |
| Readable | `'pause'` | ✓ | ✓ |  |
| Readable | `'readable'` | ✓ | ✓ |  |
| Readable | `'resume'` | ✓ | ✓ |  |
| Writable | `'close'` | ✓ | ✓ |  |
| Writable | `'drain'` | ✓ | ✓ |  |
| Writable | `'error'` | ✓ | ✓ |  |
| Writable | `'finish'` | ✓ | ✓ |  |
| Writable | `'pipe'` | ✓ | ✓ |  |
| Writable | `'unpipe'` | ✓ | ✓ |  |
| Transform | `'end'` | ✓ | ✓ |  |
| Transform | `'finish'` | ✓ | ✓ |  |

---

### node:stream/promises

Available via `require('node:stream/promises')`.

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `streamPromises.pipeline(source[, ...transforms], destination[, options])` | ✓ | ✓ | Returns Promise |
| `streamPromises.pipeline(streams[, options])` | ✓ | ✓ |  |
| `streamPromises.finished(stream[, options])` | ✓ | ✓ | Returns Promise |

---

### node:stream/web

Available via `require('node:stream/web')`. Web Streams API.

#### Classes
| Class / Method / Property | Node.js | Bun | Notes |
|---------------------------|---------|-----|-------|
| `class ReadableStream` | ✓ | ✓ |  |
| `new ReadableStream([underlyingSource[, strategy]])` | ✓ | ✓ |  |
| `readableStream.locked` | ✓ | ✓ |  |
| `readableStream.cancel([reason])` | ✓ | ✓ |  |
| `readableStream.getReader([options])` | ✓ | ✓ |  |
| `readableStream.pipeThrough(transform[, options])` | ✓ | ✓ |  |
| `readableStream.pipeTo(destination[, options])` | ✓ | ✓ |  |
| `readableStream.tee()` | ✓ | ✓ |  |
| `readableStream.values([options])` | ✓ | ✓ |  |
| `readableStream[Symbol.asyncIterator]()` | ✓ | ✓ |  |
| `ReadableStream.from(iterable)` | ✓ | ✓ |  |
| `class ReadableStreamDefaultReader` | ✓ | ✓ |  |
| `new ReadableStreamDefaultReader(stream)` | ✓ | ✓ |  |
| `reader.closed` | ✓ | ✓ |  |
| `reader.read()` | ✓ | ✓ |  |
| `reader.cancel([reason])` | ✓ | ✓ |  |
| `reader.releaseLock()` | ✓ | ✓ |  |
| `class ReadableStreamBYOBReader` | ✓ | ✓ |  |
| `new ReadableStreamBYOBReader(stream)` | ✓ | ✓ |  |
| `byobReader.closed` | ✓ | ✓ |  |
| `byobReader.read(view[, options])` | ✓ | ✓ |  |
| `byobReader.cancel([reason])` | ✓ | ✓ |  |
| `byobReader.releaseLock()` | ✓ | ✓ |  |
| `class ReadableStreamDefaultController` | ✓ | ✓ |  |
| `controller.desiredSize` | ✓ | ✓ |  |
| `controller.close()` | ✓ | ✓ |  |
| `controller.enqueue([chunk])` | ✓ | ✓ |  |
| `controller.error([error])` | ✓ | ✓ |  |
| `class ReadableByteStreamController` | ✓ | ✓ |  |
| `byteController.byobRequest` | ✓ | ✓ |  |
| `byteController.desiredSize` | ✓ | ✓ |  |
| `byteController.close()` | ✓ | ✓ |  |
| `byteController.enqueue(chunk)` | ✓ | ✓ |  |
| `byteController.error([error])` | ✓ | ✓ |  |
| `class ReadableStreamBYOBRequest` | ✓ | ✓ |  |
| `byobRequest.view` | ✓ | ✓ |  |
| `byobRequest.respond(bytesWritten)` | ✓ | ✓ |  |
| `byobRequest.respondWithNewView(view)` | ✓ | ✓ |  |
| `class WritableStream` | ✓ | ✓ |  |
| `new WritableStream([underlyingSink[, strategy]])` | ✓ | ✓ |  |
| `writableStream.locked` | ✓ | ✓ |  |
| `writableStream.abort([reason])` | ✓ | ✓ |  |
| `writableStream.close()` | ✓ | ✓ |  |
| `writableStream.getWriter()` | ✓ | ✓ |  |
| `class WritableStreamDefaultWriter` | ✓ | ✓ |  |
| `new WritableStreamDefaultWriter(stream)` | ✓ | ✓ |  |
| `writer.closed` | ✓ | ✓ |  |
| `writer.desiredSize` | ✓ | ✓ |  |
| `writer.ready` | ✓ | ✓ |  |
| `writer.write([chunk])` | ✓ | ✓ |  |
| `writer.close()` | ✓ | ✓ |  |
| `writer.abort([reason])` | ✓ | ✓ |  |
| `writer.releaseLock()` | ✓ | ✓ |  |
| `class WritableStreamDefaultController` | ✓ | ✓ |  |
| `writableController.signal` | ✓ | ✓ |  |
| `writableController.error([error])` | ✓ | ✓ |  |
| `class TransformStream` | ✓ | ✓ |  |
| `new TransformStream([transformer[, writableStrategy[, readableStrategy]]])` | ✓ | ✓ |  |
| `transformStream.readable` | ✓ | ✓ |  |
| `transformStream.writable` | ✓ | ✓ |  |
| `class TransformStreamDefaultController` | ✓ | ✓ |  |
| `transformController.desiredSize` | ✓ | ✓ |  |
| `transformController.enqueue([chunk])` | ✓ | ✓ |  |
| `transformController.error([reason])` | ✓ | ✓ |  |
| `transformController.terminate()` | ✓ | ✓ |  |
| `class ByteLengthQueuingStrategy` | ✓ | ✓ |  |
| `new ByteLengthQueuingStrategy(init)` | ✓ | ✓ |  |
| `strategy.highWaterMark` | ✓ | ✓ |  |
| `strategy.size` | ✓ | ✓ |  |
| `class CountQueuingStrategy` | ✓ | ✓ |  |
| `new CountQueuingStrategy(init)` | ✓ | ✓ |  |
| `class TextEncoderStream` | ✓ | ✓ |  |
| `new TextEncoderStream()` | ✓ | ✓ |  |
| `textEncoderStream.encoding` | ✓ | ✓ |  |
| `textEncoderStream.readable` | ✓ | ✓ |  |
| `textEncoderStream.writable` | ✓ | ✓ |  |
| `class TextDecoderStream` | ✓ | ✓ |  |
| `new TextDecoderStream([encoding[, options]])` | ✓ | ✓ |  |
| `textDecoderStream.encoding` | ✓ | ✓ |  |
| `textDecoderStream.fatal` | ✓ | ✓ |  |
| `textDecoderStream.ignoreBOM` | ✓ | ✓ |  |
| `class CompressionStream` | ✓ | ✓ |  |
| `new CompressionStream(format)` | ✓ | ✓ | Formats: deflate / deflate-raw / gzip / brotli |
| `class DecompressionStream` | ✓ | ✓ |  |
| `new DecompressionStream(format)` | ✓ | ✓ |  |

---

### node:stream/consumers

Available via `require('node:stream/consumers')`. Functions to consume Readable streams into common formats.

#### Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `consumers.arrayBuffer(stream)` | ✓ | ✓ | Returns `Promise<ArrayBuffer>` |
| `consumers.blob(stream)` | ✓ | ✓ | Returns `Promise<Blob>` |
| `consumers.buffer(stream)` | ✓ | ✓ | Returns `Promise<Buffer>` |
| `consumers.bytes(stream)` | ✓ | ⚠ | Newer Node API — returns `Promise<Uint8Array>` |
| `consumers.json(stream)` | ✓ | ✓ | Returns `Promise<any>` |
| `consumers.text(stream)` | ✓ | ✓ | Returns `Promise<string>` |

---

### node:buffer

#### Static Methods (Buffer)
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Buffer.alloc(size[, fill[, encoding]])` | ✓ | ✓ |  |
| `Buffer.allocUnsafe(size)` | ✓ | ✓ |  |
| `Buffer.allocUnsafeSlow(size)` | ✓ | ✓ |  |
| `Buffer.byteLength(string[, encoding])` | ✓ | ✓ |  |
| `Buffer.compare(buf1, buf2)` | ✓ | ✓ |  |
| `Buffer.concat(list[, totalLength])` | ✓ | ✓ |  |
| `Buffer.copyBytesFrom(view[, offset[, length]])` | ✓ | ✓ |  |
| `Buffer.from(array)` | ✓ | ✓ |  |
| `Buffer.from(arrayBuffer[, byteOffset[, length]])` | ✓ | ✓ |  |
| `Buffer.from(buffer)` | ✓ | ✓ |  |
| `Buffer.from(object[, offsetOrEncoding[, length]])` | ✓ | ✓ |  |
| `Buffer.from(string[, encoding])` | ✓ | ✓ |  |
| `Buffer.isBuffer(obj)` | ✓ | ✓ |  |
| `Buffer.isEncoding(encoding)` | ✓ | ✓ |  |

#### Static Properties (Buffer)
| Property | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `Buffer.poolSize` | ✓ | ✓ |  |

#### Instance Methods (Buffer) — Read
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `buf.readBigInt64BE([offset])` | ✓ | ✓ |  |
| `buf.readBigInt64LE([offset])` | ✓ | ✓ |  |
| `buf.readBigUInt64BE([offset])` | ✓ | ✓ |  |
| `buf.readBigUInt64LE([offset])` | ✓ | ✓ |  |
| `buf.readDoubleBE([offset])` | ✓ | ✓ |  |
| `buf.readDoubleLE([offset])` | ✓ | ✓ |  |
| `buf.readFloatBE([offset])` | ✓ | ✓ |  |
| `buf.readFloatLE([offset])` | ✓ | ✓ |  |
| `buf.readInt8([offset])` | ✓ | ✓ |  |
| `buf.readInt16BE([offset])` | ✓ | ✓ |  |
| `buf.readInt16LE([offset])` | ✓ | ✓ |  |
| `buf.readInt32BE([offset])` | ✓ | ✓ |  |
| `buf.readInt32LE([offset])` | ✓ | ✓ |  |
| `buf.readIntBE(offset, byteLength)` | ✓ | ✓ |  |
| `buf.readIntLE(offset, byteLength)` | ✓ | ✓ |  |
| `buf.readUInt8([offset])` | ✓ | ✓ |  |
| `buf.readUInt16BE([offset])` | ✓ | ✓ |  |
| `buf.readUInt16LE([offset])` | ✓ | ✓ |  |
| `buf.readUInt32BE([offset])` | ✓ | ✓ |  |
| `buf.readUInt32LE([offset])` | ✓ | ✓ |  |
| `buf.readUIntBE(offset, byteLength)` | ✓ | ✓ |  |
| `buf.readUIntLE(offset, byteLength)` | ✓ | ✓ |  |

#### Instance Methods (Buffer) — Write
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `buf.writeBigInt64BE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeBigInt64LE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeBigUInt64BE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeBigUInt64LE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeDoubleBE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeDoubleLE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeFloatBE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeFloatLE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeInt8(value[, offset])` | ✓ | ✓ |  |
| `buf.writeInt16BE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeInt16LE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeInt32BE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeInt32LE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeIntBE(value, offset, byteLength)` | ✓ | ✓ |  |
| `buf.writeIntLE(value, offset, byteLength)` | ✓ | ✓ |  |
| `buf.writeUInt8(value[, offset])` | ✓ | ✓ |  |
| `buf.writeUInt16BE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeUInt16LE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeUInt32BE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeUInt32LE(value[, offset])` | ✓ | ✓ |  |
| `buf.writeUIntBE(value, offset, byteLength)` | ✓ | ✓ |  |
| `buf.writeUIntLE(value, offset, byteLength)` | ✓ | ✓ |  |
| `buf.write(string[, offset[, length]][, encoding])` | ✓ | ✓ |  |

#### Instance Methods (Buffer) — Utility
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `buf.compare(target[, targetStart[, targetEnd[, sourceStart[, sourceEnd]]]])` | ✓ | ✓ |  |
| `buf.copy(target[, targetStart[, sourceStart[, sourceEnd]]])` | ✓ | ✓ |  |
| `buf.entries()` | ✓ | ✓ |  |
| `buf.equals(otherBuffer)` | ✓ | ✓ |  |
| `buf.fill(value[, offset[, end]][, encoding])` | ✓ | ✓ |  |
| `buf.includes(value[, start[, end]][, encoding])` | ✓ | ✓ |  |
| `buf.indexOf(value[, start[, end]][, encoding])` | ✓ | ✓ |  |
| `buf.keys()` | ✓ | ✓ |  |
| `buf.lastIndexOf(value[, start[, end]][, encoding])` | ✓ | ✓ |  |
| `buf.slice([start[, end]])` | ✓ | ✓ | Deprecated — use subarray |
| `buf.subarray([start[, end]])` | ✓ | ✓ |  |
| `buf.swap16()` | ✓ | ✓ |  |
| `buf.swap32()` | ✓ | ✓ |  |
| `buf.swap64()` | ✓ | ✓ |  |
| `buf.toJSON()` | ✓ | ✓ |  |
| `buf.toString([encoding[, start[, end]]])` | ✓ | ✓ |  |
| `buf.values()` | ✓ | ✓ |  |

#### Instance Properties (Buffer)
| Property | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `buf[index]` | ✓ | ✓ |  |
| `buf.buffer` | ✓ | ✓ | Underlying ArrayBuffer |
| `buf.byteOffset` | ✓ | ✓ |  |
| `buf.length` | ✓ | ✓ |  |
| `buf.parent` | ✓ | ✓ | Deprecated |

#### Class: Blob
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class buffer.Blob` | ✓ | ✓ |  |
| `new buffer.Blob([sources[, options]])` | ✓ | ✓ |  |
| `blob.arrayBuffer()` | ✓ | ✓ |  |
| `blob.bytes()` | ✓ | ✓ |  |
| `blob.slice([start[, end[, type]]])` | ✓ | ✓ |  |
| `blob.stream()` | ✓ | ✓ |  |
| `blob.text()` | ✓ | ✓ |  |
| `blob.size` | ✓ | ✓ |  |
| `blob.type` | ✓ | ✓ |  |

#### Class: File
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class buffer.File` | ✓ | ✓ |  |
| `new buffer.File(sources, fileName[, options])` | ✓ | ✓ |  |
| `file.name` | ✓ | ✓ |  |
| `file.lastModified` | ✓ | ✓ |  |

#### Module-Level Functions
| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `buffer.atob(data)` | ✓ | ✓ |  |
| `buffer.btoa(data)` | ✓ | ✓ |  |
| `buffer.isAscii(input)` | ✓ | ✓ |  |
| `buffer.isUtf8(input)` | ✓ | ✓ |  |
| `buffer.resolveObjectURL(id)` | ✓ | ✓ |  |
| `buffer.transcode(source, fromEnc, toEnc)` | ✓ | ✓ |  |

#### Module-Level Constants
| Constant | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `buffer.INSPECT_MAX_BYTES` | ✓ | ✓ |  |
| `buffer.kMaxLength` | ✓ | ✓ |  |
| `buffer.kStringMaxLength` | ✓ | ✓ |  |
| `buffer.constants.MAX_LENGTH` | ✓ | ✓ |  |
| `buffer.constants.MAX_STRING_LENGTH` | ✓ | ✓ |  |

#### Supported Encodings (Buffer / Strings)
| Encoding | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `'utf8'` / `'utf-8'` | ✓ | ✓ |  |
| `'utf16le'` / `'utf-16le'` | ✓ | ✓ |  |
| `'latin1'` / `'binary'` | ✓ | ✓ |  |
| `'base64'` | ✓ | ✓ |  |
| `'base64url'` | ✓ | ✓ | RFC 4648 |
| `'hex'` | ✓ | ✓ |  |
| `'ascii'` | ✓ | ✓ | Legacy |
| `'ucs2'` / `'ucs-2'` | ✓ | ✓ | Aliases of utf16le |
## Node.js + Bun Runtime API Surface — Parity Inventory (Part 2)

Leaf-level inventory of Node.js native modules NOT covered in Part 1.
Bun column reflects status as documented at https://bun.sh/docs/runtime/nodejs-apis (compatibility with Node.js v23).

Legend: ✓ supported · ✗ not supported · ⚠ partial/with caveats

---

### node:events

Bun status: 🟢 Fully implemented. 100% of Node.js's test suite passes.

#### Classes

| Class / Method | Node.js | Bun | Notes |
|----------------|---------|-----|-------|
| `class EventEmitter` | ✓ | ✓ |  |
| `new EventEmitter([options])` (`captureRejections`) | ✓ | ✓ |  |
| `EventEmitter.prototype.addListener(eventName, listener)` | ✓ | ✓ | alias of `on` |
| `EventEmitter.prototype.emit(eventName[, ...args])` | ✓ | ✓ |  |
| `EventEmitter.prototype.eventNames()` | ✓ | ✓ |  |
| `EventEmitter.prototype.getMaxListeners()` | ✓ | ✓ |  |
| `EventEmitter.prototype.listenerCount(eventName[, listener])` | ✓ | ✓ |  |
| `EventEmitter.prototype.listeners(eventName)` | ✓ | ✓ |  |
| `EventEmitter.prototype.off(eventName, listener)` | ✓ | ✓ |  |
| `EventEmitter.prototype.on(eventName, listener)` | ✓ | ✓ |  |
| `EventEmitter.prototype.once(eventName, listener)` | ✓ | ✓ |  |
| `EventEmitter.prototype.prependListener(eventName, listener)` | ✓ | ✓ |  |
| `EventEmitter.prototype.prependOnceListener(eventName, listener)` | ✓ | ✓ |  |
| `EventEmitter.prototype.rawListeners(eventName)` | ✓ | ✓ |  |
| `EventEmitter.prototype.removeAllListeners([eventName])` | ✓ | ✓ |  |
| `EventEmitter.prototype.removeListener(eventName, listener)` | ✓ | ✓ |  |
| `EventEmitter.prototype.setMaxListeners(n)` | ✓ | ✓ |  |
| `EventEmitter.prototype[Symbol.for('nodejs.rejection')]()` | ✓ | ✓ |  |
| `class EventEmitterAsyncResource` | ✓ | ⚠ | Bun uses `AsyncResource` underneath |
| `EventEmitterAsyncResource.prototype.asyncId` | ✓ | ⚠ |  |
| `EventEmitterAsyncResource.prototype.asyncResource` | ✓ | ⚠ |  |
| `EventEmitterAsyncResource.prototype.triggerAsyncId` | ✓ | ⚠ |  |
| `EventEmitterAsyncResource.prototype.emitDestroy()` | ✓ | ⚠ |  |
| `class Event` | ✓ | ✓ | Web API |
| `Event.prototype.composedPath()` | ✓ | ✓ |  |
| `Event.prototype.initEvent(type, bubbles, cancelable)` | ✓ | ✓ | legacy |
| `Event.prototype.preventDefault()` | ✓ | ✓ |  |
| `Event.prototype.stopImmediatePropagation()` | ✓ | ✓ |  |
| `Event.prototype.stopPropagation()` | ✓ | ✓ |  |
| `class CustomEvent` | ✓ | ✓ |  |
| `class EventTarget` | ✓ | ✓ |  |
| `EventTarget.prototype.addEventListener(type, listener[, options])` | ✓ | ✓ |  |
| `EventTarget.prototype.dispatchEvent(event)` | ✓ | ✓ |  |
| `EventTarget.prototype.removeEventListener(type, listener[, options])` | ✓ | ✓ |  |
| `class NodeEventTarget` | ✓ | ✓ |  |

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `events.once(emitter, name[, options])` | ✓ | ✓ |  |
| `events.on(emitter, eventName[, options])` | ✓ | ✓ | AsyncIterator |
| `events.getEventListeners(emitterOrTarget, eventName)` | ✓ | ✓ |  |
| `events.getMaxListeners(emitterOrTarget)` | ✓ | ✓ |  |
| `events.setMaxListeners(n[, ...eventTargets])` | ✓ | ✓ |  |
| `events.listenerCount(emitterOrTarget, eventName)` | ✓ | ✓ |  |
| `events.addAbortListener(signal, listener)` | ✓ | ✓ | returns Disposable |

#### Constants / Properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `events.defaultMaxListeners` | ✓ | ✓ | default 10 |
| `events.errorMonitor` | ✓ | ✓ | symbol |
| `events.captureRejections` | ✓ | ✓ |  |
| `events.captureRejectionSymbol` | ✓ | ✓ |  |

#### Events emitted by EventEmitter

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'newListener'` | ✓ | ✓ |  |
| `'removeListener'` | ✓ | ✓ |  |

---

### node:util

Bun status: 🟡 Missing `getCallSite`, `getCallSites`, `getSystemErrorMap`, `getSystemErrorMessage`, `transferableAbortSignal`, `transferableAbortController`.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `util.callbackify(original)` | ✓ | ✓ |  |
| `util.debuglog(section[, callback])` | ✓ | ✓ |  |
| `util.debug(section)` | ✓ | ✓ | alias of `debuglog` |
| `util.deprecate(fn, msg[, code[, options]])` | ✓ | ✓ |  |
| `util.format(format[, ...args])` | ✓ | ✓ |  |
| `util.formatWithOptions(inspectOptions, format[, ...args])` | ✓ | ✓ |  |
| `util.getCallSites([frameCount][, options])` | ✓ | ✗ | missing in Bun |
| `util.getSystemErrorName(err)` | ✓ | ✓ |  |
| `util.getSystemErrorMap()` | ✓ | ✗ | missing in Bun |
| `util.getSystemErrorMessage(err)` | ✓ | ✗ | missing in Bun |
| `util.inherits(constructor, superConstructor)` | ✓ | ✓ | legacy |
| `util.inspect(object[, options])` | ✓ | ✓ |  |
| `util.isDeepStrictEqual(val1, val2[, options])` | ✓ | ✓ |  |
| `util.parseArgs([config])` | ✓ | ✓ |  |
| `util.parseEnv(content)` | ✓ | ⚠ | newer API |
| `util.promisify(original)` | ✓ | ✓ |  |
| `util.stripVTControlCharacters(str)` | ✓ | ✓ |  |
| `util.styleText(format, text[, options])` | ✓ | ✓ |  |
| `util.toUSVString(string)` | ✓ | ✓ |  |
| `util.transferableAbortController()` | ✓ | ✗ | missing in Bun |
| `util.transferableAbortSignal(signal)` | ✓ | ✗ | missing in Bun |
| `util.aborted(signal, resource)` | ✓ | ✓ |  |
| `util.convertProcessSignalToExitCode(signal)` | ✓ | ⚠ | v25.4.0 |
| `util.diff(actual, expected)` | ✓ | ⚠ | experimental v23.11.0 |
| `util.setTraceSigInt(enable)` | ✓ | ⚠ | v24.6.0 |

#### Classes

| Class / Method | Node.js | Bun | Notes |
|----------------|---------|-----|-------|
| `class util.MIMEType` | ✓ | ✓ |  |
| `MIMEType.prototype.type` | ✓ | ✓ |  |
| `MIMEType.prototype.subtype` | ✓ | ✓ |  |
| `MIMEType.prototype.essence` | ✓ | ✓ |  |
| `MIMEType.prototype.params` | ✓ | ✓ |  |
| `MIMEType.prototype.toString()` | ✓ | ✓ |  |
| `MIMEType.prototype.toJSON()` | ✓ | ✓ |  |
| `class util.MIMEParams` | ✓ | ✓ |  |
| `MIMEParams.prototype.delete(name)` | ✓ | ✓ |  |
| `MIMEParams.prototype.entries()` | ✓ | ✓ |  |
| `MIMEParams.prototype.get(name)` | ✓ | ✓ |  |
| `MIMEParams.prototype.has(name)` | ✓ | ✓ |  |
| `MIMEParams.prototype.keys()` | ✓ | ✓ |  |
| `MIMEParams.prototype.set(name, value)` | ✓ | ✓ |  |
| `MIMEParams.prototype.values()` | ✓ | ✓ |  |
| `class util.TextDecoder` | ✓ | ✓ | WHATWG |
| `class util.TextEncoder` | ✓ | ✓ | WHATWG |

#### util.inspect properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `util.inspect.custom` | ✓ | ✓ | symbol |
| `util.inspect.defaultOptions` | ✓ | ✓ |  |
| `util.inspect.styles` | ✓ | ✓ |  |
| `util.inspect.colors` | ✓ | ✓ |  |
| `util.promisify.custom` | ✓ | ✓ | symbol |

#### util.types predicates

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `util.types.isAnyArrayBuffer(value)` | ✓ | ✓ |  |
| `util.types.isArrayBuffer(value)` | ✓ | ✓ |  |
| `util.types.isArrayBufferView(value)` | ✓ | ✓ |  |
| `util.types.isArgumentsObject(value)` | ✓ | ✓ |  |
| `util.types.isAsyncFunction(value)` | ✓ | ✓ |  |
| `util.types.isBigInt64Array(value)` | ✓ | ✓ |  |
| `util.types.isBigUint64Array(value)` | ✓ | ✓ |  |
| `util.types.isBooleanObject(value)` | ✓ | ✓ |  |
| `util.types.isBoxedPrimitive(value)` | ✓ | ✓ |  |
| `util.types.isCryptoKey(value)` | ✓ | ✓ |  |
| `util.types.isDataView(value)` | ✓ | ✓ |  |
| `util.types.isDate(value)` | ✓ | ✓ |  |
| `util.types.isExternal(value)` | ✓ | ✓ |  |
| `util.types.isFloat16Array(value)` | ✓ | ⚠ | v22.0.0 |
| `util.types.isFloat32Array(value)` | ✓ | ✓ |  |
| `util.types.isFloat64Array(value)` | ✓ | ✓ |  |
| `util.types.isGeneratorFunction(value)` | ✓ | ✓ |  |
| `util.types.isGeneratorObject(value)` | ✓ | ✓ |  |
| `util.types.isInt8Array(value)` | ✓ | ✓ |  |
| `util.types.isInt16Array(value)` | ✓ | ✓ |  |
| `util.types.isInt32Array(value)` | ✓ | ✓ |  |
| `util.types.isKeyObject(value)` | ✓ | ✓ |  |
| `util.types.isMap(value)` | ✓ | ✓ |  |
| `util.types.isMapIterator(value)` | ✓ | ✓ |  |
| `util.types.isModuleNamespaceObject(value)` | ✓ | ✓ |  |
| `util.types.isNativeError(value)` | ✓ | ✓ |  |
| `util.types.isNumberObject(value)` | ✓ | ✓ |  |
| `util.types.isPromise(value)` | ✓ | ✓ |  |
| `util.types.isProxy(value)` | ✓ | ✓ |  |
| `util.types.isRegExp(value)` | ✓ | ✓ |  |
| `util.types.isSet(value)` | ✓ | ✓ |  |
| `util.types.isSetIterator(value)` | ✓ | ✓ |  |
| `util.types.isSharedArrayBuffer(value)` | ✓ | ✓ |  |
| `util.types.isStringObject(value)` | ✓ | ✓ |  |
| `util.types.isSymbolObject(value)` | ✓ | ✓ |  |
| `util.types.isTypedArray(value)` | ✓ | ✓ |  |
| `util.types.isUint8Array(value)` | ✓ | ✓ |  |
| `util.types.isUint8ClampedArray(value)` | ✓ | ✓ |  |
| `util.types.isUint16Array(value)` | ✓ | ✓ |  |
| `util.types.isUint32Array(value)` | ✓ | ✓ |  |
| `util.types.isWeakMap(value)` | ✓ | ✓ |  |
| `util.types.isWeakSet(value)` | ✓ | ✓ |  |

#### Deprecated APIs

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `util._extend(target, source)` | ✓ | ✓ | deprecated |
| `util.isArray(object)` | ✓ | ✓ | deprecated, use `Array.isArray` |
| `util.isBoolean(object)` | ✓ | ✓ | deprecated |
| `util.isBuffer(object)` | ✓ | ✓ | deprecated, use `Buffer.isBuffer` |
| `util.isDate(object)` | ✓ | ✓ | deprecated |
| `util.isError(object)` | ✓ | ✓ | deprecated |
| `util.isFunction(object)` | ✓ | ✓ | deprecated |
| `util.isNull(object)` | ✓ | ✓ | deprecated |
| `util.isNullOrUndefined(object)` | ✓ | ✓ | deprecated |
| `util.isNumber(object)` | ✓ | ✓ | deprecated |
| `util.isObject(object)` | ✓ | ✓ | deprecated |
| `util.isPrimitive(object)` | ✓ | ✓ | deprecated |
| `util.isRegExp(object)` | ✓ | ✓ | deprecated |
| `util.isString(object)` | ✓ | ✓ | deprecated |
| `util.isSymbol(object)` | ✓ | ✓ | deprecated |
| `util.isUndefined(object)` | ✓ | ✓ | deprecated |
| `util.log(string)` | ✓ | ✓ | deprecated |
| `util.print(...args)` | ✓ | ✓ | deprecated |
| `util.puts(...args)` | ✓ | ✓ | deprecated |
| `util.error(...args)` | ✓ | ✓ | deprecated |

---

### node:sys

Bun status: 🟡 alias for node:util. Same status as node:util.

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `node:sys` exports | ✓ | ⚠ | deprecated; alias for `node:util`; same gaps |

---

### node:os

Bun status: 🟢 Fully implemented. 100% of Node.js's test suite passes.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `os.arch()` | ✓ | ✓ |  |
| `os.availableParallelism()` | ✓ | ✓ |  |
| `os.cpus()` | ✓ | ✓ |  |
| `os.endianness()` | ✓ | ✓ |  |
| `os.freemem()` | ✓ | ✓ |  |
| `os.getPriority([pid])` | ✓ | ✓ |  |
| `os.homedir()` | ✓ | ✓ |  |
| `os.hostname()` | ✓ | ✓ |  |
| `os.loadavg()` | ✓ | ✓ | Windows returns `[0,0,0]` |
| `os.machine()` | ✓ | ✓ |  |
| `os.networkInterfaces()` | ✓ | ✓ |  |
| `os.platform()` | ✓ | ✓ |  |
| `os.release()` | ✓ | ✓ |  |
| `os.setPriority([pid, ]priority)` | ✓ | ✓ |  |
| `os.tmpdir()` | ✓ | ✓ |  |
| `os.totalmem()` | ✓ | ✓ |  |
| `os.type()` | ✓ | ✓ |  |
| `os.uptime()` | ✓ | ✓ |  |
| `os.userInfo([options])` | ✓ | ✓ |  |
| `os.version()` | ✓ | ✓ |  |

#### Properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `os.EOL` | ✓ | ✓ |  |
| `os.devNull` | ✓ | ✓ |  |
| `os.constants` | ✓ | ✓ |  |

#### Signal Constants (`os.constants.signals`)

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `SIGHUP` | ✓ | ✓ |  |
| `SIGINT` | ✓ | ✓ |  |
| `SIGQUIT` | ✓ | ✓ |  |
| `SIGILL` | ✓ | ✓ |  |
| `SIGTRAP` | ✓ | ✓ |  |
| `SIGABRT` | ✓ | ✓ |  |
| `SIGIOT` | ✓ | ✓ |  |
| `SIGBUS` | ✓ | ✓ |  |
| `SIGFPE` | ✓ | ✓ |  |
| `SIGKILL` | ✓ | ✓ |  |
| `SIGUSR1` | ✓ | ✓ |  |
| `SIGUSR2` | ✓ | ✓ |  |
| `SIGSEGV` | ✓ | ✓ |  |
| `SIGPIPE` | ✓ | ✓ |  |
| `SIGALRM` | ✓ | ✓ |  |
| `SIGTERM` | ✓ | ✓ |  |
| `SIGCHLD` | ✓ | ✓ |  |
| `SIGSTKFLT` | ✓ | ✓ |  |
| `SIGCONT` | ✓ | ✓ |  |
| `SIGSTOP` | ✓ | ✓ |  |
| `SIGTSTP` | ✓ | ✓ |  |
| `SIGBREAK` | ✓ | ✓ | Windows |
| `SIGTTIN` | ✓ | ✓ |  |
| `SIGTTOU` | ✓ | ✓ |  |
| `SIGURG` | ✓ | ✓ |  |
| `SIGXCPU` | ✓ | ✓ |  |
| `SIGXFSZ` | ✓ | ✓ |  |
| `SIGVTALRM` | ✓ | ✓ |  |
| `SIGPROF` | ✓ | ✓ |  |
| `SIGWINCH` | ✓ | ✓ |  |
| `SIGIO` | ✓ | ✓ |  |
| `SIGPOLL` | ✓ | ✓ |  |
| `SIGLOST` | ✓ | ✓ |  |
| `SIGPWR` | ✓ | ✓ |  |
| `SIGINFO` | ✓ | ✓ |  |
| `SIGSYS` | ✓ | ✓ |  |
| `SIGUNUSED` | ✓ | ✓ |  |

#### POSIX Error Constants (`os.constants.errno`)

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `E2BIG` | ✓ | ✓ |  |
| `EACCES` | ✓ | ✓ |  |
| `EADDRINUSE` | ✓ | ✓ |  |
| `EADDRNOTAVAIL` | ✓ | ✓ |  |
| `EAFNOSUPPORT` | ✓ | ✓ |  |
| `EAGAIN` | ✓ | ✓ |  |
| `EALREADY` | ✓ | ✓ |  |
| `EBADF` | ✓ | ✓ |  |
| `EBADMSG` | ✓ | ✓ |  |
| `EBUSY` | ✓ | ✓ |  |
| `ECANCELED` | ✓ | ✓ |  |
| `ECHILD` | ✓ | ✓ |  |
| `ECONNABORTED` | ✓ | ✓ |  |
| `ECONNREFUSED` | ✓ | ✓ |  |
| `ECONNRESET` | ✓ | ✓ |  |
| `EDEADLK` | ✓ | ✓ |  |
| `EDESTADDRREQ` | ✓ | ✓ |  |
| `EDOM` | ✓ | ✓ |  |
| `EDQUOT` | ✓ | ✓ |  |
| `EEXIST` | ✓ | ✓ |  |
| `EFAULT` | ✓ | ✓ |  |
| `EFBIG` | ✓ | ✓ |  |
| `EHOSTUNREACH` | ✓ | ✓ |  |
| `EIDRM` | ✓ | ✓ |  |
| `EILSEQ` | ✓ | ✓ |  |
| `EINPROGRESS` | ✓ | ✓ |  |
| `EINTR` | ✓ | ✓ |  |
| `EINVAL` | ✓ | ✓ |  |
| `EIO` | ✓ | ✓ |  |
| `EISCONN` | ✓ | ✓ |  |
| `EISDIR` | ✓ | ✓ |  |
| `ELOOP` | ✓ | ✓ |  |
| `EMFILE` | ✓ | ✓ |  |
| `EMLINK` | ✓ | ✓ |  |
| `EMSGSIZE` | ✓ | ✓ |  |
| `EMULTIHOP` | ✓ | ✓ |  |
| `ENAMETOOLONG` | ✓ | ✓ |  |
| `ENETDOWN` | ✓ | ✓ |  |
| `ENETRESET` | ✓ | ✓ |  |
| `ENETUNREACH` | ✓ | ✓ |  |
| `ENFILE` | ✓ | ✓ |  |
| `ENOBUFS` | ✓ | ✓ |  |
| `ENODATA` | ✓ | ✓ |  |
| `ENODEV` | ✓ | ✓ |  |
| `ENOENT` | ✓ | ✓ |  |
| `ENOEXEC` | ✓ | ✓ |  |
| `ENOLCK` | ✓ | ✓ |  |
| `ENOLINK` | ✓ | ✓ |  |
| `ENOMEM` | ✓ | ✓ |  |
| `ENOMSG` | ✓ | ✓ |  |
| `ENOPROTOOPT` | ✓ | ✓ |  |
| `ENOSPC` | ✓ | ✓ |  |
| `ENOSR` | ✓ | ✓ |  |
| `ENOSTR` | ✓ | ✓ |  |
| `ENOSYS` | ✓ | ✓ |  |
| `ENOTCONN` | ✓ | ✓ |  |
| `ENOTDIR` | ✓ | ✓ |  |
| `ENOTEMPTY` | ✓ | ✓ |  |
| `ENOTSOCK` | ✓ | ✓ |  |
| `ENOTSUP` | ✓ | ✓ |  |
| `ENOTTY` | ✓ | ✓ |  |
| `ENXIO` | ✓ | ✓ |  |
| `EOPNOTSUPP` | ✓ | ✓ |  |
| `EOVERFLOW` | ✓ | ✓ |  |
| `EPERM` | ✓ | ✓ |  |
| `EPIPE` | ✓ | ✓ |  |
| `EPROTO` | ✓ | ✓ |  |
| `EPROTONOSUPPORT` | ✓ | ✓ |  |
| `EPROTOTYPE` | ✓ | ✓ |  |
| `ERANGE` | ✓ | ✓ |  |
| `EROFS` | ✓ | ✓ |  |
| `ESPIPE` | ✓ | ✓ |  |
| `ESRCH` | ✓ | ✓ |  |
| `ESTALE` | ✓ | ✓ |  |
| `ETIME` | ✓ | ✓ |  |
| `ETIMEDOUT` | ✓ | ✓ |  |
| `ETXTBSY` | ✓ | ✓ |  |
| `EWOULDBLOCK` | ✓ | ✓ |  |
| `EXDEV` | ✓ | ✓ |  |

#### Windows-specific Error Constants (`os.constants.errno`)

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `WSAEINTR` | ✓ | ✓ | Windows only |
| `WSAEBADF` | ✓ | ✓ |  |
| `WSAEACCES` | ✓ | ✓ |  |
| `WSAEFAULT` | ✓ | ✓ |  |
| `WSAEINVAL` | ✓ | ✓ |  |
| `WSAEMFILE` | ✓ | ✓ |  |
| `WSAEWOULDBLOCK` | ✓ | ✓ |  |
| `WSAEINPROGRESS` | ✓ | ✓ |  |
| `WSAEALREADY` | ✓ | ✓ |  |
| `WSAENOTSOCK` | ✓ | ✓ |  |
| `WSAEDESTADDRREQ` | ✓ | ✓ |  |
| `WSAEMSGSIZE` | ✓ | ✓ |  |
| `WSAEPROTOTYPE` | ✓ | ✓ |  |
| `WSAENOPROTOOPT` | ✓ | ✓ |  |
| `WSAEPROTONOSUPPORT` | ✓ | ✓ |  |
| `WSAESOCKTNOSUPPORT` | ✓ | ✓ |  |
| `WSAEOPNOTSUPP` | ✓ | ✓ |  |
| `WSAEPFNOSUPPORT` | ✓ | ✓ |  |
| `WSAEAFNOSUPPORT` | ✓ | ✓ |  |
| `WSAEADDRINUSE` | ✓ | ✓ |  |
| `WSAEADDRNOTAVAIL` | ✓ | ✓ |  |
| `WSAENETDOWN` | ✓ | ✓ |  |
| `WSAENETUNREACH` | ✓ | ✓ |  |
| `WSAENETRESET` | ✓ | ✓ |  |
| `WSAECONNABORTED` | ✓ | ✓ |  |
| `WSAECONNRESET` | ✓ | ✓ |  |
| `WSAENOBUFS` | ✓ | ✓ |  |
| `WSAEISCONN` | ✓ | ✓ |  |
| `WSAENOTCONN` | ✓ | ✓ |  |
| `WSAESHUTDOWN` | ✓ | ✓ |  |
| `WSAETOOMANYREFS` | ✓ | ✓ |  |
| `WSAETIMEDOUT` | ✓ | ✓ |  |
| `WSAECONNREFUSED` | ✓ | ✓ |  |
| `WSAELOOP` | ✓ | ✓ |  |
| `WSAENAMETOOLONG` | ✓ | ✓ |  |
| `WSAEHOSTDOWN` | ✓ | ✓ |  |
| `WSAEHOSTUNREACH` | ✓ | ✓ |  |
| `WSAENOTEMPTY` | ✓ | ✓ |  |
| `WSAEPROCLIM` | ✓ | ✓ |  |
| `WSAEUSERS` | ✓ | ✓ |  |
| `WSAEDQUOT` | ✓ | ✓ |  |
| `WSAESTALE` | ✓ | ✓ |  |
| `WSAEREMOTE` | ✓ | ✓ |  |
| `WSASYSNOTREADY` | ✓ | ✓ |  |
| `WSAVERNOTSUPPORTED` | ✓ | ✓ |  |
| `WSANOTINITIALISED` | ✓ | ✓ |  |
| `WSAEDISCON` | ✓ | ✓ |  |
| `WSAENOMORE` | ✓ | ✓ |  |
| `WSAECANCELLED` | ✓ | ✓ |  |
| `WSAEINVALIDPROCTABLE` | ✓ | ✓ |  |
| `WSAEINVALIDPROVIDER` | ✓ | ✓ |  |
| `WSAEPROVIDERFAILEDINIT` | ✓ | ✓ |  |
| `WSASYSCALLFAILURE` | ✓ | ✓ |  |
| `WSASERVICE_NOT_FOUND` | ✓ | ✓ |  |
| `WSATYPE_NOT_FOUND` | ✓ | ✓ |  |
| `WSA_E_NO_MORE` | ✓ | ✓ |  |
| `WSA_E_CANCELLED` | ✓ | ✓ |  |
| `WSAEREFUSED` | ✓ | ✓ |  |

#### Priority Constants (`os.constants.priority`)

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `PRIORITY_LOW` | ✓ | ✓ |  |
| `PRIORITY_BELOW_NORMAL` | ✓ | ✓ |  |
| `PRIORITY_NORMAL` | ✓ | ✓ |  |
| `PRIORITY_ABOVE_NORMAL` | ✓ | ✓ |  |
| `PRIORITY_HIGH` | ✓ | ✓ |  |
| `PRIORITY_HIGHEST` | ✓ | ✓ |  |

#### dlopen Constants (`os.constants.dlopen`)

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `RTLD_LAZY` | ✓ | ✓ |  |
| `RTLD_NOW` | ✓ | ✓ |  |
| `RTLD_GLOBAL` | ✓ | ✓ |  |
| `RTLD_LOCAL` | ✓ | ✓ |  |
| `RTLD_DEEPBIND` | ✓ | ✓ |  |

#### libuv Constants

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `os.constants.UV_UDP_REUSEADDR` | ✓ | ✓ |  |

---

### node:process (and global `process`)

Bun status: 🟡 Mostly implemented. `process.binding` partial. `process.title` no-op on macOS/Linux. `getActiveResourcesInfo`, `setActiveResourcesInfo`, `getActiveResources`, `setSourceMapsEnabled` are stubs. `loadEnvFile`, `getBuiltinModule` not implemented.

#### Process Methods

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `process.abort()` | ✓ | ✓ |  |
| `process.exit([code])` | ✓ | ✓ |  |
| `process.execve(file[, args[, env]])` | ✓ | ⚠ |  |
| `process.chdir(directory)` | ✓ | ✓ |  |
| `process.cwd()` | ✓ | ✓ |  |
| `process.memoryUsage()` | ✓ | ✓ |  |
| `process.memoryUsage.rss()` | ✓ | ✓ |  |
| `process.availableMemory()` | ✓ | ✓ |  |
| `process.constrainedMemory()` | ✓ | ✓ |  |
| `process.cpuUsage([previousValue])` | ✓ | ✓ |  |
| `process.threadCpuUsage([previousValue])` | ✓ | ⚠ |  |
| `process.resourceUsage()` | ✓ | ✓ |  |
| `process.getActiveResourcesInfo()` | ✓ | ⚠ | stub in Bun |
| `process.uptime()` | ✓ | ✓ |  |
| `process.getuid()` | ✓ | ✓ | POSIX |
| `process.geteuid()` | ✓ | ✓ | POSIX |
| `process.setuid(id)` | ✓ | ✓ | POSIX |
| `process.seteuid(id)` | ✓ | ✓ | POSIX |
| `process.getgid()` | ✓ | ✓ | POSIX |
| `process.getegid()` | ✓ | ✓ | POSIX |
| `process.setgid(id)` | ✓ | ✓ | POSIX |
| `process.setegid(id)` | ✓ | ✓ | POSIX |
| `process.getgroups()` | ✓ | ✓ | POSIX |
| `process.setgroups(groups)` | ✓ | ✓ | POSIX |
| `process.initgroups(user, extraGroup)` | ✓ | ⚠ |  |
| `process.kill(pid[, signal])` | ✓ | ✓ |  |
| `process.send(message[, sendHandle[, options]][, callback])` | ✓ | ⚠ | IPC; see child_process notes |
| `process.disconnect()` | ✓ | ✓ |  |
| `process.channel` | ✓ | ⚠ |  |
| `process.channel.ref()` | ✓ | ⚠ |  |
| `process.channel.unref()` | ✓ | ⚠ |  |
| `process.emitWarning(warning[, options])` | ✓ | ✓ |  |
| `process.setUncaughtExceptionCaptureCallback(fn)` | ✓ | ✓ |  |
| `process.addUncaughtExceptionCaptureCallback(fn)` | ✓ | ⚠ |  |
| `process.hasUncaughtExceptionCaptureCallback()` | ✓ | ✓ |  |
| `process.dlopen(module, filename[, flags])` | ✓ | ✓ |  |
| `process.getBuiltinModule(id)` | ✓ | ✗ | not implemented in Bun |
| `process.loadEnvFile(path)` | ✓ | ✗ | not implemented in Bun |
| `process.setSourceMapsEnabled(val)` | ✓ | ⚠ | stub |
| `process.hrtime([time])` | ✓ | ✓ |  |
| `process.hrtime.bigint()` | ✓ | ✓ |  |
| `process.permission.has(scope[, reference])` | ✓ | ⚠ |  |
| `process.umask()` | ✓ | ✓ |  |
| `process.umask(mask)` | ✓ | ✓ |  |
| `process.finalization.register(ref, callback)` | ✓ | ✓ |  |
| `process.finalization.registerBeforeExit(ref, callback)` | ✓ | ✓ |  |
| `process.finalization.unregister(ref)` | ✓ | ✓ |  |
| `process.nextTick(callback[, ...args])` | ✓ | ✓ |  |
| `process.ref(maybeRefable)` | ✓ | ⚠ |  |
| `process.unref(maybeRefable)` | ✓ | ⚠ |  |
| `process.report.getReport([err])` | ✓ | ⚠ |  |
| `process.report.writeReport([filename][, err])` | ✓ | ⚠ |  |
| `process.binding(name)` | ✓ | ⚠ | partial; internal |

#### Process Properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `process.pid` | ✓ | ✓ |  |
| `process.ppid` | ✓ | ✓ |  |
| `process.platform` | ✓ | ✓ |  |
| `process.arch` | ✓ | ✓ |  |
| `process.version` | ✓ | ✓ |  |
| `process.versions` | ✓ | ✓ |  |
| `process.release` | ✓ | ✓ |  |
| `process.argv` | ✓ | ✓ |  |
| `process.argv0` | ✓ | ✓ |  |
| `process.execPath` | ✓ | ✓ |  |
| `process.execArgv` | ✓ | ✓ |  |
| `process.title` | ✓ | ⚠ | no-op on macOS/Linux |
| `process.mainModule` | ✓ | ✓ |  |
| `process.exitCode` | ✓ | ✓ |  |
| `process.connected` | ✓ | ✓ |  |
| `process.env` | ✓ | ✓ |  |
| `process.config` | ✓ | ✓ |  |
| `process.allowedNodeEnvironmentFlags` | ✓ | ✓ |  |
| `process.debugPort` | ✓ | ✓ |  |
| `process.features.cached_builtins` | ✓ | ✓ |  |
| `process.features.debug` | ✓ | ✓ |  |
| `process.features.inspector` | ✓ | ✓ |  |
| `process.features.ipv6` | ✓ | ✓ | deprecated |
| `process.features.tls` | ✓ | ✓ |  |
| `process.features.tls_alpn` | ✓ | ✓ | deprecated |
| `process.features.tls_ocsp` | ✓ | ✓ | deprecated |
| `process.features.tls_sni` | ✓ | ✓ | deprecated |
| `process.features.uv` | ✓ | ✓ | deprecated |
| `process.features.require_module` | ✓ | ✓ |  |
| `process.features.typescript` | ✓ | ✓ |  |
| `process.noDeprecation` | ✓ | ✓ |  |
| `process.throwDeprecation` | ✓ | ✓ |  |
| `process.traceDeprecation` | ✓ | ✓ |  |
| `process.traceProcessWarnings` | ✓ | ✓ |  |
| `process.stdin` | ✓ | ✓ |  |
| `process.stdout` | ✓ | ✓ |  |
| `process.stderr` | ✓ | ✓ |  |
| `process.report.compact` | ✓ | ⚠ |  |
| `process.report.directory` | ✓ | ⚠ |  |
| `process.report.filename` | ✓ | ⚠ |  |
| `process.report.reportOnFatalError` | ✓ | ⚠ |  |
| `process.report.reportOnSignal` | ✓ | ⚠ |  |
| `process.report.reportOnUncaughtException` | ✓ | ⚠ |  |
| `process.report.excludeEnv` | ✓ | ⚠ |  |
| `process.report.signal` | ✓ | ⚠ |  |
| `process.sourceMapsEnabled` | ✓ | ⚠ |  |

#### Process Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'beforeExit'` | ✓ | ✓ |  |
| `'exit'` | ✓ | ✓ |  |
| `'uncaughtException'` | ✓ | ✓ |  |
| `'uncaughtExceptionMonitor'` | ✓ | ✓ |  |
| `'unhandledRejection'` | ✓ | ✓ |  |
| `'rejectionHandled'` | ✓ | ✓ |  |
| `'message'` | ✓ | ✓ |  |
| `'disconnect'` | ✓ | ✓ |  |
| `'workerMessage'` | ✓ | ⚠ |  |
| `'warning'` | ✓ | ✓ |  |
| `'worker'` | ✓ | ⚠ |  |
| `'SIGINT'` | ✓ | ✓ |  |
| `'SIGTERM'` | ✓ | ✓ |  |
| `'SIGHUP'` | ✓ | ✓ |  |
| `'SIGUSR1'` | ✓ | ✓ | reserved for debugger |
| `'SIGUSR2'` | ✓ | ✓ |  |
| `'SIGWINCH'` | ✓ | ✓ |  |
| `'SIGPIPE'` | ✓ | ✓ |  |
| `'SIGBREAK'` | ✓ | ✓ | Windows |
| `'SIGBUS'` / `'SIGFPE'` / `'SIGSEGV'` / `'SIGILL'` | ✓ | ✓ | fatal — not safe to handle |

---

### node:child_process

Bun status: 🟡 Missing `proc.gid`, `proc.uid`. `Stream` class not exported. IPC cannot send socket handles. Node.js↔Bun IPC requires JSON serialization.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `child_process.spawn(command[, args][, options])` | ✓ | ✓ |  |
| `child_process.exec(command[, options][, callback])` | ✓ | ✓ |  |
| `child_process.execFile(file[, args][, options][, callback])` | ✓ | ✓ |  |
| `child_process.fork(modulePath[, args][, options])` | ✓ | ✓ |  |
| `child_process.spawnSync(command[, args][, options])` | ✓ | ✓ |  |
| `child_process.execSync(command[, options])` | ✓ | ✓ |  |
| `child_process.execFileSync(file[, args][, options])` | ✓ | ✓ |  |
| `util.promisify(exec)` | ✓ | ✓ |  |
| `util.promisify(execFile)` | ✓ | ✓ |  |

#### Class: ChildProcess

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `subprocess.channel` | ✓ | ✓ |  |
| `subprocess.connected` | ✓ | ✓ |  |
| `subprocess.exitCode` | ✓ | ✓ |  |
| `subprocess.killed` | ✓ | ✓ |  |
| `subprocess.pid` | ✓ | ✓ |  |
| `subprocess.signalCode` | ✓ | ✓ |  |
| `subprocess.spawnargs` | ✓ | ✓ |  |
| `subprocess.spawnfile` | ✓ | ✓ |  |
| `subprocess.stdin` | ✓ | ✓ |  |
| `subprocess.stdout` | ✓ | ✓ |  |
| `subprocess.stderr` | ✓ | ✓ |  |
| `subprocess.stdio` | ✓ | ✓ |  |
| `subprocess.uid` | ✓ | ✗ | missing in Bun |
| `subprocess.gid` | ✓ | ✗ | missing in Bun |
| `subprocess.kill([signal])` | ✓ | ✓ |  |
| `subprocess.send(message[, sendHandle[, options]][, callback])` | ✓ | ⚠ | cannot send socket handles |
| `subprocess.disconnect()` | ✓ | ✓ |  |
| `subprocess.ref()` | ✓ | ✓ |  |
| `subprocess.unref()` | ✓ | ✓ |  |
| `subprocess[Symbol.dispose]()` | ✓ | ✓ |  |

#### Events on ChildProcess

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'spawn'` | ✓ | ✓ |  |
| `'error'` | ✓ | ✓ |  |
| `'exit'` | ✓ | ✓ |  |
| `'close'` | ✓ | ✓ |  |
| `'disconnect'` | ✓ | ✓ |  |
| `'message'` | ✓ | ✓ |  |

#### Exports

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `child_process.ChildProcess` | ✓ | ✓ |  |
| `child_process.Stream` | ✓ | ✗ | not exported in Bun |

---

### node:cluster

Bun status: 🟡 Handles and file descriptors cannot be passed between workers. HTTP load-balancing across processes only on Linux (via SO_REUSEPORT). Otherwise implemented but not battle-tested.

#### Module-Level Properties / Methods

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `cluster.isPrimary` | ✓ | ✓ |  |
| `cluster.isMaster` | ✓ | ✓ | deprecated, alias of `isPrimary` |
| `cluster.isWorker` | ✓ | ✓ |  |
| `cluster.worker` | ✓ | ✓ | worker only |
| `cluster.workers` | ✓ | ✓ | primary only |
| `cluster.settings` | ✓ | ✓ |  |
| `cluster.schedulingPolicy` | ✓ | ✓ |  |
| `cluster.SCHED_RR` | ✓ | ✓ |  |
| `cluster.SCHED_NONE` | ✓ | ✓ |  |
| `cluster.fork([env])` | ✓ | ✓ |  |
| `cluster.disconnect([callback])` | ✓ | ✓ |  |
| `cluster.setupPrimary([settings])` | ✓ | ✓ |  |
| `cluster.setupMaster([settings])` | ✓ | ✓ | deprecated alias |

#### Cluster Module Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'fork'` | ✓ | ✓ |  |
| `'online'` | ✓ | ✓ |  |
| `'listening'` | ✓ | ✓ |  |
| `'message'` | ✓ | ✓ |  |
| `'disconnect'` | ✓ | ✓ |  |
| `'exit'` | ✓ | ✓ |  |
| `'setup'` | ✓ | ✓ |  |

#### Worker Class

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `worker.id` | ✓ | ✓ |  |
| `worker.process` | ✓ | ✓ |  |
| `worker.exitedAfterDisconnect` | ✓ | ✓ |  |
| `worker.send(message[, sendHandle[, options]][, callback])` | ✓ | ⚠ | no socket handles |
| `worker.disconnect()` | ✓ | ✓ |  |
| `worker.kill([signal])` | ✓ | ✓ |  |
| `worker.destroy()` | ✓ | ✓ | alias |
| `worker.isConnected()` | ✓ | ✓ |  |
| `worker.isDead()` | ✓ | ✓ |  |

#### Worker Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'online'` | ✓ | ✓ |  |
| `'disconnect'` | ✓ | ✓ |  |
| `'message'` | ✓ | ✓ |  |
| `'listening'` | ✓ | ✓ |  |
| `'exit'` | ✓ | ✓ |  |
| `'error'` | ✓ | ✓ |  |

---

### node:worker_threads

Bun status: 🟡 `Worker` doesn't support `stdin`, `stdout`, `stderr`, `trackedUnmanagedFds`, `resourceLimits`. Missing `markAsUntransferable`, `moveMessagePortToContext`.

#### Module-Level Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `worker_threads.getEnvironmentData(key)` | ✓ | ✓ |  |
| `worker_threads.setEnvironmentData(key[, value])` | ✓ | ✓ |  |
| `worker_threads.markAsUntransferable(object)` | ✓ | ✗ | missing in Bun |
| `worker_threads.isMarkedAsUntransferable(object)` | ✓ | ⚠ |  |
| `worker_threads.markAsUncloneable(object)` | ✓ | ⚠ |  |
| `worker_threads.moveMessagePortToContext(port, ctx)` | ✓ | ✗ | missing in Bun |
| `worker_threads.receiveMessageOnPort(port)` | ✓ | ✓ |  |
| `worker_threads.postMessageToThread(threadId, value[, transferList][, timeout])` | ✓ | ⚠ |  |

#### Module-Level Properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `worker_threads.isMainThread` | ✓ | ✓ |  |
| `worker_threads.isInternalThread` | ✓ | ⚠ |  |
| `worker_threads.parentPort` | ✓ | ✓ |  |
| `worker_threads.threadId` | ✓ | ✓ |  |
| `worker_threads.threadName` | ✓ | ⚠ |  |
| `worker_threads.workerData` | ✓ | ✓ |  |
| `worker_threads.resourceLimits` | ✓ | ⚠ |  |
| `worker_threads.SHARE_ENV` | ✓ | ✓ | symbol |
| `worker_threads.locks` | ✓ | ⚠ | experimental |

#### Class: Worker

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new Worker(filename[, options])` | ✓ | ✓ |  |
| option: `argv` | ✓ | ✓ |  |
| option: `env` | ✓ | ✓ |  |
| option: `eval` | ✓ | ✓ |  |
| option: `execArgv` | ✓ | ✓ |  |
| option: `stdin` | ✓ | ✗ | not supported in Bun |
| option: `stdout` | ✓ | ✗ | not supported in Bun |
| option: `stderr` | ✓ | ✗ | not supported in Bun |
| option: `workerData` | ✓ | ✓ |  |
| option: `trackUnmanagedFds` | ✓ | ✗ | not supported in Bun |
| option: `transferList` | ✓ | ✓ |  |
| option: `resourceLimits` | ✓ | ✗ | not supported in Bun |
| option: `name` | ✓ | ✓ |  |
| `worker.postMessage(value[, transferList])` | ✓ | ✓ |  |
| `worker.getHeapSnapshot([options])` | ✓ | ⚠ |  |
| `worker.getHeapStatistics()` | ✓ | ⚠ |  |
| `worker.cpuUsage([prev])` | ✓ | ⚠ |  |
| `worker.startCpuProfile([options])` | ✓ | ⚠ |  |
| `worker.startHeapProfile([options])` | ✓ | ⚠ |  |
| `worker.ref()` | ✓ | ✓ |  |
| `worker.unref()` | ✓ | ✓ |  |
| `worker.terminate()` | ✓ | ✓ |  |
| `worker[Symbol.asyncDispose]()` | ✓ | ⚠ |  |
| `worker.performance` | ✓ | ⚠ |  |
| `worker.performance.eventLoopUtilization()` | ✓ | ⚠ |  |
| `worker.resourceLimits` | ✓ | ⚠ |  |
| `worker.threadId` | ✓ | ✓ |  |
| `worker.threadName` | ✓ | ⚠ |  |
| `worker.stdin` | ✓ | ✗ |  |
| `worker.stdout` | ✓ | ✗ |  |
| `worker.stderr` | ✓ | ✗ |  |

#### Worker Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'online'` | ✓ | ✓ |  |
| `'message'` | ✓ | ✓ |  |
| `'messageerror'` | ✓ | ✓ |  |
| `'error'` | ✓ | ✓ |  |
| `'exit'` | ✓ | ✓ |  |

#### Class: MessagePort

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `port.postMessage(value[, transferList])` | ✓ | ✓ |  |
| `port.close()` | ✓ | ✓ |  |
| `port.start()` | ✓ | ✓ |  |
| `port.ref()` | ✓ | ✓ |  |
| `port.unref()` | ✓ | ✓ |  |
| `port.hasRef()` | ✓ | ✓ |  |

#### MessagePort Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'message'` | ✓ | ✓ |  |
| `'messageerror'` | ✓ | ✓ |  |
| `'close'` | ✓ | ✓ |  |

#### Class: MessageChannel

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new MessageChannel()` | ✓ | ✓ |  |
| `channel.port1` | ✓ | ✓ |  |
| `channel.port2` | ✓ | ✓ |  |

#### Class: BroadcastChannel

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new BroadcastChannel(name)` | ✓ | ✓ |  |
| `bc.postMessage(message)` | ✓ | ✓ |  |
| `bc.close()` | ✓ | ✓ |  |
| `bc.ref()` | ✓ | ✓ |  |
| `bc.unref()` | ✓ | ✓ |  |
| `bc.onmessage` | ✓ | ✓ |  |
| `bc.onmessageerror` | ✓ | ✓ |  |

#### BroadcastChannel Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'message'` | ✓ | ✓ |  |
| `'messageerror'` | ✓ | ✓ |  |

#### Web Locks (experimental)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `locks.request(name[, options], callback)` | ✓ | ⚠ | experimental |
| `locks.query()` | ✓ | ⚠ | experimental |
| `class Lock` (`name`, `mode`) | ✓ | ⚠ | experimental |

---

### node:zlib

Bun status: 🟢 Fully implemented. 98% of Node.js's test suite passes.

#### Classes

| Class | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `zlib.Deflate` | ✓ | ✓ |  |
| `zlib.DeflateRaw` | ✓ | ✓ |  |
| `zlib.Gzip` | ✓ | ✓ |  |
| `zlib.Gunzip` | ✓ | ✓ |  |
| `zlib.Inflate` | ✓ | ✓ |  |
| `zlib.InflateRaw` | ✓ | ✓ |  |
| `zlib.Unzip` | ✓ | ✓ |  |
| `zlib.BrotliCompress` | ✓ | ✓ |  |
| `zlib.BrotliDecompress` | ✓ | ✓ |  |
| `zlib.ZstdCompress` | ✓ | ✓ | experimental |
| `zlib.ZstdDecompress` | ✓ | ✓ | experimental |
| `zlib.ZlibBase` (base class) | ✓ | ✓ |  |

#### Factory Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `zlib.createDeflate([options])` | ✓ | ✓ |  |
| `zlib.createDeflateRaw([options])` | ✓ | ✓ |  |
| `zlib.createGunzip([options])` | ✓ | ✓ |  |
| `zlib.createGzip([options])` | ✓ | ✓ |  |
| `zlib.createInflate([options])` | ✓ | ✓ |  |
| `zlib.createInflateRaw([options])` | ✓ | ✓ |  |
| `zlib.createUnzip([options])` | ✓ | ✓ |  |
| `zlib.createBrotliCompress([options])` | ✓ | ✓ |  |
| `zlib.createBrotliDecompress([options])` | ✓ | ✓ |  |
| `zlib.createZstdCompress([options])` | ✓ | ✓ |  |
| `zlib.createZstdDecompress([options])` | ✓ | ✓ |  |

#### Convenience Functions (callback + sync)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `zlib.deflate(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.deflateSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.deflateRaw(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.deflateRawSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.gzip(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.gzipSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.gunzip(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.gunzipSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.inflate(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.inflateSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.inflateRaw(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.inflateRawSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.unzip(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.unzipSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.brotliCompress(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.brotliCompressSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.brotliDecompress(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.brotliDecompressSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.zstdCompress(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.zstdCompressSync(buffer[, options])` | ✓ | ✓ |  |
| `zlib.zstdDecompress(buffer[, options], callback)` | ✓ | ✓ |  |
| `zlib.zstdDecompressSync(buffer[, options])` | ✓ | ✓ |  |

#### ZlibBase Methods / Properties

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `zlib.close([callback])` | ✓ | ✓ |  |
| `zlib.flush([kind,] callback)` | ✓ | ✓ |  |
| `zlib.params(level, strategy, callback)` | ✓ | ✓ |  |
| `zlib.reset()` | ✓ | ✓ |  |
| `zlib.bytesWritten` | ✓ | ✓ |  |
| `zlib.bytesRead` | ✓ | ✓ |  |

#### Other Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `zlib.crc32(data[, value])` | ✓ | ⚠ |  |
| `zlib.constants` | ✓ | ✓ | object with all constants |

#### Constants — Flush Values

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `Z_NO_FLUSH` | ✓ | ✓ |  |
| `Z_PARTIAL_FLUSH` | ✓ | ✓ |  |
| `Z_SYNC_FLUSH` | ✓ | ✓ |  |
| `Z_FULL_FLUSH` | ✓ | ✓ |  |
| `Z_FINISH` | ✓ | ✓ |  |
| `Z_BLOCK` | ✓ | ✓ |  |
| `Z_TREES` | ✓ | ✓ |  |

#### Constants — Return Codes

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `Z_OK` | ✓ | ✓ |  |
| `Z_STREAM_END` | ✓ | ✓ |  |
| `Z_NEED_DICT` | ✓ | ✓ |  |
| `Z_ERRNO` | ✓ | ✓ |  |
| `Z_STREAM_ERROR` | ✓ | ✓ |  |
| `Z_DATA_ERROR` | ✓ | ✓ |  |
| `Z_MEM_ERROR` | ✓ | ✓ |  |
| `Z_BUF_ERROR` | ✓ | ✓ |  |
| `Z_VERSION_ERROR` | ✓ | ✓ |  |

#### Constants — Compression Levels

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `Z_NO_COMPRESSION` | ✓ | ✓ |  |
| `Z_BEST_SPEED` | ✓ | ✓ |  |
| `Z_BEST_COMPRESSION` | ✓ | ✓ |  |
| `Z_DEFAULT_COMPRESSION` | ✓ | ✓ |  |

#### Constants — Strategy

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `Z_FILTERED` | ✓ | ✓ |  |
| `Z_HUFFMAN_ONLY` | ✓ | ✓ |  |
| `Z_RLE` | ✓ | ✓ |  |
| `Z_FIXED` | ✓ | ✓ |  |
| `Z_DEFAULT_STRATEGY` | ✓ | ✓ |  |
| `ZLIB_VERNUM` | ✓ | ✓ |  |
| `DEFLATE` / `INFLATE` / `GZIP` / `GUNZIP` / `DEFLATERAW` / `INFLATERAW` / `UNZIP` | ✓ | ✓ | engine selectors |
| `BROTLI_DECODE` / `BROTLI_ENCODE` | ✓ | ✓ |  |

#### Constants — Brotli

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `BROTLI_OPERATION_PROCESS` | ✓ | ✓ |  |
| `BROTLI_OPERATION_FLUSH` | ✓ | ✓ |  |
| `BROTLI_OPERATION_FINISH` | ✓ | ✓ |  |
| `BROTLI_OPERATION_EMIT_METADATA` | ✓ | ✓ |  |
| `BROTLI_PARAM_MODE` | ✓ | ✓ |  |
| `BROTLI_PARAM_QUALITY` | ✓ | ✓ |  |
| `BROTLI_PARAM_SIZE_HINT` | ✓ | ✓ |  |
| `BROTLI_PARAM_LGWIN` | ✓ | ✓ |  |
| `BROTLI_PARAM_LGBLOCK` | ✓ | ✓ |  |
| `BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING` | ✓ | ✓ |  |
| `BROTLI_PARAM_LARGE_WINDOW` | ✓ | ✓ |  |
| `BROTLI_PARAM_NPOSTFIX` | ✓ | ✓ |  |
| `BROTLI_PARAM_NDIRECT` | ✓ | ✓ |  |
| `BROTLI_MODE_GENERIC` / `BROTLI_MODE_TEXT` / `BROTLI_MODE_FONT` | ✓ | ✓ |  |
| `BROTLI_MIN_QUALITY` / `BROTLI_MAX_QUALITY` / `BROTLI_DEFAULT_QUALITY` | ✓ | ✓ |  |
| `BROTLI_MIN_WINDOW_BITS` / `BROTLI_MAX_WINDOW_BITS` / `BROTLI_DEFAULT_WINDOW` | ✓ | ✓ |  |
| `BROTLI_MIN_INPUT_BLOCK_BITS` / `BROTLI_MAX_INPUT_BLOCK_BITS` | ✓ | ✓ |  |

#### Constants — Zstd (experimental)

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `ZSTD_e_continue` / `ZSTD_e_flush` / `ZSTD_e_end` | ✓ | ⚠ |  |
| `ZSTD_c_compressionLevel` / `ZSTD_c_strategy` / `ZSTD_c_windowLog` | ✓ | ⚠ |  |
| `ZSTD_fast` / `ZSTD_dfast` / `ZSTD_greedy` / `ZSTD_lazy` / `ZSTD_lazy2` / `ZSTD_btlazy2` / `ZSTD_btopt` / `ZSTD_btultra` / `ZSTD_btultra2` | ✓ | ⚠ |  |

---

### node:querystring

Bun status: 🟢 Fully implemented. 100% of Node.js's test suite passes.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `querystring.parse(str[, sep[, eq[, options]]])` | ✓ | ✓ |  |
| `querystring.stringify(obj[, sep[, eq[, options]]])` | ✓ | ✓ |  |
| `querystring.escape(str)` | ✓ | ✓ |  |
| `querystring.unescape(str)` | ✓ | ✓ |  |
| `querystring.unescapeBuffer(str[, decodeSpaces])` | ✓ | ✓ | enumerable Buffer-returning helper |
| `querystring.decode(...)` | ✓ | ✓ | alias of `parse` |
| `querystring.encode(...)` | ✓ | ✓ | alias of `stringify` |

Note: Module is stable (Stability: 2) but `URLSearchParams` is recommended for new code.

---

### node:url

Bun status: 🟢 Fully implemented.

#### Class: URL

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new URL(input[, base])` | ✓ | ✓ |  |
| `url.hash` | ✓ | ✓ |  |
| `url.host` | ✓ | ✓ |  |
| `url.hostname` | ✓ | ✓ |  |
| `url.href` | ✓ | ✓ |  |
| `url.origin` | ✓ | ✓ |  |
| `url.password` | ✓ | ✓ |  |
| `url.pathname` | ✓ | ✓ |  |
| `url.port` | ✓ | ✓ |  |
| `url.protocol` | ✓ | ✓ |  |
| `url.search` | ✓ | ✓ |  |
| `url.searchParams` | ✓ | ✓ |  |
| `url.username` | ✓ | ✓ |  |
| `url.toString()` | ✓ | ✓ |  |
| `url.toJSON()` | ✓ | ✓ |  |
| `URL.canParse(input[, base])` | ✓ | ✓ |  |
| `URL.parse(input[, base])` | ✓ | ✓ |  |
| `URL.createObjectURL(blob)` | ✓ | ✓ |  |
| `URL.revokeObjectURL(id)` | ✓ | ✓ |  |

#### Class: URLSearchParams

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new URLSearchParams()` | ✓ | ✓ |  |
| `new URLSearchParams(string)` | ✓ | ✓ |  |
| `new URLSearchParams(obj)` | ✓ | ✓ |  |
| `new URLSearchParams(iterable)` | ✓ | ✓ |  |
| `params.append(name, value)` | ✓ | ✓ |  |
| `params.delete(name[, value])` | ✓ | ✓ |  |
| `params.entries()` | ✓ | ✓ |  |
| `params.forEach(fn[, thisArg])` | ✓ | ✓ |  |
| `params.get(name)` | ✓ | ✓ |  |
| `params.getAll(name)` | ✓ | ✓ |  |
| `params.has(name[, value])` | ✓ | ✓ |  |
| `params.keys()` | ✓ | ✓ |  |
| `params.set(name, value)` | ✓ | ✓ |  |
| `params.size` | ✓ | ✓ |  |
| `params.sort()` | ✓ | ✓ |  |
| `params.toString()` | ✓ | ✓ |  |
| `params.values()` | ✓ | ✓ |  |
| `params[Symbol.iterator]()` | ✓ | ✓ |  |

#### Class: URLPattern (experimental)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new URLPattern()` / `new URLPattern(string)` / `new URLPattern(obj)` | ✓ | ⚠ | experimental |
| `urlPattern.exec(input[, baseURL])` | ✓ | ⚠ |  |
| `urlPattern.test(input[, baseURL])` | ✓ | ⚠ |  |

#### Module-Level Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `url.domainToASCII(domain)` | ✓ | ✓ |  |
| `url.domainToUnicode(domain)` | ✓ | ✓ |  |
| `url.fileURLToPath(url[, options])` | ✓ | ✓ |  |
| `url.fileURLToPathBuffer(url[, options])` | ✓ | ⚠ | newer |
| `url.pathToFileURL(path[, options])` | ✓ | ✓ |  |
| `url.format(URL[, options])` | ✓ | ✓ |  |
| `url.urlToHttpOptions(url)` | ✓ | ✓ |  |

#### Legacy API (Deprecated)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `url.parse(urlString[, parseQueryString[, slashesDenoteHost]])` | ✓ | ✓ | deprecated |
| `url.format(urlObject)` | ✓ | ✓ | deprecated overload |
| `url.resolve(from, to)` | ✓ | ✓ | deprecated |
| Legacy `urlObject` (auth, hash, host, hostname, href, path, pathname, port, protocol, query, search, slashes) | ✓ | ✓ | deprecated |

---

### node:vm

Bun status: 🟡 Core functionality + ES modules implemented including `Script`, `createContext`, `runInContext`, `runInNewContext`, `runInThisContext`, `compileFunction`, `isContext`, `Module`, `SourceTextModule`, `SyntheticModule`, `importModuleDynamically`. `timeout` and `breakOnSigint` supported. Missing `measureMemory` and some `cachedData` functionality.

#### Class: vm.Script

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new vm.Script(code[, options])` | ✓ | ✓ |  |
| `script.cachedDataRejected` | ✓ | ⚠ | partial cachedData |
| `script.sourceMapURL` | ✓ | ✓ |  |
| `script.createCachedData()` | ✓ | ⚠ |  |
| `script.runInContext(contextifiedObject[, options])` | ✓ | ✓ |  |
| `script.runInNewContext([contextObject[, options]])` | ✓ | ✓ |  |
| `script.runInThisContext([options])` | ✓ | ✓ |  |

#### Class: vm.Module

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `module.error` | ✓ | ✓ |  |
| `module.identifier` | ✓ | ✓ |  |
| `module.namespace` | ✓ | ✓ |  |
| `module.status` | ✓ | ✓ |  |
| `module.evaluate([options])` | ✓ | ✓ |  |
| `module.link(linker)` | ✓ | ✓ |  |

#### Class: vm.SourceTextModule

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new vm.SourceTextModule(code[, options])` | ✓ | ✓ |  |
| `dependencySpecifiers` | ✓ | ✓ | deprecated |
| `moduleRequests` | ✓ | ⚠ |  |
| `createCachedData()` | ✓ | ⚠ |  |
| `hasAsyncGraph()` | ✓ | ⚠ |  |
| `hasTopLevelAwait()` | ✓ | ⚠ |  |
| `instantiate()` | ✓ | ⚠ |  |
| `linkRequests(modules)` | ✓ | ⚠ |  |

#### Class: vm.SyntheticModule

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new vm.SyntheticModule(exportNames, evaluateCallback[, options])` | ✓ | ✓ |  |
| `setExport(name, value)` | ✓ | ✓ |  |

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `vm.createContext([contextObject[, options]])` | ✓ | ✓ |  |
| `vm.isContext(object)` | ✓ | ✓ |  |
| `vm.runInContext(code, contextifiedObject[, options])` | ✓ | ✓ |  |
| `vm.runInNewContext(code[, contextObject[, options]])` | ✓ | ✓ |  |
| `vm.runInThisContext(code[, options])` | ✓ | ✓ |  |
| `vm.compileFunction(code[, params[, options]])` | ✓ | ✓ |  |
| `vm.measureMemory([options])` | ✓ | ✗ | missing in Bun |

#### Constants

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER` | ✓ | ⚠ |  |
| `vm.constants.DONT_CONTEXTIFY` | ✓ | ⚠ |  |

---

### node:async_hooks

Bun status: 🟡 `AsyncLocalStorage` and `AsyncResource` implemented. v8 promise hooks not called.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `async_hooks.createHook(options)` | ✓ | ⚠ | v8 promise hooks not called |
| `async_hooks.executionAsyncId()` | ✓ | ⚠ |  |
| `async_hooks.executionAsyncResource()` | ✓ | ⚠ |  |
| `async_hooks.triggerAsyncId()` | ✓ | ⚠ |  |

#### createHook Callbacks

| Callback | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `init(asyncId, type, triggerAsyncId, resource)` | ✓ | ⚠ |  |
| `before(asyncId)` | ✓ | ⚠ |  |
| `after(asyncId)` | ✓ | ⚠ |  |
| `destroy(asyncId)` | ✓ | ⚠ |  |
| `promiseResolve(asyncId)` | ✓ | ⚠ |  |

#### Properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `async_hooks.asyncWrapProviders` | ✓ | ⚠ |  |

#### Class: AsyncHook

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `asyncHook.enable()` | ✓ | ⚠ |  |
| `asyncHook.disable()` | ✓ | ⚠ |  |

#### Class: AsyncLocalStorage

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new AsyncLocalStorage()` | ✓ | ✓ |  |
| `als.getStore()` | ✓ | ✓ |  |
| `als.run(store, callback, ...args)` | ✓ | ✓ |  |
| `als.exit(callback, ...args)` | ✓ | ✓ |  |
| `als.enterWith(store)` | ✓ | ✓ |  |
| `als.disable()` | ✓ | ✓ |  |
| `als.bind(fn)` | ✓ | ✓ |  |
| `als.snapshot()` | ✓ | ✓ |  |
| `AsyncLocalStorage.bind(fn)` | ✓ | ✓ |  |
| `AsyncLocalStorage.snapshot()` | ✓ | ✓ |  |

#### Class: AsyncResource

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new AsyncResource(type[, options])` | ✓ | ✓ |  |
| `ar.asyncId()` | ✓ | ✓ |  |
| `ar.triggerAsyncId()` | ✓ | ✓ |  |
| `ar.runInAsyncScope(fn, thisArg, ...args)` | ✓ | ✓ |  |
| `ar.emitDestroy()` | ✓ | ✓ |  |
| `ar.bind(fn)` | ✓ | ✓ |  |
| `AsyncResource.bind(fn)` | ✓ | ✓ |  |

---

### node:perf_hooks

Bun status: 🟡 APIs are implemented, but Node.js test suite does not pass yet for this module.

#### performance Methods

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `performance.now()` | ✓ | ✓ |  |
| `performance.mark(name[, options])` | ✓ | ✓ |  |
| `performance.measure(name[, startMarkOrOptions[, endMark]])` | ✓ | ✓ |  |
| `performance.clearMarks([name])` | ✓ | ✓ |  |
| `performance.clearMeasures([name])` | ✓ | ✓ |  |
| `performance.clearResourceTimings([name])` | ✓ | ✓ |  |
| `performance.getEntries()` | ✓ | ✓ |  |
| `performance.getEntriesByName(name[, type])` | ✓ | ✓ |  |
| `performance.getEntriesByType(type)` | ✓ | ✓ |  |
| `performance.eventLoopUtilization([util1[, util2]])` | ✓ | ⚠ |  |
| `performance.setResourceTimingBufferSize(maxSize)` | ✓ | ⚠ |  |
| `performance.timerify(fn[, options])` | ✓ | ⚠ |  |
| `performance.markResourceTiming(...)` | ✓ | ⚠ |  |
| `performance.toJSON()` | ✓ | ✓ |  |
| `performance.nodeTiming` | ✓ | ⚠ |  |
| `performance.timeOrigin` | ✓ | ✓ |  |
| `'resourcetimingbufferfull'` event | ✓ | ⚠ |  |

#### Class: PerformanceEntry

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `entry.name` | ✓ | ✓ |  |
| `entry.entryType` | ✓ | ✓ |  |
| `entry.startTime` | ✓ | ✓ |  |
| `entry.duration` | ✓ | ✓ |  |
| `entry.detail` | ✓ | ✓ |  |

#### Class: PerformanceMark / PerformanceMeasure

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `PerformanceMark` extends PerformanceEntry | ✓ | ✓ |  |
| `PerformanceMeasure` extends PerformanceEntry | ✓ | ✓ |  |

#### Class: PerformanceNodeEntry

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `entry.detail` | ✓ | ⚠ |  |
| `entry.flags` | ✓ | ⚠ | deprecated |
| `entry.kind` | ✓ | ⚠ | deprecated |

#### Class: PerformanceNodeTiming

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `nodeStart` | ✓ | ⚠ |  |
| `v8Start` | ✓ | ⚠ |  |
| `environment` | ✓ | ⚠ |  |
| `bootstrapComplete` | ✓ | ⚠ |  |
| `loopStart` | ✓ | ⚠ |  |
| `loopExit` | ✓ | ⚠ |  |
| `idleTime` | ✓ | ⚠ |  |
| `uvMetricsInfo` | ✓ | ⚠ |  |

#### Class: PerformanceResourceTiming

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `workerStart` / `redirectStart` / `redirectEnd` / `fetchStart` | ✓ | ✓ |  |
| `domainLookupStart` / `domainLookupEnd` | ✓ | ✓ |  |
| `connectStart` / `connectEnd` / `secureConnectionStart` | ✓ | ✓ |  |
| `requestStart` / `responseEnd` | ✓ | ✓ |  |
| `transferSize` / `encodedBodySize` / `decodedBodySize` | ✓ | ✓ |  |
| `toJSON()` | ✓ | ✓ |  |

#### Class: PerformanceObserver

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new PerformanceObserver(callback)` | ✓ | ✓ |  |
| `PerformanceObserver.supportedEntryTypes` | ✓ | ✓ |  |
| `observer.observe(options)` | ✓ | ✓ |  |
| `observer.disconnect()` | ✓ | ✓ |  |
| `observer.takeRecords()` | ✓ | ✓ |  |

#### Class: PerformanceObserverEntryList

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `list.getEntries()` | ✓ | ✓ |  |
| `list.getEntriesByName(name[, type])` | ✓ | ✓ |  |
| `list.getEntriesByType(type)` | ✓ | ✓ |  |

#### Class: Histogram

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `histogram.count` / `countBigInt` | ✓ | ⚠ |  |
| `histogram.min` / `minBigInt` | ✓ | ⚠ |  |
| `histogram.max` / `maxBigInt` | ✓ | ⚠ |  |
| `histogram.mean` | ✓ | ⚠ |  |
| `histogram.stddev` | ✓ | ⚠ |  |
| `histogram.exceeds` / `exceedsBigInt` | ✓ | ⚠ |  |
| `histogram.percentiles` / `percentilesBigInt` | ✓ | ⚠ |  |
| `histogram.percentile(percentile)` | ✓ | ⚠ |  |
| `histogram.percentileBigInt(percentile)` | ✓ | ⚠ |  |
| `histogram.reset()` | ✓ | ⚠ |  |

#### Class: IntervalHistogram (extends Histogram)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `histogram.enable()` | ✓ | ⚠ |  |
| `histogram.disable()` | ✓ | ⚠ |  |
| `histogram[Symbol.dispose]()` | ✓ | ⚠ |  |

#### Class: RecordableHistogram (extends Histogram)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `histogram.record(val)` | ✓ | ⚠ |  |
| `histogram.recordDelta()` | ✓ | ⚠ |  |
| `histogram.add(other)` | ✓ | ⚠ |  |

#### Module-Level Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `perf_hooks.createHistogram([options])` | ✓ | ⚠ |  |
| `perf_hooks.monitorEventLoopDelay([options])` | ✓ | ⚠ |  |
| `perf_hooks.eventLoopUtilization([util1[, util2]])` | ✓ | ⚠ |  |
| `perf_hooks.timerify(fn[, options])` | ✓ | ⚠ |  |

---

### node:timers

Bun status: 🟢 Recommended to use global `setTimeout`, etc., instead.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `setImmediate(callback[, ...args])` | ✓ | ✓ |  |
| `setInterval(callback[, delay[, ...args]])` | ✓ | ✓ |  |
| `setTimeout(callback[, delay[, ...args]])` | ✓ | ✓ |  |
| `clearImmediate(immediate)` | ✓ | ✓ |  |
| `clearInterval(timeout)` | ✓ | ✓ |  |
| `clearTimeout(timeout)` | ✓ | ✓ |  |

#### Class: Immediate

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `immediate.ref()` | ✓ | ✓ |  |
| `immediate.unref()` | ✓ | ✓ |  |
| `immediate.hasRef()` | ✓ | ✓ |  |
| `immediate[Symbol.dispose]()` | ✓ | ✓ |  |

#### Class: Timeout

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `timeout.ref()` | ✓ | ✓ |  |
| `timeout.unref()` | ✓ | ✓ |  |
| `timeout.hasRef()` | ✓ | ✓ |  |
| `timeout.refresh()` | ✓ | ✓ |  |
| `timeout.close()` | ✓ | ✓ | legacy |
| `timeout[Symbol.toPrimitive]()` | ✓ | ✓ |  |
| `timeout[Symbol.dispose]()` | ✓ | ✓ |  |

---

### node:timers/promises

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `setTimeout([delay[, value[, options]]])` | ✓ | ✓ | options: `ref`, `signal` |
| `setImmediate([value[, options]])` | ✓ | ✓ |  |
| `setInterval([delay[, value[, options]]])` | ✓ | ✓ | async iterator |
| `scheduler.wait(delay[, options])` | ✓ | ⚠ | experimental |
| `scheduler.yield()` | ✓ | ⚠ | experimental |

---

### node:tty

Bun status: 🟢 Fully implemented.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `tty.isatty(fd)` | ✓ | ✓ |  |

#### Class: tty.ReadStream (extends net.Socket)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new tty.ReadStream(fd[, options])` | ✓ | ✓ |  |
| `readStream.isRaw` | ✓ | ✓ |  |
| `readStream.isTTY` | ✓ | ✓ |  |
| `readStream.fd` | ✓ | ✓ |  |
| `readStream.setRawMode(mode)` | ✓ | ✓ |  |

#### Class: tty.WriteStream (extends net.Socket)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new tty.WriteStream(fd)` | ✓ | ✓ |  |
| `writeStream.isTTY` | ✓ | ✓ |  |
| `writeStream.fd` | ✓ | ✓ |  |
| `writeStream.columns` | ✓ | ✓ |  |
| `writeStream.rows` | ✓ | ✓ |  |
| `writeStream.clearLine(dir[, callback])` | ✓ | ✓ |  |
| `writeStream.clearScreenDown([callback])` | ✓ | ✓ |  |
| `writeStream.cursorTo(x[, y][, callback])` | ✓ | ✓ |  |
| `writeStream.moveCursor(dx, dy[, callback])` | ✓ | ✓ |  |
| `writeStream.getWindowSize()` | ✓ | ✓ |  |
| `writeStream.getColorDepth([env])` | ✓ | ✓ |  |
| `writeStream.hasColors([count][, env])` | ✓ | ✓ |  |

#### Events on WriteStream

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'resize'` | ✓ | ✓ |  |

---

### node:v8

Bun status: 🟡 `writeHeapSnapshot` and `getHeapSnapshot` implemented. `serialize`/`deserialize` use JavaScriptCore's wire format instead of V8's. Other methods not implemented.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `v8.cachedDataVersionTag()` | ✓ | ✗ |  |
| `v8.getHeapCodeStatistics()` | ✓ | ✗ |  |
| `v8.getHeapSnapshot([options])` | ✓ | ✓ |  |
| `v8.getHeapSpaceStatistics()` | ✓ | ✗ |  |
| `v8.getHeapStatistics()` | ✓ | ✗ |  |
| `v8.getCppHeapStatistics([detailLevel])` | ✓ | ✗ |  |
| `v8.queryObjects(ctor[, options])` | ✓ | ✗ |  |
| `v8.setFlagsFromString(flags)` | ✓ | ✗ |  |
| `v8.stopCoverage()` | ✓ | ✗ |  |
| `v8.takeCoverage()` | ✓ | ✗ |  |
| `v8.writeHeapSnapshot([filename[, options]])` | ✓ | ✓ |  |
| `v8.setHeapSnapshotNearHeapLimit(limit)` | ✓ | ✗ |  |
| `v8.isStringOneByteRepresentation(content)` | ✓ | ✗ |  |
| `v8.startCpuProfile([options])` | ✓ | ✗ |  |
| `v8.startHeapProfile([options])` | ✓ | ✗ |  |
| `v8.serialize(value)` | ✓ | ⚠ | uses JSC wire format |
| `v8.deserialize(buffer)` | ✓ | ⚠ | uses JSC wire format |

#### Class: v8.Serializer

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new Serializer()` | ✓ | ⚠ |  |
| `writeHeader()` | ✓ | ⚠ |  |
| `writeValue(value)` | ✓ | ⚠ |  |
| `releaseBuffer()` | ✓ | ⚠ |  |
| `transferArrayBuffer(id, arrayBuffer)` | ✓ | ⚠ |  |
| `writeUint32(value)` | ✓ | ⚠ |  |
| `writeUint64(hi, lo)` | ✓ | ⚠ |  |
| `writeDouble(value)` | ✓ | ⚠ |  |
| `writeRawBytes(buffer)` | ✓ | ⚠ |  |
| `_writeHostObject(object)` | ✓ | ⚠ |  |
| `_getDataCloneError(message)` | ✓ | ⚠ |  |
| `_getSharedArrayBufferId(sab)` | ✓ | ⚠ |  |
| `_setTreatArrayBufferViewsAsHostObjects(flag)` | ✓ | ⚠ |  |

#### Class: v8.Deserializer

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new Deserializer(buffer)` | ✓ | ⚠ |  |
| `readHeader()` | ✓ | ⚠ |  |
| `readValue()` | ✓ | ⚠ |  |
| `transferArrayBuffer(id, arrayBuffer)` | ✓ | ⚠ |  |
| `getWireFormatVersion()` | ✓ | ⚠ |  |
| `readUint32()` | ✓ | ⚠ |  |
| `readUint64()` | ✓ | ⚠ |  |
| `readDouble()` | ✓ | ⚠ |  |
| `readRawBytes(length)` | ✓ | ⚠ |  |
| `_readHostObject()` | ✓ | ⚠ |  |

#### Convenience Classes

| Class | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `v8.DefaultSerializer` | ✓ | ⚠ |  |
| `v8.DefaultDeserializer` | ✓ | ⚠ |  |

#### Promise Hooks API

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `v8.promiseHooks.onInit(init)` | ✓ | ✗ |  |
| `v8.promiseHooks.onSettled(settled)` | ✓ | ✗ |  |
| `v8.promiseHooks.onBefore(before)` | ✓ | ✗ |  |
| `v8.promiseHooks.onAfter(after)` | ✓ | ✗ |  |
| `v8.promiseHooks.createHook(callbacks)` | ✓ | ✗ |  |

#### Startup Snapshot API

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `v8.startupSnapshot.addSerializeCallback(cb[, data])` | ✓ | ✗ |  |
| `v8.startupSnapshot.addDeserializeCallback(cb[, data])` | ✓ | ✗ |  |
| `v8.startupSnapshot.setDeserializeMainFunction(cb[, data])` | ✓ | ✗ |  |
| `v8.startupSnapshot.isBuildingSnapshot()` | ✓ | ✗ |  |

#### Class: v8.GCProfiler

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new GCProfiler()` | ✓ | ✗ |  |
| `start()` | ✓ | ✗ |  |
| `stop()` | ✓ | ✗ |  |
| `[Symbol.dispose]()` | ✓ | ✗ |  |

#### Class: SyncCPUProfileHandle / SyncHeapProfileHandle / CPUProfileHandle / HeapProfileHandle

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `stop()` | ✓ | ✗ |  |
| `[Symbol.dispose]()` | ✓ | ✗ |  |
| `[Symbol.asyncDispose]()` | ✓ | ✗ |  |

---

### node:assert (and node:assert/strict)

Bun status: 🟢 Fully implemented.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `assert(value[, message])` | ✓ | ✓ | alias of `assert.ok` |
| `assert.ok(value[, message])` | ✓ | ✓ |  |
| `assert.fail([message])` | ✓ | ✓ |  |
| `assert.equal(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.strictEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.notEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.notStrictEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.deepEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.deepStrictEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.notDeepEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.notDeepStrictEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.partialDeepStrictEqual(actual, expected[, message])` | ✓ | ✓ |  |
| `assert.match(string, regexp[, message])` | ✓ | ✓ |  |
| `assert.doesNotMatch(string, regexp[, message])` | ✓ | ✓ |  |
| `assert.throws(fn[, error][, message])` | ✓ | ✓ |  |
| `assert.doesNotThrow(fn[, error][, message])` | ✓ | ✓ |  |
| `assert.rejects(asyncFn[, error][, message])` | ✓ | ✓ |  |
| `assert.doesNotReject(asyncFn[, error][, message])` | ✓ | ✓ |  |
| `assert.ifError(value)` | ✓ | ✓ |  |

#### Classes

| Class | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `assert.AssertionError` | ✓ | ✓ |  |
| `assert.Assert` | ✓ | ⚠ | v24.6.0+ |
| `assert.CallTracker` | ✓ | ✓ | deprecated |

#### CallTracker Methods (deprecated)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `tracker.calls(fn[, exact])` | ✓ | ✓ | deprecated |
| `tracker.getCalls(fn)` | ✓ | ✓ | deprecated |
| `tracker.report()` | ✓ | ✓ | deprecated |
| `tracker.reset([fn])` | ✓ | ✓ | deprecated |
| `tracker.verify()` | ✓ | ✓ | deprecated |

#### Namespaces

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `assert.strict` namespace | ✓ | ✓ |  |
| `node:assert/strict` module | ✓ | ✓ |  |

---

### node:console

Bun status: 🟢 Fully implemented.

#### Class: Console

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new Console(stdout[, stderr][, ignoreErrors])` | ✓ | ✓ |  |
| `new Console(options)` (`stdout`/`stderr`/`ignoreErrors`/`colorMode`/`inspectOptions`/`groupIndentation`) | ✓ | ✓ |  |
| `console.Console` (class export) | ✓ | ✓ |  |

#### Standard Methods

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `console.assert(value[, ...message])` | ✓ | ✓ |  |
| `console.clear()` | ✓ | ✓ |  |
| `console.count([label])` | ✓ | ✓ |  |
| `console.countReset([label])` | ✓ | ✓ |  |
| `console.debug(data[, ...args])` | ✓ | ✓ |  |
| `console.dir(obj[, options])` | ✓ | ✓ |  |
| `console.dirxml(...data)` | ✓ | ✓ |  |
| `console.error([data][, ...args])` | ✓ | ✓ |  |
| `console.group([...label])` | ✓ | ✓ |  |
| `console.groupCollapsed()` | ✓ | ✓ |  |
| `console.groupEnd()` | ✓ | ✓ |  |
| `console.info([data][, ...args])` | ✓ | ✓ |  |
| `console.log([data][, ...args])` | ✓ | ✓ |  |
| `console.table(tabularData[, properties])` | ✓ | ✓ |  |
| `console.time([label])` | ✓ | ✓ |  |
| `console.timeEnd([label])` | ✓ | ✓ |  |
| `console.timeLog([label][, ...data])` | ✓ | ✓ |  |
| `console.trace([message][, ...args])` | ✓ | ✓ |  |
| `console.warn([data][, ...args])` | ✓ | ✓ |  |

#### Inspector-Only Methods

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `console.profile([label])` | ✓ | ⚠ |  |
| `console.profileEnd([label])` | ✓ | ⚠ |  |
| `console.timeStamp([label])` | ✓ | ⚠ |  |

---

### node:module

Bun status: 🟡 Missing `syncBuiltinESMExports`, `Module#load()`. `module._extensions`, `module._pathCache`, `module._cache` are no-ops. `module.register` not implemented (use `Bun.plugin`).

#### Module Static Methods/Properties

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Module.builtinModules` | ✓ | ✓ |  |
| `Module.createRequire(filename)` | ✓ | ✓ |  |
| `Module.findPackageJSON(specifier[, base])` | ✓ | ⚠ |  |
| `Module.findSourceMap(path)` | ✓ | ⚠ |  |
| `Module.flushCompileCache()` | ✓ | ⚠ |  |
| `Module.getCompileCacheDir()` | ✓ | ⚠ |  |
| `Module.getSourceMapsSupport()` | ✓ | ⚠ |  |
| `Module.isBuiltin(moduleName)` | ✓ | ✓ |  |
| `Module.register(specifier[, parentURL][, options])` | ✓ | ✗ | use Bun.plugin instead |
| `Module.registerHooks(options)` | ✓ | ⚠ |  |
| `Module.runMain()` | ✓ | ✓ |  |
| `Module.setSourceMapsSupport(enabled[, options])` | ✓ | ⚠ |  |
| `Module.stripTypeScriptTypes(code[, options])` | ✓ | ⚠ |  |
| `Module.syncBuiltinESMExports()` | ✓ | ✗ | missing in Bun |
| `Module.wrap(code)` | ✓ | ✓ |  |
| `Module.wrapper` | ✓ | ✓ |  |
| `Module.constants.compileCacheStatus` | ✓ | ⚠ |  |

#### Module Instance Properties/Methods

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `module.children` | ✓ | ✓ |  |
| `module.exports` | ✓ | ✓ |  |
| `module.filename` | ✓ | ✓ |  |
| `module.id` | ✓ | ✓ |  |
| `module.loaded` | ✓ | ✓ |  |
| `module.parent` | ✓ | ✓ |  |
| `module.path` | ✓ | ✓ |  |
| `module.paths` | ✓ | ✓ |  |
| `module.isPreloading` | ✓ | ✓ |  |
| `module.require(id)` | ✓ | ✓ |  |
| `module.load()` | ✓ | ✗ | missing in Bun |

#### require.cache infrastructure

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `require.cache` overrides | ✓ | ✓ |  |
| `module._extensions` | ✓ | ⚠ | no-op in Bun |
| `module._cache` | ✓ | ⚠ | no-op in Bun |
| `module._pathCache` | ✓ | ⚠ | no-op in Bun |

#### Customization Hooks

| Hook | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `initialize(data)` (async) | ✓ | ✗ |  |
| `resolve(specifier, context, nextResolve)` (async) | ✓ | ✗ |  |
| `load(url, context, nextLoad)` (async) | ✓ | ✗ |  |
| `resolve(specifier, context, nextResolve)` (sync) | ✓ | ⚠ |  |
| `load(url, context, nextLoad)` (sync) | ✓ | ⚠ |  |

#### Compile Cache API

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `module.enableCompileCache([options])` | ✓ | ⚠ |  |
| `module.getCompileCacheDir()` | ✓ | ⚠ |  |
| `module.flushCompileCache()` | ✓ | ⚠ |  |
| `module.constants.compileCacheStatus.{ENABLED,ALREADY_ENABLED,FAILED,DISABLED}` | ✓ | ⚠ |  |

#### Class: SourceMap

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new SourceMap(payload[, { lineLengths }])` | ✓ | ⚠ |  |
| `sourceMap.payload` | ✓ | ⚠ |  |
| `sourceMap.findEntry(lineOffset, columnOffset)` | ✓ | ⚠ |  |
| `sourceMap.findOrigin(lineNumber, columnNumber)` | ✓ | ⚠ |  |

---

### node:string_decoder

Bun status: 🟢 Fully implemented. 100% of Node.js's test suite passes.

#### Class: StringDecoder

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new StringDecoder([encoding])` | ✓ | ✓ | default `'utf8'` |
| `stringDecoder.write(buffer)` | ✓ | ✓ |  |
| `stringDecoder.end([buffer])` | ✓ | ✓ |  |
| `stringDecoder.lastChar` | ✓ | ✓ | internal but observable |
| `stringDecoder.lastNeed` | ✓ | ✓ | internal but observable |
| `stringDecoder.lastTotal` | ✓ | ✓ | internal but observable |

---

### node:readline

Bun status: 🟢 Fully implemented.

#### Module-Level Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `readline.createInterface(options)` | ✓ | ✓ |  |
| `readline.clearLine(stream, dir[, callback])` | ✓ | ✓ |  |
| `readline.clearScreenDown(stream[, callback])` | ✓ | ✓ |  |
| `readline.cursorTo(stream, x[, y][, callback])` | ✓ | ✓ |  |
| `readline.moveCursor(stream, dx, dy[, callback])` | ✓ | ✓ |  |
| `readline.emitKeypressEvents(stream[, interface])` | ✓ | ✓ |  |

#### Class: readline.Interface

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `rl.close()` | ✓ | ✓ |  |
| `rl[Symbol.dispose]()` | ✓ | ✓ |  |
| `rl.pause()` | ✓ | ✓ |  |
| `rl.resume()` | ✓ | ✓ |  |
| `rl.prompt([preserveCursor])` | ✓ | ✓ |  |
| `rl.setPrompt(prompt)` | ✓ | ✓ |  |
| `rl.getPrompt()` | ✓ | ✓ |  |
| `rl.question(query[, options], callback)` | ✓ | ✓ |  |
| `rl.write(data[, key])` | ✓ | ✓ |  |
| `rl.getCursorPos()` | ✓ | ✓ |  |
| `rl[Symbol.asyncIterator]()` | ✓ | ✓ |  |
| `rl.line` | ✓ | ✓ |  |
| `rl.cursor` | ✓ | ✓ |  |
| `rl.terminal` | ✓ | ✓ |  |

#### Events on readline.Interface

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'line'` | ✓ | ✓ |  |
| `'close'` | ✓ | ✓ |  |
| `'pause'` | ✓ | ✓ |  |
| `'resume'` | ✓ | ✓ |  |
| `'history'` | ✓ | ✓ |  |
| `'SIGINT'` | ✓ | ✓ |  |
| `'SIGTSTP'` | ✓ | ✓ | not Windows |
| `'SIGCONT'` | ✓ | ✓ | not Windows |
| `'error'` | ✓ | ✓ |  |

---

### node:readline/promises

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `readlinePromises.createInterface(options)` | ✓ | ✓ |  |
| `rl.question(query[, options])` (returns Promise) | ✓ | ✓ | `signal` supported |
| `class Readline(stream[, options])` | ✓ | ✓ |  |
| `rl.clearLine(dir)` | ✓ | ✓ |  |
| `rl.clearScreenDown()` | ✓ | ✓ |  |
| `rl.cursorTo(x[, y])` | ✓ | ✓ |  |
| `rl.moveCursor(dx, dy)` | ✓ | ✓ |  |
| `rl.commit()` | ✓ | ✓ |  |
| `rl.rollback()` | ✓ | ✓ |  |

---

### node:repl

Bun status: 🔴 Not implemented.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `repl.start([options])` | ✓ | ✗ |  |
| `repl.builtinModules` | ✓ | ✗ | deprecated; use `module.builtinModules` |

#### Constants

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `repl.REPL_MODE_SLOPPY` | ✓ | ✗ |  |
| `repl.REPL_MODE_STRICT` | ✓ | ✗ |  |
| `repl.Recoverable` | ✓ | ✗ |  |

#### Class: REPLServer (extends readline.Interface)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `replServer.context` | ✓ | ✗ |  |
| `replServer.editorMode` | ✓ | ✗ |  |
| `replServer.useColors` | ✓ | ✗ |  |
| `replServer.useGlobal` | ✓ | ✗ |  |
| `replServer.ignoreUndefined` | ✓ | ✗ |  |
| `replServer.replMode` | ✓ | ✗ |  |
| `replServer.defineCommand(keyword, cmd)` | ✓ | ✗ |  |
| `replServer.displayPrompt([preserveCursor])` | ✓ | ✗ |  |
| `replServer.clearBufferedCommand()` | ✓ | ✗ |  |
| `replServer.setupHistory(historyConfig, callback)` | ✓ | ✗ |  |

#### Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'exit'` | ✓ | ✗ |  |
| `'reset'` | ✓ | ✗ |  |

---

### node:trace_events

Bun status: 🔴 Not implemented.

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `trace_events.createTracing(options)` | ✓ | ✗ |  |
| `trace_events.getEnabledCategories()` | ✓ | ✗ |  |

#### Class: Tracing

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `tracing.categories` | ✓ | ✗ |  |
| `tracing.enabled` | ✓ | ✗ |  |
| `tracing.enable()` | ✓ | ✗ |  |
| `tracing.disable()` | ✓ | ✗ |  |

---

### node:inspector (and node:inspector/promises)

Bun status: 🟡 Partially implemented. `Profiler` API supported (`Profiler.enable`, `Profiler.disable`, `Profiler.start`, `Profiler.stop`, `Profiler.setSamplingInterval`). Other inspector APIs not yet implemented.

#### Module-Level

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `inspector.open([port[, host[, wait]]])` | ✓ | ⚠ |  |
| `inspector.close()` | ✓ | ⚠ |  |
| `inspector.url()` | ✓ | ⚠ |  |
| `inspector.waitForDebugger()` | ✓ | ⚠ |  |
| `inspector.console` | ✓ | ⚠ |  |

#### Class: Session

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new Session()` | ✓ | ⚠ |  |
| `session.connect()` | ✓ | ⚠ |  |
| `session.connectToMainThread()` | ✓ | ✗ |  |
| `session.disconnect()` | ✓ | ⚠ |  |
| `session.post(method[, params[, callback]])` (callback) | ✓ | ⚠ | Profiler.* only |
| `session.post(method[, params])` (promise, `inspector/promises`) | ✓ | ⚠ | Profiler.* only |

#### Session Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'inspectorNotification'` | ✓ | ⚠ |  |
| `<protocol-method>` (e.g. `'Debugger.paused'`) | ✓ | ⚠ |  |

#### Network Namespace (experimental)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `inspector.Network.requestWillBeSent(params)` | ✓ | ✗ |  |
| `inspector.Network.responseReceived(params)` | ✓ | ✗ |  |
| `inspector.Network.dataReceived(params)` | ✓ | ✗ |  |
| `inspector.Network.dataSent(params)` | ✓ | ✗ |  |
| `inspector.Network.loadingFinished(params)` | ✓ | ✗ |  |
| `inspector.Network.loadingFailed(params)` | ✓ | ✗ |  |
| `inspector.Network.webSocketCreated(params)` | ✓ | ✗ |  |
| `inspector.Network.webSocketHandshakeResponseReceived(params)` | ✓ | ✗ |  |
| `inspector.Network.webSocketClosed(params)` | ✓ | ✗ |  |

---

### node:diagnostics_channel

Bun status: 🟢 Fully implemented.

#### Module-Level Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `diagnostics_channel.hasSubscribers(name)` | ✓ | ✓ |  |
| `diagnostics_channel.channel(name)` | ✓ | ✓ |  |
| `diagnostics_channel.subscribe(name, onMessage)` | ✓ | ✓ |  |
| `diagnostics_channel.unsubscribe(name, onMessage)` | ✓ | ✓ |  |
| `diagnostics_channel.tracingChannel(nameOrChannels)` | ✓ | ✓ | experimental |
| `diagnostics_channel.boundedChannel(nameOrChannels)` | ✓ | ✓ | v26.1.0 experimental |

#### Class: Channel

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `channel.hasSubscribers` | ✓ | ✓ |  |
| `channel.publish(message)` | ✓ | ✓ |  |
| `channel.subscribe(onMessage)` | ✓ | ✓ |  |
| `channel.unsubscribe(onMessage)` | ✓ | ✓ |  |
| `channel.bindStore(store[, transform])` | ✓ | ✓ | experimental |
| `channel.unbindStore(store)` | ✓ | ✓ | experimental |
| `channel.runStores(context, fn[, thisArg[, ...args]])` | ✓ | ✓ | experimental |
| `channel.withStoreScope(data)` | ✓ | ✓ | v26.1.0 experimental |

#### Class: TracingChannel

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `tracingChannel.subscribe(subscribers)` | ✓ | ✓ |  |
| `tracingChannel.unsubscribe(subscribers)` | ✓ | ✓ |  |
| `tracingChannel.traceSync(fn[, context[, thisArg[, ...args]]])` | ✓ | ✓ |  |
| `tracingChannel.tracePromise(fn[, context[, thisArg[, ...args]]])` | ✓ | ✓ |  |
| `tracingChannel.traceCallback(fn[, position[, context[, thisArg[, ...args]]]])` | ✓ | ✓ |  |
| `tracingChannel.hasSubscribers` | ✓ | ✓ |  |

#### Class: BoundedChannel (v26.1.0)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `boundedChannel.hasSubscribers` | ✓ | ✓ |  |
| `boundedChannel.subscribe(handlers)` | ✓ | ✓ |  |
| `boundedChannel.unsubscribe(handlers)` | ✓ | ✓ |  |
| `boundedChannel.run(context, fn[, thisArg[, ...args]])` | ✓ | ✓ |  |
| `boundedChannel.withScope([context])` | ✓ | ✓ |  |

#### TracingChannel Subscriber Hooks

| Hook | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `start` | ✓ | ✓ |  |
| `end` | ✓ | ✓ |  |
| `asyncStart` | ✓ | ✓ |  |
| `asyncEnd` | ✓ | ✓ |  |
| `error` | ✓ | ✓ |  |

---

### node:wasi

Bun status: 🟡 Partially implemented.

#### Class: WASI

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new WASI([options])` | ✓ | ⚠ |  |
| option: `args` | ✓ | ⚠ |  |
| option: `env` | ✓ | ⚠ |  |
| option: `preopens` | ✓ | ⚠ |  |
| option: `returnOnExit` | ✓ | ⚠ |  |
| option: `stdin` | ✓ | ⚠ |  |
| option: `stdout` | ✓ | ⚠ |  |
| option: `stderr` | ✓ | ⚠ |  |
| option: `version` (`unstable` / `preview1`) | ✓ | ⚠ |  |
| `wasi.getImportObject()` | ✓ | ⚠ |  |
| `wasi.start(instance)` | ✓ | ⚠ |  |
| `wasi.initialize(instance)` | ✓ | ⚠ |  |
| `wasi.finalizeBindings(instance[, options])` | ✓ | ⚠ |  |
| `wasi.wasiImport` | ✓ | ⚠ |  |

---

### node:test (and node:test/reporters, node:test/mock)

Bun status: 🟡 Partly implemented. Missing mocks, snapshots, timers. Use `bun:test` instead.

#### Core Test Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `test([name][, options][, fn])` | ✓ | ⚠ |  |
| `test.skip([name][, options][, fn])` | ✓ | ⚠ |  |
| `test.todo([name][, options][, fn])` | ✓ | ⚠ |  |
| `test.only([name][, options][, fn])` | ✓ | ⚠ |  |
| `suite([name][, options][, fn])` | ✓ | ⚠ |  |
| `suite.skip([name][, options][, fn])` | ✓ | ⚠ |  |
| `suite.todo([name][, options][, fn])` | ✓ | ⚠ |  |
| `suite.only([name][, options][, fn])` | ✓ | ⚠ |  |
| `describe([name][, options][, fn])` | ✓ | ⚠ | alias of `suite` |
| `describe.skip()` / `describe.todo()` / `describe.only()` | ✓ | ⚠ |  |
| `it([name][, options][, fn])` | ✓ | ⚠ | alias of `test` |
| `it.skip()` / `it.todo()` / `it.only()` | ✓ | ⚠ |  |

#### Lifecycle Hooks

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `before([fn][, options])` | ✓ | ⚠ |  |
| `after([fn][, options])` | ✓ | ⚠ |  |
| `beforeEach([fn][, options])` | ✓ | ⚠ |  |
| `afterEach([fn][, options])` | ✓ | ⚠ |  |

#### Test Runner

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `run([options])` | ✓ | ⚠ |  |

#### TestContext

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `t.test([name][, options][, fn])` | ✓ | ⚠ |  |
| `t.skip([message])` | ✓ | ⚠ |  |
| `t.todo([message])` | ✓ | ⚠ |  |
| `t.before([fn][, options])` | ✓ | ⚠ |  |
| `t.after([fn][, options])` | ✓ | ⚠ |  |
| `t.beforeEach([fn][, options])` | ✓ | ⚠ |  |
| `t.afterEach([fn][, options])` | ✓ | ⚠ |  |
| `t.diagnostic(message)` | ✓ | ⚠ |  |
| `t.plan(count[, options])` | ✓ | ⚠ |  |
| `t.runOnly(shouldRunOnlyTests)` | ✓ | ⚠ |  |
| `t.waitFor(condition[, options])` | ✓ | ⚠ |  |
| `t.assert` | ✓ | ⚠ |  |
| `t.assert.register(name, fn)` | ✓ | ⚠ |  |
| `t.assert.snapshot(value[, options])` | ✓ | ✗ | snapshots missing |
| `t.assert.fileSnapshot(value, path[, options])` | ✓ | ✗ |  |
| `t.mock` (MockTracker) | ✓ | ✗ | mocks missing |
| `t.name` | ✓ | ⚠ |  |
| `t.fullName` | ✓ | ⚠ |  |
| `t.filePath` | ✓ | ⚠ |  |
| `t.passed` | ✓ | ⚠ |  |
| `t.error` | ✓ | ⚠ |  |
| `t.attempt` | ✓ | ⚠ |  |
| `t.workerId` | ✓ | ⚠ |  |
| `t.signal` | ✓ | ⚠ |  |

#### SuiteContext

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `s.name` | ✓ | ⚠ |  |
| `s.fullName` | ✓ | ⚠ |  |
| `s.filePath` | ✓ | ⚠ |  |
| `s.signal` | ✓ | ⚠ |  |
| `s.passed` | ✓ | ⚠ |  |
| `s.attempt` | ✓ | ⚠ |  |
| `s.diagnostic(message)` | ✓ | ⚠ |  |

#### MockTracker (mock / context.mock)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `mock.fn([original[, implementation]][, options])` | ✓ | ✗ |  |
| `mock.method(object, methodName[, impl][, options])` | ✓ | ✗ |  |
| `mock.getter(object, methodName[, impl][, options])` | ✓ | ✗ |  |
| `mock.setter(object, methodName[, impl][, options])` | ✓ | ✗ |  |
| `mock.property(object, propertyName[, value])` | ✓ | ✗ |  |
| `mock.module(specifier[, options])` | ✓ | ✗ |  |
| `mock.reset()` | ✓ | ✗ |  |
| `mock.restoreAll()` | ✓ | ✗ |  |

#### MockFunctionContext

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `mock.calls` | ✓ | ✗ |  |
| `mock.callCount()` | ✓ | ✗ |  |
| `mock.resetCalls()` | ✓ | ✗ |  |
| `mock.mockImplementation(implementation)` | ✓ | ✗ |  |
| `mock.mockImplementationOnce(implementation[, onCall])` | ✓ | ✗ |  |
| `mock.restore()` | ✓ | ✗ |  |

#### MockPropertyContext

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `mock.accesses` | ✓ | ✗ |  |
| `mock.accessCount()` | ✓ | ✗ |  |
| `mock.resetAccesses()` | ✓ | ✗ |  |
| `mock.mockImplementation(value)` | ✓ | ✗ |  |
| `mock.mockImplementationOnce(value[, onAccess])` | ✓ | ✗ |  |
| `mock.restore()` | ✓ | ✗ |  |

#### MockTimers (mock.timers)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `mock.timers.enable([enableOptions])` | ✓ | ✗ | timers missing |
| `mock.timers.reset()` | ✓ | ✗ |  |
| `mock.timers.tick([milliseconds])` | ✓ | ✗ |  |
| `mock.timers.runAll()` | ✓ | ✗ |  |
| `mock.timers.setTime(milliseconds)` | ✓ | ✗ |  |
| `mock.timers[Symbol.dispose]()` | ✓ | ✗ |  |

#### Snapshot

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `snapshot.setDefaultSnapshotSerializers(serializers)` | ✓ | ✗ |  |
| `snapshot.setResolveSnapshotPath(fn)` | ✓ | ✗ |  |

#### node:test/reporters

| Reporter | Node.js | Bun | Notes |
|----------|---------|-----|-------|
| `spec` | ✓ | ⚠ |  |
| `tap` | ✓ | ⚠ |  |
| `dot` | ✓ | ⚠ |  |
| `junit` | ✓ | ⚠ |  |
| `lcov` | ✓ | ⚠ |  |
| `gha` (GitHub Actions) | ✓ | ⚠ |  |

#### TestsStream Events (run() return value)

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'test:start'` | ✓ | ⚠ |  |
| `'test:plan'` | ✓ | ⚠ |  |
| `'test:pass'` | ✓ | ⚠ |  |
| `'test:fail'` | ✓ | ⚠ |  |
| `'test:complete'` | ✓ | ⚠ |  |
| `'test:diagnostic'` | ✓ | ⚠ |  |
| `'test:coverage'` | ✓ | ⚠ |  |
| `'test:enqueue'` | ✓ | ⚠ |  |
| `'test:dequeue'` | ✓ | ⚠ |  |
| `'test:watch:drained'` | ✓ | ⚠ |  |
| `'test:watch:restarted'` | ✓ | ⚠ |  |
| `'test:stderr'` | ✓ | ⚠ |  |
| `'test:stdout'` | ✓ | ⚠ |  |
| `'test:summary'` | ✓ | ⚠ |  |
| `'test:interrupted'` | ✓ | ⚠ |  |

---

### node:sqlite

Bun status: 🔴 Not implemented. (Use `bun:sqlite` instead.)

#### Class: DatabaseSync

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `new DatabaseSync(path[, options])` | ✓ | ✗ |  |
| option: `open` | ✓ | ✗ |  |
| option: `readOnly` | ✓ | ✗ |  |
| option: `enableForeignKeyConstraints` | ✓ | ✗ |  |
| option: `enableDoubleQuotedStringLiterals` | ✓ | ✗ |  |
| option: `allowExtension` | ✓ | ✗ |  |
| option: `timeout` | ✓ | ✗ |  |
| option: `readBigInts` | ✓ | ✗ |  |
| option: `returnArrays` | ✓ | ✗ |  |
| option: `allowBareNamedParameters` | ✓ | ✗ |  |
| option: `allowUnknownNamedParameters` | ✓ | ✗ |  |
| option: `defensive` | ✓ | ✗ |  |
| option: `limits` | ✓ | ✗ |  |
| `db.close()` | ✓ | ✗ |  |
| `db.exec(sql)` | ✓ | ✗ |  |
| `db.function(name[, options], fn)` | ✓ | ✗ |  |
| `db.aggregate(name, options)` | ✓ | ✗ |  |
| `db.prepare(sql[, options])` | ✓ | ✗ |  |
| `db.applyChangeset(changeset[, options])` | ✓ | ✗ |  |
| `db.createSession([options])` | ✓ | ✗ |  |
| `db.createTagStore([maxSize])` | ✓ | ✗ |  |
| `db.enableLoadExtension(allow)` | ✓ | ✗ |  |
| `db.loadExtension(path)` | ✓ | ✗ |  |
| `db.location([dbName])` | ✓ | ✗ |  |
| `db.open()` | ✓ | ✗ |  |
| `db.enableDefensive(active)` | ✓ | ✗ |  |
| `db.serialize([dbName])` | ✓ | ✗ |  |
| `db.deserialize(buffer[, options])` | ✓ | ✗ |  |
| `db.setAuthorizer(callback)` | ✓ | ✗ |  |
| `db.isOpen` | ✓ | ✗ |  |
| `db.isTransaction` | ✓ | ✗ |  |
| `db.limits` | ✓ | ✗ |  |
| `db[Symbol.dispose]()` | ✓ | ✗ |  |

#### Class: StatementSync

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `stmt.all([namedParameters][, ...anonymousParameters])` | ✓ | ✗ |  |
| `stmt.get([namedParameters][, ...anonymousParameters])` | ✓ | ✗ |  |
| `stmt.iterate([namedParameters][, ...anonymousParameters])` | ✓ | ✗ |  |
| `stmt.run([namedParameters][, ...anonymousParameters])` | ✓ | ✗ |  |
| `stmt.columns()` | ✓ | ✗ |  |
| `stmt.setAllowBareNamedParameters(enabled)` | ✓ | ✗ |  |
| `stmt.setAllowUnknownNamedParameters(enabled)` | ✓ | ✗ |  |
| `stmt.setReadBigInts(enabled)` | ✓ | ✗ |  |
| `stmt.setReturnArrays(enabled)` | ✓ | ✗ |  |
| `stmt.sourceSQL` | ✓ | ✗ |  |
| `stmt.expandedSQL` | ✓ | ✗ |  |

#### Class: Session

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `session.changeset()` | ✓ | ✗ |  |
| `session.patchset()` | ✓ | ✗ |  |
| `session.close()` | ✓ | ✗ |  |
| `session[Symbol.dispose]()` | ✓ | ✗ |  |

#### Class: SQLTagStore

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `tagStore.all` (tagged template) | ✓ | ✗ |  |
| `tagStore.get` | ✓ | ✗ |  |
| `tagStore.iterate` | ✓ | ✗ |  |
| `tagStore.run` | ✓ | ✗ |  |
| `tagStore.size` | ✓ | ✗ |  |
| `tagStore.capacity` | ✓ | ✗ |  |
| `tagStore.db` | ✓ | ✗ |  |
| `tagStore.clear()` | ✓ | ✗ |  |

#### Module-Level Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `sqlite.backup(sourceDb, path[, options])` | ✓ | ✗ |  |

#### Constants (`sqlite.constants`)

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `SQLITE_CHANGESET_DATA` | ✓ | ✗ |  |
| `SQLITE_CHANGESET_NOTFOUND` | ✓ | ✗ |  |
| `SQLITE_CHANGESET_CONFLICT` | ✓ | ✗ |  |
| `SQLITE_CHANGESET_CONSTRAINT` | ✓ | ✗ |  |
| `SQLITE_CHANGESET_FOREIGN_KEY` | ✓ | ✗ |  |
| `SQLITE_CHANGESET_OMIT` | ✓ | ✗ |  |
| `SQLITE_CHANGESET_REPLACE` | ✓ | ✗ |  |
| `SQLITE_CHANGESET_ABORT` | ✓ | ✗ |  |
| `SQLITE_OK` / `SQLITE_DENY` / `SQLITE_IGNORE` | ✓ | ✗ |  |
| `SQLITE_CREATE_INDEX` / `_TABLE` / `_TEMP_INDEX` / `_TEMP_TABLE` / `_TEMP_TRIGGER` / `_TEMP_VIEW` / `_TRIGGER` / `_VIEW` | ✓ | ✗ |  |
| `SQLITE_DELETE` / `SQLITE_DROP_*` | ✓ | ✗ |  |
| `SQLITE_INSERT` / `SQLITE_PRAGMA` / `SQLITE_READ` / `SQLITE_SELECT` / `SQLITE_TRANSACTION` / `SQLITE_UPDATE` | ✓ | ✗ |  |
| `SQLITE_ATTACH` / `SQLITE_DETACH` / `SQLITE_ALTER_TABLE` / `SQLITE_REINDEX` / `SQLITE_ANALYZE` / `SQLITE_CREATE_VTABLE` / `SQLITE_DROP_VTABLE` / `SQLITE_FUNCTION` / `SQLITE_SAVEPOINT` / `SQLITE_COPY` / `SQLITE_RECURSIVE` | ✓ | ✗ |  |

---

### node:punycode

Bun status: 🟢 Fully implemented. 100% of Node.js's test suite passes. **Deprecated by Node.js.**

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `punycode.decode(string)` | ✓ | ✓ | deprecated module |
| `punycode.encode(string)` | ✓ | ✓ | deprecated module |
| `punycode.toASCII(domain)` | ✓ | ✓ | deprecated module |
| `punycode.toUnicode(domain)` | ✓ | ✓ | deprecated module |
| `punycode.ucs2.decode(string)` | ✓ | ✓ | deprecated module |
| `punycode.ucs2.encode(codePoints)` | ✓ | ✓ | deprecated module |

#### Properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `punycode.default` | ✓ | ✓ | deprecated module; CommonJS namespace object |
| `punycode.version` | ✓ | ✓ |  |

---

### node:domain

Bun status: 🟡 Missing `Domain` class export, `domain.active`. **Pending deprecation in Node.js.**

#### Functions

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `domain.create()` | ✓ | ✓ | deprecated module |

#### Module-Level Properties

| Item | Node.js | Bun | Notes |
|------|---------|-----|-------|
| `domain.active` | ✓ | ✗ | missing in Bun |
| `domain.Domain` (class) | ✓ | ✗ | missing in Bun |

#### Class: Domain (extends EventEmitter)

| Member | Node.js | Bun | Notes |
|--------|---------|-----|-------|
| `domain.members` | ✓ | ⚠ |  |
| `domain.add(emitter)` | ✓ | ⚠ |  |
| `domain.bind(callback)` | ✓ | ⚠ |  |
| `domain.intercept(callback)` | ✓ | ⚠ |  |
| `domain.enter()` | ✓ | ⚠ |  |
| `domain.exit()` | ✓ | ⚠ |  |
| `domain.remove(emitter)` | ✓ | ⚠ |  |
| `domain.run(fn[, ...args])` | ✓ | ⚠ |  |

#### Events

| Event | Node.js | Bun | Notes |
|-------|---------|-----|-------|
| `'error'` (with `error.domain`, `domainEmitter`, `domainBound`, `domainThrown` annotations) | ✓ | ⚠ |  |

---

### Summary

| Module | Bun Status |
|--------|-----------|
| node:events | 🟢 Full |
| node:util | 🟡 5 functions missing |
| node:sys | 🟡 alias for util |
| node:os | 🟢 Full |
| node:process | 🟡 minor gaps; `loadEnvFile` missing |
| node:child_process | 🟡 `proc.uid`/`gid`, socket-handle IPC missing |
| node:cluster | 🟡 cross-worker FDs only on Linux |
| node:worker_threads | 🟡 `stdin`/`stdout`/`stderr` etc. options missing |
| node:zlib | 🟢 Full (Zstd experimental) |
| node:querystring | 🟢 Full |
| node:url | 🟢 Full |
| node:vm | 🟡 `measureMemory` + some cachedData missing |
| node:async_hooks | 🟡 v8 promise hooks not called |
| node:perf_hooks | 🟡 implemented but tests don't pass |
| node:timers | 🟢 Full |
| node:timers/promises | 🟢 Full |
| node:tty | 🟢 Full |
| node:v8 | 🟡 limited; serialize uses JSC wire format |
| node:assert | 🟢 Full |
| node:console | 🟢 Full |
| node:module | 🟡 `register`, `syncBuiltinESMExports` missing |
| node:string_decoder | 🟢 Full |
| node:readline | 🟢 Full |
| node:readline/promises | 🟢 Full |
| node:repl | 🔴 Not implemented |
| node:trace_events | 🔴 Not implemented |
| node:inspector | 🟡 Profiler.* only |
| node:diagnostics_channel | 🟢 Full |
| node:wasi | 🟡 Partial |
| node:test | 🟡 mocks, snapshots, timers missing |
| node:sqlite | 🔴 Not implemented (use bun:sqlite) |
| node:punycode | 🟢 Full (Node-deprecated) |
| node:domain | 🟡 `Domain` class missing |
## Web Globals + Bun-only APIs — Parity Inventory

Leaf-level inventory of (A) Web/Global APIs implemented by both Node.js and Bun, and (B) APIs Bun ships that Node.js does not. The Node column reflects stable Node.js status (LTS/current); the Bun column reflects Bun ≥ 1.3.x. `⚠` flags partial/stub/non-spec behavior.

Sources: bun.sh/docs/runtime/web-apis, bun.sh/docs/runtime/bun-apis, bun.sh/docs/api/*, bun.sh/reference/bun/jsc, nodejs.org/api/webcrypto, MDN.

---

### Web / Global APIs

#### Globals (top-level functions / values)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `globalThis` | ✓ | ✓ |  |
| `console` | ✓ | ✓ | Both implement Web Console API + Node extensions |
| `performance` | ✓ | ✓ | High-res timing global |
| `queueMicrotask(cb)` | ✓ | ✓ |  |
| `structuredClone(value, options?)` | ✓ | ✓ | Both delegate to HTML Structured Clone algorithm |
| `atob(b64)` | ✓ | ✓ |  |
| `btoa(str)` | ✓ | ✓ |  |
| `reportError(err)` | ✓ | ✓ |  |
| `fetch(input, init?)` | ✓ | ✓ | Both spec-conformant; Bun additionally accepts `s3://` and `file://` URLs |
| `setTimeout(cb, ms, ...args)` | ✓ | ✓ | Node returns `Timeout` object; Bun returns a `number` (web-spec) but with Node compat for `.unref()` / `.ref()` |
| `setInterval(cb, ms, ...args)` | ✓ | ✓ | Same return-type divergence as `setTimeout` |
| `setImmediate(cb, ...args)` | ✓ | ✓ | Node returns `Immediate`; Bun returns object with `.unref()`/`.ref()` |
| `clearTimeout(id)` | ✓ | ✓ |  |
| `clearInterval(id)` | ✓ | ✓ |  |
| `clearImmediate(id)` | ✓ | ✓ |  |
| `__dirname` | ✓ | ✓ | CJS module scope only |
| `__filename` | ✓ | ✓ | CJS module scope only |
| `require(id)` | ✓ | ✓ | CJS-style; also injected into ESM under Bun |
| `module` / `exports` | ✓ | ✓ | CJS module scope only |
| `alert(msg)` | ✗ | ✓ | Browser-style; Bun for CLIs only — not in Node |
| `confirm(msg)` | ✗ | ✓ | Browser-style; CLIs only |
| `prompt(msg, default?)` | ✗ | ✓ | Browser-style; CLIs only |
| `ShadowRealm` | ⚠ | ✓ | Node: unflagged ShadowRealm not yet shipped; Bun ships tc39 proposal |
| `process` | ✓ | ✓ | Node-style global; Bun also exposes |
| `Buffer` | ✓ | ✓ | Node-style global; Bun also exposes |
| `URL` (global) | ✓ | ✓ |  |
| `URLSearchParams` (global) | ✓ | ✓ |  |

#### URL

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class URL` | ✓ | ✓ |  |
| `new URL(input, base?)` | ✓ | ✓ |  |
| `URL.canParse(input, base?)` | ✓ | ✓ |  |
| `URL.parse(input, base?)` | ✓ | ✓ | Returns URL or null |
| `URL.createObjectURL(blob)` | ✓ | ✓ |  |
| `URL.revokeObjectURL(url)` | ✓ | ✓ |  |
| `url.href` | ✓ | ✓ |  |
| `url.origin` | ✓ | ✓ |  |
| `url.protocol` | ✓ | ✓ |  |
| `url.username` | ✓ | ✓ |  |
| `url.password` | ✓ | ✓ |  |
| `url.host` | ✓ | ✓ |  |
| `url.hostname` | ✓ | ✓ |  |
| `url.port` | ✓ | ✓ |  |
| `url.pathname` | ✓ | ✓ |  |
| `url.search` | ✓ | ✓ |  |
| `url.searchParams` | ✓ | ✓ | Returns URLSearchParams |
| `url.hash` | ✓ | ✓ |  |
| `url.toString()` | ✓ | ✓ |  |
| `url.toJSON()` | ✓ | ✓ |  |

#### URLSearchParams

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class URLSearchParams` | ✓ | ✓ |  |
| `new URLSearchParams(init?)` | ✓ | ✓ | Accepts string / iterable / record / another USP |
| `usp.append(name, value)` | ✓ | ✓ |  |
| `usp.delete(name, value?)` | ✓ | ✓ |  |
| `usp.entries()` | ✓ | ✓ |  |
| `usp.forEach(cb, thisArg?)` | ✓ | ✓ |  |
| `usp.get(name)` | ✓ | ✓ |  |
| `usp.getAll(name)` | ✓ | ✓ |  |
| `usp.has(name, value?)` | ✓ | ✓ |  |
| `usp.keys()` | ✓ | ✓ |  |
| `usp.set(name, value)` | ✓ | ✓ |  |
| `usp.size` | ✓ | ✓ |  |
| `usp.sort()` | ✓ | ✓ |  |
| `usp.toString()` | ✓ | ✓ |  |
| `usp.values()` | ✓ | ✓ |  |
| `usp[Symbol.iterator]()` | ✓ | ✓ |  |

#### Headers

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class Headers` | ✓ | ✓ |  |
| `new Headers(init?)` | ✓ | ✓ |  |
| `headers.append(name, value)` | ✓ | ✓ |  |
| `headers.delete(name)` | ✓ | ✓ |  |
| `headers.get(name)` | ✓ | ✓ |  |
| `headers.getSetCookie()` | ✓ | ✓ |  |
| `headers.has(name)` | ✓ | ✓ |  |
| `headers.set(name, value)` | ✓ | ✓ |  |
| `headers.forEach(cb)` | ✓ | ✓ |  |
| `headers.entries()` | ✓ | ✓ |  |
| `headers.keys()` | ✓ | ✓ |  |
| `headers.values()` | ✓ | ✓ |  |
| `headers[Symbol.iterator]()` | ✓ | ✓ |  |
| `headers.toJSON()` | ✗ | ✓ | Bun extension |
| `headers.count` | ✗ | ✓ | Bun extension |
| `headers.getAll(name)` | ✗ | ✓ | Bun extension |

#### Request

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class Request` | ✓ | ✓ |  |
| `new Request(input, init?)` | ✓ | ✓ |  |
| `request.method` | ✓ | ✓ |  |
| `request.url` | ✓ | ✓ |  |
| `request.headers` | ✓ | ✓ |  |
| `request.destination` | ✓ | ✓ |  |
| `request.referrer` | ✓ | ✓ |  |
| `request.referrerPolicy` | ✓ | ✓ |  |
| `request.mode` | ✓ | ✓ |  |
| `request.credentials` | ✓ | ✓ |  |
| `request.cache` | ✓ | ✓ |  |
| `request.redirect` | ✓ | ✓ |  |
| `request.integrity` | ✓ | ✓ |  |
| `request.keepalive` | ✓ | ✓ |  |
| `request.signal` | ✓ | ✓ |  |
| `request.body` | ✓ | ✓ |  |
| `request.bodyUsed` | ✓ | ✓ |  |
| `request.duplex` | ✓ | ✓ |  |
| `request.arrayBuffer()` | ✓ | ✓ |  |
| `request.blob()` | ✓ | ✓ |  |
| `request.bytes()` | ✓ | ✓ |  |
| `request.clone()` | ✓ | ✓ |  |
| `request.formData()` | ✓ | ✓ |  |
| `request.json()` | ✓ | ✓ |  |
| `request.text()` | ✓ | ✓ |  |

#### Response

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class Response` | ✓ | ✓ |  |
| `new Response(body?, init?)` | ✓ | ✓ |  |
| `Response.error()` | ✓ | ✓ |  |
| `Response.json(data, init?)` | ✓ | ✓ |  |
| `Response.redirect(url, status?)` | ✓ | ✓ |  |
| `response.body` | ✓ | ✓ |  |
| `response.bodyUsed` | ✓ | ✓ |  |
| `response.headers` | ✓ | ✓ |  |
| `response.ok` | ✓ | ✓ |  |
| `response.redirected` | ✓ | ✓ |  |
| `response.status` | ✓ | ✓ |  |
| `response.statusText` | ✓ | ✓ |  |
| `response.type` | ✓ | ✓ |  |
| `response.url` | ✓ | ✓ |  |
| `response.arrayBuffer()` | ✓ | ✓ |  |
| `response.blob()` | ✓ | ✓ |  |
| `response.bytes()` | ✓ | ✓ |  |
| `response.clone()` | ✓ | ✓ |  |
| `response.formData()` | ✓ | ✓ |  |
| `response.json()` | ✓ | ✓ |  |
| `response.text()` | ✓ | ✓ |  |

#### Blob

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class Blob` | ✓ | ✓ |  |
| `new Blob(parts?, options?)` | ✓ | ✓ |  |
| `blob.size` | ✓ | ✓ |  |
| `blob.type` | ✓ | ✓ |  |
| `blob.arrayBuffer()` | ✓ | ✓ |  |
| `blob.bytes()` | ✓ | ✓ |  |
| `blob.slice(start?, end?, type?)` | ✓ | ✓ |  |
| `blob.stream()` | ✓ | ✓ |  |
| `blob.text()` | ✓ | ✓ |  |
| `blob.json()` | ✗ | ✓ | Bun extension; Node has no `.json()` on Blob |
| `blob.formData()` | ✗ | ✓ | Bun extension |
| `blob.name` | ✗ | ✓ | Bun extension (when from Bun.file) |
| `blob.lastModified` | ✗ | ✓ | Bun extension (when from Bun.file) |

#### File

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class File` | ✓ | ✓ |  |
| `new File(parts, name, options?)` | ✓ | ✓ |  |
| `file.name` | ✓ | ✓ |  |
| `file.lastModified` | ✓ | ✓ |  |
| `file.webkitRelativePath` | ✓ | ✓ |  |
| (inherits all `Blob` methods) | ✓ | ✓ |  |

#### FormData

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class FormData` | ✓ | ✓ |  |
| `new FormData()` | ✓ | ✓ |  |
| `fd.append(name, value, filename?)` | ✓ | ✓ |  |
| `fd.delete(name)` | ✓ | ✓ |  |
| `fd.get(name)` | ✓ | ✓ |  |
| `fd.getAll(name)` | ✓ | ✓ |  |
| `fd.has(name)` | ✓ | ✓ |  |
| `fd.set(name, value, filename?)` | ✓ | ✓ |  |
| `fd.entries()` | ✓ | ✓ |  |
| `fd.keys()` | ✓ | ✓ |  |
| `fd.values()` | ✓ | ✓ |  |
| `fd.forEach(cb)` | ✓ | ✓ |  |
| `fd[Symbol.iterator]()` | ✓ | ✓ |  |

#### ReadableStream + readers/controllers

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class ReadableStream` | ✓ | ✓ |  |
| `new ReadableStream(underlyingSource?, queuingStrategy?)` | ✓ | ✓ |  |
| `ReadableStream.from(iterable)` | ✓ | ✓ |  |
| `rs.locked` | ✓ | ✓ |  |
| `rs.cancel(reason?)` | ✓ | ✓ |  |
| `rs.getReader(options?)` | ✓ | ✓ |  |
| `rs.pipeThrough(transform, options?)` | ✓ | ✓ |  |
| `rs.pipeTo(dest, options?)` | ✓ | ✓ |  |
| `rs.tee()` | ✓ | ✓ |  |
| `rs.values(options?)` | ✓ | ✓ |  |
| `rs[Symbol.asyncIterator]()` | ✓ | ✓ |  |
| `class ReadableStreamDefaultReader` | ✓ | ✓ |  |
| `reader.read()` | ✓ | ✓ |  |
| `reader.releaseLock()` | ✓ | ✓ |  |
| `reader.cancel(reason?)` | ✓ | ✓ |  |
| `reader.closed` | ✓ | ✓ |  |
| `class ReadableStreamBYOBReader` | ✓ | ✓ |  |
| `byob.read(view, options?)` | ✓ | ✓ |  |
| `byob.releaseLock()` | ✓ | ✓ |  |
| `byob.cancel(reason?)` | ✓ | ✓ |  |
| `class ReadableStreamDefaultController` | ✓ | ✓ |  |
| `controller.desiredSize` | ✓ | ✓ |  |
| `controller.close()` | ✓ | ✓ |  |
| `controller.enqueue(chunk)` | ✓ | ✓ |  |
| `controller.error(e?)` | ✓ | ✓ |  |
| `class ReadableByteStreamController` | ✓ | ✓ |  |
| `controller.byobRequest` | ✓ | ✓ |  |
| `class ReadableStreamBYOBRequest` | ✓ | ✓ |  |
| `req.view` | ✓ | ✓ |  |
| `req.respond(bytesWritten)` | ✓ | ✓ |  |
| `req.respondWithNewView(view)` | ✓ | ✓ |  |

#### WritableStream

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class WritableStream` | ✓ | ✓ |  |
| `new WritableStream(underlyingSink?, queuingStrategy?)` | ✓ | ✓ |  |
| `ws.locked` | ✓ | ✓ |  |
| `ws.abort(reason?)` | ✓ | ✓ |  |
| `ws.close()` | ✓ | ✓ |  |
| `ws.getWriter()` | ✓ | ✓ |  |
| `class WritableStreamDefaultWriter` | ✓ | ✓ |  |
| `writer.desiredSize` | ✓ | ✓ |  |
| `writer.ready` | ✓ | ✓ |  |
| `writer.closed` | ✓ | ✓ |  |
| `writer.abort(reason?)` | ✓ | ✓ |  |
| `writer.close()` | ✓ | ✓ |  |
| `writer.releaseLock()` | ✓ | ✓ |  |
| `writer.write(chunk)` | ✓ | ✓ |  |
| `class WritableStreamDefaultController` | ✓ | ✓ |  |
| `controller.signal` | ✓ | ✓ |  |
| `controller.error(e?)` | ✓ | ✓ |  |

#### TransformStream

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class TransformStream` | ✓ | ✓ |  |
| `new TransformStream(transformer?, writableStrategy?, readableStrategy?)` | ✓ | ✓ |  |
| `ts.readable` | ✓ | ✓ |  |
| `ts.writable` | ✓ | ✓ |  |
| `class TransformStreamDefaultController` | ✓ | ✓ |  |
| `controller.desiredSize` | ✓ | ✓ |  |
| `controller.enqueue(chunk)` | ✓ | ✓ |  |
| `controller.error(e?)` | ✓ | ✓ |  |
| `controller.terminate()` | ✓ | ✓ |  |

#### Queuing strategies

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class ByteLengthQueuingStrategy` | ✓ | ✓ |  |
| `new ByteLengthQueuingStrategy({highWaterMark})` | ✓ | ✓ |  |
| `strategy.highWaterMark` | ✓ | ✓ |  |
| `strategy.size(chunk)` | ✓ | ✓ |  |
| `class CountQueuingStrategy` | ✓ | ✓ |  |
| `new CountQueuingStrategy({highWaterMark})` | ✓ | ✓ |  |
| `strategy.highWaterMark` | ✓ | ✓ |  |
| `strategy.size()` | ✓ | ✓ |  |

#### AbortController / AbortSignal

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class AbortController` | ✓ | ✓ |  |
| `new AbortController()` | ✓ | ✓ |  |
| `ac.signal` | ✓ | ✓ |  |
| `ac.abort(reason?)` | ✓ | ✓ |  |
| `class AbortSignal` | ✓ | ✓ |  |
| `AbortSignal.abort(reason?)` | ✓ | ✓ |  |
| `AbortSignal.timeout(ms)` | ✓ | ✓ |  |
| `AbortSignal.any(signals)` | ✓ | ✓ |  |
| `signal.aborted` | ✓ | ✓ |  |
| `signal.reason` | ✓ | ✓ |  |
| `signal.throwIfAborted()` | ✓ | ✓ |  |
| `signal.onabort` | ✓ | ✓ |  |
| event: `"abort"` | ✓ | ✓ |  |

#### Encoding (TextEncoder / TextDecoder)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class TextEncoder` | ✓ | ✓ |  |
| `new TextEncoder()` | ✓ | ✓ | Only `utf-8` per spec |
| `enc.encoding` | ✓ | ✓ |  |
| `enc.encode(input?)` | ✓ | ✓ |  |
| `enc.encodeInto(src, dest)` | ✓ | ✓ |  |
| `class TextDecoder` | ✓ | ✓ |  |
| `new TextDecoder(label?, options?)` | ✓ | ✓ |  |
| `dec.encoding` | ✓ | ✓ |  |
| `dec.fatal` | ✓ | ✓ |  |
| `dec.ignoreBOM` | ✓ | ✓ |  |
| `dec.decode(input?, options?)` | ✓ | ✓ |  |
| `class TextEncoderStream` | ✓ | ✓ |  |
| `new TextEncoderStream()` | ✓ | ✓ |  |
| `tes.encoding` | ✓ | ✓ |  |
| `tes.readable` | ✓ | ✓ |  |
| `tes.writable` | ✓ | ✓ |  |
| `class TextDecoderStream` | ✓ | ✓ |  |
| `new TextDecoderStream(label?, options?)` | ✓ | ✓ |  |
| `tds.encoding` | ✓ | ✓ |  |
| `tds.fatal` | ✓ | ✓ |  |
| `tds.ignoreBOM` | ✓ | ✓ |  |
| `tds.readable` | ✓ | ✓ |  |
| `tds.writable` | ✓ | ✓ |  |

#### DOM-style events

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class Event` | ✓ | ✓ |  |
| `new Event(type, init?)` | ✓ | ✓ |  |
| `event.type` | ✓ | ✓ |  |
| `event.target` | ✓ | ✓ |  |
| `event.currentTarget` | ✓ | ✓ |  |
| `event.eventPhase` | ✓ | ✓ |  |
| `event.bubbles` | ✓ | ✓ |  |
| `event.cancelable` | ✓ | ✓ |  |
| `event.defaultPrevented` | ✓ | ✓ |  |
| `event.composed` | ✓ | ✓ |  |
| `event.isTrusted` | ✓ | ✓ |  |
| `event.timeStamp` | ✓ | ✓ |  |
| `event.composedPath()` | ✓ | ✓ |  |
| `event.preventDefault()` | ✓ | ✓ |  |
| `event.stopImmediatePropagation()` | ✓ | ✓ |  |
| `event.stopPropagation()` | ✓ | ✓ |  |
| `class EventTarget` | ✓ | ✓ |  |
| `new EventTarget()` | ✓ | ✓ |  |
| `et.addEventListener(type, listener, options?)` | ✓ | ✓ |  |
| `et.removeEventListener(type, listener, options?)` | ✓ | ✓ |  |
| `et.dispatchEvent(event)` | ✓ | ✓ |  |
| `class CustomEvent` | ✓ | ✓ |  |
| `new CustomEvent(type, init?)` | ✓ | ✓ |  |
| `ce.detail` | ✓ | ✓ |  |
| `class MessageEvent` | ✓ | ✓ |  |
| `new MessageEvent(type, init?)` | ✓ | ✓ |  |
| `me.data` | ✓ | ✓ |  |
| `me.origin` | ✓ | ✓ |  |
| `me.lastEventId` | ✓ | ✓ |  |
| `me.source` | ✓ | ✓ |  |
| `me.ports` | ✓ | ✓ |  |
| `class ErrorEvent` | ✓ | ✓ |  |
| `new ErrorEvent(type, init?)` | ✓ | ✓ |  |
| `ee.message` | ✓ | ✓ |  |
| `ee.filename` | ✓ | ✓ |  |
| `ee.lineno` | ✓ | ✓ |  |
| `ee.colno` | ✓ | ✓ |  |
| `ee.error` | ✓ | ✓ |  |
| `class CloseEvent` | ✓ | ✓ |  |
| `new CloseEvent(type, init?)` | ✓ | ✓ |  |
| `ce.wasClean` | ✓ | ✓ |  |
| `ce.code` | ✓ | ✓ |  |
| `ce.reason` | ✓ | ✓ |  |
| `class PromiseRejectionEvent` | ✓ | ✓ |  |
| `pre.promise` | ✓ | ✓ |  |
| `pre.reason` | ✓ | ✓ |  |

#### Channel messaging

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class MessageChannel` | ✓ | ✓ |  |
| `new MessageChannel()` | ✓ | ✓ |  |
| `mc.port1` | ✓ | ✓ |  |
| `mc.port2` | ✓ | ✓ |  |
| `class MessagePort` | ✓ | ✓ |  |
| `port.postMessage(data, transferList?)` | ✓ | ✓ |  |
| `port.start()` | ✓ | ✓ |  |
| `port.close()` | ✓ | ✓ |  |
| `port.onmessage` | ✓ | ✓ |  |
| `port.onmessageerror` | ✓ | ✓ |  |
| `port.ref()` | ✓ | ✓ | Node extension; Bun mirrors |
| `port.unref()` | ✓ | ✓ | Node extension; Bun mirrors |
| event: `"message"` | ✓ | ✓ |  |
| event: `"messageerror"` | ✓ | ✓ |  |
| event: `"close"` | ✓ | ✓ |  |
| `class BroadcastChannel` | ✓ | ✓ |  |
| `new BroadcastChannel(name)` | ✓ | ✓ |  |
| `bc.name` | ✓ | ✓ |  |
| `bc.postMessage(data)` | ✓ | ✓ |  |
| `bc.close()` | ✓ | ✓ |  |
| `bc.onmessage` | ✓ | ✓ |  |
| `bc.onmessageerror` | ✓ | ✓ |  |
| `bc.ref()` | ✓ | ✓ | Node extension |
| `bc.unref()` | ✓ | ✓ | Node extension |
| event: `"message"` | ✓ | ✓ |  |
| event: `"messageerror"` | ✓ | ✓ |  |

#### WebSocket (client)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class WebSocket` | ✓ | ✓ | Node 22+ stable; Bun has had it for years |
| `new WebSocket(url, protocols?)` | ✓ | ✓ |  |
| `ws.url` | ✓ | ✓ |  |
| `ws.readyState` | ✓ | ✓ |  |
| `ws.bufferedAmount` | ✓ | ✓ |  |
| `ws.extensions` | ✓ | ✓ |  |
| `ws.protocol` | ✓ | ✓ |  |
| `ws.binaryType` | ✓ | ✓ |  |
| `ws.send(data)` | ✓ | ✓ |  |
| `ws.close(code?, reason?)` | ✓ | ✓ |  |
| `ws.ping(data?)` | ✗ | ✓ | Bun extension (Node lacks ping on client WebSocket) |
| `ws.pong(data?)` | ✗ | ✓ | Bun extension |
| `ws.terminate()` | ✗ | ✓ | Bun extension |
| `ws.onopen` | ✓ | ✓ |  |
| `ws.onmessage` | ✓ | ✓ |  |
| `ws.onerror` | ✓ | ✓ |  |
| `ws.onclose` | ✓ | ✓ |  |
| `WebSocket.CONNECTING / OPEN / CLOSING / CLOSED` | ✓ | ✓ |  |
| event: `"open"` | ✓ | ✓ |  |
| event: `"message"` | ✓ | ✓ |  |
| event: `"error"` | ✓ | ✓ |  |
| event: `"close"` | ✓ | ✓ |  |

#### WebCrypto — `globalThis.crypto`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `crypto.getRandomValues(typedArray)` | ✓ | ✓ | Max 65536 bytes per call; throws for Float32/64Array |
| `crypto.randomUUID()` | ✓ | ✓ | RFC 4122 v4 |
| `crypto.subtle` | ✓ | ✓ | SubtleCrypto instance |
| `crypto.timingSafeEqual(a, b)` | ✓ | ⚠ | Bun: only via `node:crypto` import — not on global WebCrypto in browser-spec sense |

#### WebCrypto — `crypto.subtle` (SubtleCrypto)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `subtle.encrypt(algorithm, key, data)` | ✓ | ✓ | AES-CBC, AES-CTR, AES-GCM, AES-OCB, ChaCha20-Poly1305, RSA-OAEP |
| `subtle.decrypt(algorithm, key, data)` | ✓ | ✓ | Same algorithm set |
| `subtle.sign(algorithm, key, data)` | ✓ | ✓ | ECDSA, Ed25519, Ed448, HMAC, RSA-PSS, RSASSA-PKCS1-v1_5; Node 24 adds KMAC/ML-DSA |
| `subtle.verify(algorithm, key, signature, data)` | ✓ | ✓ | Same set |
| `subtle.digest(algorithm, data)` | ✓ | ✓ | SHA-1, SHA-256, SHA-384, SHA-512; SHA3-* per Node 24, Bun parity varies |
| `subtle.generateKey(algorithm, extractable, keyUsages)` | ✓ | ✓ |  |
| `subtle.deriveKey(algorithm, baseKey, derivedKeyAlgo, extractable, usages)` | ✓ | ✓ | HKDF, PBKDF2, ECDH, X25519, X448 |
| `subtle.deriveBits(algorithm, baseKey, length)` | ✓ | ✓ |  |
| `subtle.importKey(format, keyData, algorithm, extractable, usages)` | ✓ | ✓ | raw / pkcs8 / spki / jwk |
| `subtle.exportKey(format, key)` | ✓ | ✓ |  |
| `subtle.wrapKey(format, key, wrappingKey, wrapAlgo)` | ✓ | ✓ |  |
| `subtle.unwrapKey(format, wrappedKey, unwrappingKey, unwrapAlgo, unwrappedKeyAlgo, extractable, usages)` | ✓ | ✓ |  |
| `subtle.encapsulateBits(...)` | ✓ | ⚠ | Node 24+ post-quantum (ML-KEM); Bun: not yet |
| `subtle.decapsulateBits(...)` | ✓ | ⚠ | Same |
| `subtle.encapsulateKey(...)` | ✓ | ⚠ | Same |
| `subtle.decapsulateKey(...)` | ✓ | ⚠ | Same |
| `subtle.getPublicKey(key, usages)` | ✓ | ⚠ | Node 24+; Bun: not yet |
| `SubtleCrypto.supports(op, algo, len?)` | ✓ | ⚠ | Node 24+; Bun: partial |
| algorithm `AES-CBC` | ✓ | ✓ |  |
| algorithm `AES-CTR` | ✓ | ✓ |  |
| algorithm `AES-GCM` | ✓ | ✓ |  |
| algorithm `AES-KW` | ✓ | ✓ | wrap/unwrap |
| algorithm `AES-OCB` | ✓ | ⚠ | Node 24+ only |
| algorithm `ChaCha20-Poly1305` | ✓ | ⚠ | Node 24+ only |
| algorithm `RSA-OAEP` | ✓ | ✓ |  |
| algorithm `RSA-PSS` | ✓ | ✓ |  |
| algorithm `RSASSA-PKCS1-v1_5` | ✓ | ✓ |  |
| algorithm `ECDSA` | ✓ | ✓ |  |
| algorithm `ECDH` | ✓ | ✓ |  |
| algorithm `Ed25519` | ✓ | ✓ |  |
| algorithm `Ed448` | ✓ | ⚠ | Node 24+; Bun varies |
| algorithm `X25519` | ✓ | ✓ |  |
| algorithm `X448` | ✓ | ⚠ |  |
| algorithm `HKDF` | ✓ | ✓ |  |
| algorithm `PBKDF2` | ✓ | ✓ |  |
| algorithm `Argon2id / Argon2i / Argon2d` | ✓ | ⚠ | Node 24+ subtle; Bun via `Bun.password` instead |
| algorithm `HMAC` | ✓ | ✓ |  |
| algorithm `KMAC128 / KMAC256` | ✓ | ⚠ | Node 24+; Bun: not yet |
| algorithm `ML-DSA-44 / 65 / 87` | ✓ | ✗ | Node 24+ post-quantum |
| algorithm `ML-KEM-512 / 768 / 1024` | ✓ | ✗ | Node 24+ post-quantum |
| digest `SHA-1` | ✓ | ✓ |  |
| digest `SHA-256` | ✓ | ✓ |  |
| digest `SHA-384` | ✓ | ✓ |  |
| digest `SHA-512` | ✓ | ✓ |  |
| digest `SHA3-256/384/512` | ✓ | ⚠ | Node 24+; Bun via `Bun.CryptoHasher` |
| digest `cSHAKE128 / cSHAKE256 / TurboSHAKE128 / TurboSHAKE256 / KT128 / KT256` | ✓ | ✗ | Node 24+ only |

#### CryptoKey

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class CryptoKey` | ✓ | ✓ |  |
| `key.algorithm` | ✓ | ✓ |  |
| `key.extractable` | ✓ | ✓ |  |
| `key.type` | ✓ | ✓ | `"secret"` / `"private"` / `"public"` |
| `key.usages` | ✓ | ✓ |  |

#### Performance

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `performance.now()` | ✓ | ✓ |  |
| `performance.timeOrigin` | ✓ | ✓ |  |
| `performance.mark(name, options?)` | ✓ | ✓ |  |
| `performance.measure(name, startOrOptions?, endMark?)` | ✓ | ✓ |  |
| `performance.clearMarks(name?)` | ✓ | ✓ |  |
| `performance.clearMeasures(name?)` | ✓ | ✓ |  |
| `performance.clearResourceTimings()` | ✓ | ✓ |  |
| `performance.getEntries()` | ✓ | ✓ |  |
| `performance.getEntriesByName(name, type?)` | ✓ | ✓ |  |
| `performance.getEntriesByType(type)` | ✓ | ✓ |  |
| `performance.setResourceTimingBufferSize(n)` | ✓ | ✓ |  |
| `performance.toJSON()` | ✓ | ✓ |  |
| `performance.eventLoopUtilization()` | ✓ | ⚠ | Node-specific; Bun: partial |
| `performance.timerify(fn, options?)` | ✓ | ⚠ | Node-specific |
| `performance.nodeTiming` | ✓ | ⚠ | Node-only |
| `class PerformanceObserver` | ✓ | ✓ |  |
| `new PerformanceObserver(cb)` | ✓ | ✓ |  |
| `po.observe(options)` | ✓ | ✓ |  |
| `po.disconnect()` | ✓ | ✓ |  |
| `po.takeRecords()` | ✓ | ✓ |  |
| `PerformanceObserver.supportedEntryTypes` | ✓ | ✓ |  |
| `class PerformanceEntry` | ✓ | ✓ |  |
| `entry.name / entryType / startTime / duration / toJSON()` | ✓ | ✓ |  |
| `class PerformanceMark` | ✓ | ✓ |  |
| `mark.detail` | ✓ | ✓ |  |
| `class PerformanceMeasure` | ✓ | ✓ |  |
| `measure.detail` | ✓ | ✓ |  |
| `class PerformanceResourceTiming` | ✓ | ✓ |  |
| (all resource-timing fields: `connectStart`, `connectEnd`, `domainLookupStart`, etc.) | ✓ | ⚠ | Subset only |

#### console

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `console.log(...args)` | ✓ | ✓ |  |
| `console.info(...args)` | ✓ | ✓ |  |
| `console.warn(...args)` | ✓ | ✓ |  |
| `console.error(...args)` | ✓ | ✓ |  |
| `console.debug(...args)` | ✓ | ✓ |  |
| `console.trace(...args)` | ✓ | ✓ |  |
| `console.assert(cond, ...args)` | ✓ | ✓ |  |
| `console.count(label?)` | ✓ | ✓ |  |
| `console.countReset(label?)` | ✓ | ✓ |  |
| `console.dir(obj, options?)` | ✓ | ✓ |  |
| `console.dirxml(...args)` | ✓ | ✓ |  |
| `console.group(...args)` | ✓ | ✓ |  |
| `console.groupCollapsed(...args)` | ✓ | ✓ |  |
| `console.groupEnd()` | ✓ | ✓ |  |
| `console.table(data, columns?)` | ✓ | ✓ |  |
| `console.time(label?)` | ✓ | ✓ |  |
| `console.timeEnd(label?)` | ✓ | ✓ |  |
| `console.timeLog(label?, ...args)` | ✓ | ✓ |  |
| `console.timeStamp(label?)` | ✓ | ✓ |  |
| `console.profile / profileEnd` | ✓ | ✓ |  |
| `console.clear()` | ✓ | ✓ |  |
| `console[Symbol.asyncIterator]()` | ✗ | ✓ | Bun extension — `for await (const line of console)` reads stdin |
| `console.write(buf)` | ✗ | ✓ | Bun extension |

#### Workers (web Worker shape)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class Worker` (global) | ✓ | ✓ | Node has it under `node:worker_threads`, Bun also as global |
| `new Worker(url, options?)` | ✓ | ✓ | Options: `name`, `type`, `credentials`, `preload`, `smol`, `ref`, `env`, `argv` etc. |
| `worker.postMessage(data, transferList?)` | ✓ | ✓ |  |
| `worker.terminate()` | ✓ | ✓ |  |
| `worker.ref()` | ✓ | ✓ | Not in browser spec; both runtimes ship it |
| `worker.unref()` | ✓ | ✓ |  |
| `worker.threadId` | ✓ | ✓ | Bun extension to align with Node |
| `worker.onmessage` | ✓ | ✓ |  |
| `worker.onerror` | ✓ | ✓ |  |
| `worker.onmessageerror` | ✓ | ✓ |  |
| `worker.onopen` | ✗ | ✓ | Bun extension fires when worker ready |
| `worker.onclose` | ✗ | ✓ | Bun extension |
| event: `"message"` | ✓ | ✓ |  |
| event: `"error"` | ✓ | ✓ |  |
| event: `"messageerror"` | ✓ | ✓ |  |
| event: `"open"` | ✗ | ✓ | Bun extension |
| event: `"close"` | ✗ | ✓ | Bun extension |
| inside worker: `self.postMessage` | ✓ | ✓ |  |
| inside worker: `self.onmessage` | ✓ | ✓ |  |
| inside worker: `self.close()` | ✓ | ✓ |  |

#### Compression streams

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class CompressionStream` | ✓ | ✓ | Formats: `"gzip"`, `"deflate"`, `"deflate-raw"` |
| `new CompressionStream(format)` | ✓ | ✓ |  |
| `cs.readable` | ✓ | ✓ |  |
| `cs.writable` | ✓ | ✓ |  |
| `class DecompressionStream` | ✓ | ✓ | Same format set |
| `new DecompressionStream(format)` | ✓ | ✓ |  |
| `ds.readable` | ✓ | ✓ |  |
| `ds.writable` | ✓ | ✓ |  |

---

### Bun-only APIs

These are exclusively on `Bun.*` (or built-in modules like `bun:*`); Node.js has no equivalent or only a different shape (called out in Notes).

#### `Bun` namespace — Globals & meta

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.version` | ✗ | ✓ | Bun version string. Node analog: `process.versions.node` |
| `Bun.revision` | ✗ | ✓ | Git SHA of Bun binary |
| `Bun.env` | ✗ | ✓ | Alias for `process.env` |
| `Bun.main` | ✗ | ✓ | Entrypoint absolute path. Node analog: `require.main.filename` |
| `Bun.argv` | ✗ | ✓ | Alias for `process.argv` |
| `Bun.isMainThread` | ✗ | ✓ | Node analog: `worker_threads.isMainThread` |
| `Bun.stdin` | ✗ | ✓ | `BunFile`. Node analog: `process.stdin` (different shape — Readable stream) |
| `Bun.stdout` | ✗ | ✓ | `BunFile`. Node analog: `process.stdout` |
| `Bun.stderr` | ✗ | ✓ | `BunFile`. Node analog: `process.stderr` |
| `Bun.nanoseconds()` | ✗ | ✓ | High-res timer (since process start). Node analog: `process.hrtime.bigint()` |
| `Bun.sleep(ms)` / `Bun.sleep(date)` | ✗ | ✓ | Returns Promise. Node analog: `timers/promises.setTimeout` |
| `Bun.sleepSync(ms)` | ✗ | ✓ | Blocking. Node analog: none (use `Atomics.wait` workaround) |
| `Bun.which(bin, options?)` | ✗ | ✓ | Resolves executable in PATH. Node analog: `which` npm pkg |
| `Bun.openInEditor(path, options?)` | ✗ | ✓ | Open file in editor (`$VISUAL`/`$EDITOR`) |
| `Bun.randomUUIDv7(encoding?, timestamp?)` | ✗ | ✓ | UUID v7. Node 24+ adds `crypto.randomUUID({ disableEntropyCache })` but no v7 |
| `Bun.peek(promise)` | ✗ | ✓ | Synchronously read settled promise |
| `Bun.peek.status(promise)` | ✗ | ✓ | `"pending"` / `"fulfilled"` / `"rejected"` |
| `Bun.deepEquals(a, b, strict?)` | ✗ | ✓ | Recursive equality |
| `Bun.deepMatch(a, b)` | ✗ | ✓ | Partial match |
| `Bun.inspect(value, options?)` | ✗ | ✓ | Like `util.inspect`. Bun-specific |
| `Bun.inspect.custom` | ✗ | ✓ | Symbol (identical to `util.inspect.custom`) |
| `Bun.inspect.table(data, columns?, options?)` | ✗ | ✓ | Returns string (not prints) |
| `Bun.escapeHTML(value)` | ✗ | ✓ | HTML entity escape |
| `Bun.stringWidth(input, options?)` | ✗ | ✓ | Terminal column count. Drop-in for `string-width` npm |
| `Bun.stripANSI(text)` | ✗ | ✓ | Drop-in for `strip-ansi` npm |
| `Bun.wrapAnsi(input, columns, options?)` | ✗ | ✓ | Drop-in for `wrap-ansi` npm |
| `Bun.fileURLToPath(url)` | ✗ | ✓ | Node analog: `url.fileURLToPath()` |
| `Bun.pathToFileURL(path)` | ✗ | ✓ | Node analog: `url.pathToFileURL()` |
| `Bun.resolveSync(specifier, root)` | ✗ | ✓ | Sync module resolution. Node analog: `require.resolve` |
| `Bun.resolve(specifier, root)` | ✗ | ✓ | Async variant |
| `Bun.allocUnsafe(n)` | ✗ | ✓ | Uninitialized `Uint8Array`. Node analog: `Buffer.allocUnsafe` |
| `Bun.concatArrayBuffers(buffers, maxLength?, asUint8Array?)` | ✗ | ✓ |  |
| `Bun.gc(force?)` | ✗ | ✓ | Trigger GC. Node analog: `global.gc()` only with `--expose-gc` |
| `Bun.generateHeapSnapshot()` | ✗ | ✓ | Node analog: `v8.writeHeapSnapshot()` (different shape) |
| `Bun.mmap(path, options?)` | ✗ | ✓ | Memory-map file. No Node analog |
| `Bun.indexOfLine(buf, offset?)` | ✗ | ✓ | Find next newline byte offset |
| `Bun.plugin(plugin)` | ✗ | ✓ | Bundler/loader plugin registration |
| `Bun.build(options)` | ✗ | ✓ | Programmatic bundler. Node has no built-in bundler |
| `Bun.Transpiler` (class) | ✗ | ✓ | TypeScript/JSX transpiler |
| `Bun.FileSystemRouter` (class) | ✗ | ✓ | Next.js-style filesystem router |
| `Bun.HTMLRewriter` (class) | ✗ | ✓ | Streaming HTML rewriter (Cloudflare-compat) |
| `Bun.embeddedFiles` | ✗ | ✓ | Array of embedded `Blob`s from `bun build --compile` |

#### `Bun.serve` — HTTP/WS server

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.serve(options)` | ✗ | ✓ | Node analog: `http.createServer` / Fastify / Hono — different shape |
| option `port` | ✗ | ✓ | Defaults to `$BUN_PORT` / `$PORT` / `$NODE_PORT` else `3000` |
| option `hostname` | ✗ | ✓ |  |
| option `unix` | ✗ | ✓ | Unix domain socket path (`\0` prefix = abstract namespace) |
| option `fetch(req, server)` | ✗ | ✓ | Main request handler |
| option `error(err)` | ✗ | ✓ |  |
| option `routes` | ✗ | ✓ | Bun ≥ 1.2.3 path-based router |
| option `websocket` | ✗ | ✓ | WebSocket handler config |
| option `tls` | ✗ | ✓ | `{ key, cert, ca, passphrase, dhParamsFile, secureOptions, serverName, lowMemoryMode }` |
| option `idleTimeout` | ✗ | ✓ | Seconds (default 10, max 255) |
| option `maxRequestBodySize` | ✗ | ✓ |  |
| option `development` | ✗ | ✓ |  |
| option `reusePort` | ✗ | ✓ |  |
| option `ipv6Only` | ✗ | ✓ |  |
| option `id` | ✗ | ✓ |  |
| option `static` | ✗ | ✓ | Map of path → Response |
| `server.stop(closeActive?)` | ✗ | ✓ |  |
| `server.reload(options)` | ✗ | ✓ | Hot route reload |
| `server.fetch(req)` | ✗ | ✓ | Internal request routing |
| `server.upgrade(req, options?)` | ✗ | ✓ | WebSocket upgrade |
| `server.publish(topic, data, compress?)` | ✗ | ✓ | Pub-sub broadcast |
| `server.subscriberCount(topic)` | ✗ | ✓ |  |
| `server.requestIP(req)` | ✗ | ✓ | `{ address, port }` or null |
| `server.timeout(req, seconds)` | ✗ | ✓ | Per-request idle timeout |
| `server.ref()` | ✗ | ✓ |  |
| `server.unref()` | ✗ | ✓ |  |
| `server.pendingRequests` | ✗ | ✓ |  |
| `server.pendingWebSockets` | ✗ | ✓ |  |
| `server.url` | ✗ | ✓ |  |
| `server.port` | ✗ | ✓ |  |
| `server.hostname` | ✗ | ✓ |  |
| `server.development` | ✗ | ✓ |  |
| `server.id` | ✗ | ✓ |  |
| `ServerWebSocket.send(data, compress?)` | ✗ | ✓ |  |
| `ServerWebSocket.sendBinary(data, compress?)` | ✗ | ✓ |  |
| `ServerWebSocket.sendText(data, compress?)` | ✗ | ✓ |  |
| `ServerWebSocket.subscribe(topic)` | ✗ | ✓ |  |
| `ServerWebSocket.unsubscribe(topic)` | ✗ | ✓ |  |
| `ServerWebSocket.isSubscribed(topic)` | ✗ | ✓ |  |
| `ServerWebSocket.publish(topic, data, compress?)` | ✗ | ✓ |  |
| `ServerWebSocket.close(code?, reason?)` | ✗ | ✓ |  |
| `ServerWebSocket.terminate()` | ✗ | ✓ |  |
| `ServerWebSocket.ping(data?)` | ✗ | ✓ |  |
| `ServerWebSocket.pong(data?)` | ✗ | ✓ |  |
| `ServerWebSocket.cork(cb)` | ✗ | ✓ |  |
| `ServerWebSocket.data` | ✗ | ✓ | Custom per-connection data |
| `ServerWebSocket.readyState` | ✗ | ✓ |  |
| `ServerWebSocket.remoteAddress` | ✗ | ✓ |  |
| `ServerWebSocket.binaryType` | ✗ | ✓ |  |
| websocket handler `open / message / close / drain / ping / pong` | ✗ | ✓ |  |

#### `Bun.file` / `Bun.write` (file I/O)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.file(path, options?)` | ✗ | ✓ | Returns `BunFile`. Accepts string / fd / URL |
| `Bun.write(dest, input)` | ✗ | ✓ | Multi-tool writer using fastest syscalls per platform |
| `BunFile.size` | ✗ | ✓ |  |
| `BunFile.type` | ✗ | ✓ | MIME |
| `BunFile.name` | ✗ | ✓ |  |
| `BunFile.lastModified` | ✗ | ✓ |  |
| `BunFile.text()` | ✗ | ✓ |  |
| `BunFile.json()` | ✗ | ✓ |  |
| `BunFile.arrayBuffer()` | ✗ | ✓ |  |
| `BunFile.bytes()` | ✗ | ✓ |  |
| `BunFile.blob()` | ✗ | ✓ |  |
| `BunFile.formData()` | ✗ | ✓ |  |
| `BunFile.stream()` | ✗ | ✓ |  |
| `BunFile.slice(start?, end?, type?)` | ✗ | ✓ |  |
| `BunFile.exists()` | ✗ | ✓ |  |
| `BunFile.delete()` | ✗ | ✓ |  |
| `BunFile.unlink()` | ✗ | ✓ | Alias |
| `BunFile.writer(options?)` | ✗ | ✓ | Returns `FileSink` |
| `FileSink.write(chunk)` | ✗ | ✓ |  |
| `FileSink.flush()` | ✗ | ✓ |  |
| `FileSink.end(error?)` | ✗ | ✓ |  |
| `FileSink.start(options?)` | ✗ | ✓ |  |
| `FileSink.ref()` | ✗ | ✓ |  |
| `FileSink.unref()` | ✗ | ✓ |  |

#### `Bun.spawn` / `Bun.spawnSync`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.spawn(cmd, options?)` | ✗ | ✓ | Node analog: `child_process.spawn` |
| `Bun.spawnSync(cmd, options?)` | ✗ | ✓ | Node analog: `child_process.spawnSync` |
| option `cmd` | ✗ | ✓ |  |
| option `cwd` | ✗ | ✓ |  |
| option `env` | ✗ | ✓ |  |
| option `stdin` | ✗ | ✓ | `"pipe"`/`"inherit"`/`"ignore"`/Bun.file/TypedArray/Response/Request/ReadableStream/Blob/fd |
| option `stdout` | ✗ | ✓ |  |
| option `stderr` | ✗ | ✓ |  |
| option `stdio` | ✗ | ✓ |  |
| option `onExit(proc, code, signal, err)` | ✗ | ✓ |  |
| option `ipc(message, subprocess)` | ✗ | ✓ |  |
| option `serialization` | ✗ | ✓ | `"json"` / `"advanced"` |
| option `windowsHide` | ✗ | ✓ |  |
| option `windowsVerbatimArguments` | ✗ | ✓ |  |
| option `argv0` | ✗ | ✓ |  |
| option `signal` (AbortSignal) | ✗ | ✓ |  |
| option `timeout` | ✗ | ✓ | ms |
| option `killSignal` | ✗ | ✓ |  |
| option `maxBuffer` | ✗ | ✓ | spawnSync only |
| option `terminal` | ✗ | ✓ | PTY (POSIX) / ConPTY (Windows) — Bun-specific |
| `Subprocess.pid` | ✗ | ✓ |  |
| `Subprocess.exited` | ✗ | ✓ | Promise |
| `Subprocess.exitCode` | ✗ | ✓ |  |
| `Subprocess.signalCode` | ✗ | ✓ |  |
| `Subprocess.killed` | ✗ | ✓ |  |
| `Subprocess.stdin` | ✗ | ✓ | FileSink \| fd \| null |
| `Subprocess.stdout` | ✗ | ✓ | ReadableStream \| fd \| null |
| `Subprocess.stderr` | ✗ | ✓ |  |
| `Subprocess.readable` | ✗ | ✓ |  |
| `Subprocess.terminal` | ✗ | ✓ |  |
| `Subprocess.kill(signal?)` | ✗ | ✓ |  |
| `Subprocess.ref()` | ✗ | ✓ |  |
| `Subprocess.unref()` | ✗ | ✓ |  |
| `Subprocess.send(msg)` | ✗ | ✓ | IPC |
| `Subprocess.disconnect()` | ✗ | ✓ |  |
| `Subprocess.resourceUsage()` | ✗ | ✓ | `{ cpuTime, maxRSS, ...}` |
| `class Bun.Terminal` | ✗ | ✓ | Reusable PTY |
| `Terminal.write(data)` | ✗ | ✓ |  |
| `Terminal.resize(cols, rows)` | ✗ | ✓ |  |
| `Terminal.setRawMode(bool)` | ✗ | ✓ |  |
| `Terminal.close()` | ✗ | ✓ |  |
| `Terminal.ref()` / `.unref()` | ✗ | ✓ |  |

#### `Bun.listen` / `Bun.connect` (TCP)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.listen(options)` | ✗ | ✓ | Node analog: `net.createServer` |
| `Bun.connect(options)` | ✗ | ✓ | Node analog: `net.connect` |
| option `hostname` | ✗ | ✓ |  |
| option `port` | ✗ | ✓ |  |
| option `unix` | ✗ | ✓ |  |
| option `tls` | ✗ | ✓ |  |
| socket handler `open / data / close / drain / error / connectError / end / timeout / handshake` | ✗ | ✓ |  |
| `Socket.write(data)` / `.end(data?)` / `.shutdown()` / `.terminate()` / `.flush()` / `.ref()` / `.unref()` | ✗ | ✓ |  |
| `Socket.timeout(seconds)` | ✗ | ✓ |  |
| `Socket.localPort` / `.remoteAddress` / `.readyState` | ✗ | ✓ |  |
| `TCPSocketServer.stop(closeActive?)` / `.ref()` / `.unref()` / `.reload(handler)` | ✗ | ✓ |  |

#### `Bun.udpSocket`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.udpSocket(options)` | ✗ | ✓ | Node analog: `dgram.createSocket` |
| option `port` / `hostname` / `socket` | ✗ | ✓ |  |
| `UDPSocket.send(data, port, address)` | ✗ | ✓ |  |
| `UDPSocket.sendMany(packets)` | ✗ | ✓ |  |
| `UDPSocket.close()` | ✗ | ✓ |  |
| `UDPSocket.ref()` / `.unref()` | ✗ | ✓ |  |
| `UDPSocket.address` / `.port` / `.hostname` / `.closed` | ✗ | ✓ |  |

#### `Bun.password`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.password.hash(password, options?)` | ✗ | ✓ | Async; auto-salts |
| `Bun.password.hashSync(password, options?)` | ✗ | ✓ | Blocking |
| `Bun.password.verify(password, hash)` | ✗ | ✓ | Async; auto-detects algorithm |
| `Bun.password.verifySync(password, hash)` | ✗ | ✓ | Blocking |
| algorithm `"argon2id"` (default) | ✗ | ✓ | `{ memoryCost, timeCost }` |
| algorithm `"argon2i"` | ✗ | ✓ |  |
| algorithm `"argon2d"` | ✗ | ✓ |  |
| algorithm `"bcrypt"` | ✗ | ✓ | `{ cost: 4..31 }`; passwords >72 bytes auto-SHA-512'd |

#### `Bun.hash` (non-cryptographic)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.hash(data, seed?)` | ✗ | ✓ | Alias for `wyhash` |
| `Bun.hash.wyhash(data, seed?)` | ✗ | ✓ | 64-bit |
| `Bun.hash.adler32(data, seed?)` | ✗ | ✓ | 32-bit |
| `Bun.hash.crc32(data, seed?)` | ✗ | ✓ | 32-bit |
| `Bun.hash.cityHash32(data, seed?)` | ✗ | ✓ | 32-bit |
| `Bun.hash.cityHash64(data, seed?)` | ✗ | ✓ | 64-bit |
| `Bun.hash.xxHash32(data, seed?)` | ✗ | ✓ |  |
| `Bun.hash.xxHash64(data, seed?)` | ✗ | ✓ |  |
| `Bun.hash.xxHash3(data, seed?)` | ✗ | ✓ |  |
| `Bun.hash.murmur32v3(data, seed?)` | ✗ | ✓ |  |
| `Bun.hash.murmur32v2(data, seed?)` | ✗ | ✓ |  |
| `Bun.hash.murmur64v2(data, seed?)` | ✗ | ✓ |  |
| `Bun.hash.rapidhash(data, seed?)` | ✗ | ✓ |  |

#### `Bun.CryptoHasher`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `new Bun.CryptoHasher(algorithm, key?)` | ✗ | ✓ | Key arg = HMAC mode. Node analog: `crypto.createHash` / `crypto.createHmac` |
| `hasher.update(data, encoding?)` | ✗ | ✓ |  |
| `hasher.digest(encoding?\|TypedArray?)` | ✗ | ✓ | encoding: `"hex"`/`"base64"`/`"base64url"`/`"binary"`; default Uint8Array |
| `hasher.copy()` | ✗ | ✓ |  |
| `hasher.algorithm` | ✗ | ✓ |  |
| `hasher.byteLength` | ✗ | ✓ |  |
| algorithm `"blake2b256"` | ✗ | ✓ |  |
| algorithm `"blake2b512"` | ✗ | ✓ |  |
| algorithm `"md4"` | ✗ | ✓ |  |
| algorithm `"md5"` | ✗ | ✓ |  |
| algorithm `"ripemd160"` | ✗ | ✓ |  |
| algorithm `"sha1"` | ✗ | ✓ |  |
| algorithm `"sha224"` | ✗ | ✓ |  |
| algorithm `"sha256"` | ✗ | ✓ |  |
| algorithm `"sha384"` | ✗ | ✓ |  |
| algorithm `"sha512"` | ✗ | ✓ |  |
| algorithm `"sha512-224"` | ✗ | ✓ |  |
| algorithm `"sha512-256"` | ✗ | ✓ |  |
| algorithm `"sha3-224"` | ✗ | ✓ |  |
| algorithm `"sha3-256"` | ✗ | ✓ |  |
| algorithm `"sha3-384"` | ✗ | ✓ |  |
| algorithm `"sha3-512"` | ✗ | ✓ |  |
| algorithm `"shake128"` | ✗ | ✓ |  |
| algorithm `"shake256"` | ✗ | ✓ |  |

#### `Bun.sha` / fast-path hashers

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.SHA1` / `.hash(input, encoding?)` | ✗ | ✓ | Static class |
| `Bun.SHA256` / `.hash(input, encoding?)` | ✗ | ✓ |  |
| `Bun.SHA384` / `.hash(input, encoding?)` | ✗ | ✓ |  |
| `Bun.SHA512` / `.hash(input, encoding?)` | ✗ | ✓ |  |
| `Bun.SHA512_256` / `.hash(input, encoding?)` | ✗ | ✓ |  |
| `Bun.MD4` / `.hash(input, encoding?)` | ✗ | ✓ |  |
| `Bun.MD5` / `.hash(input, encoding?)` | ✗ | ✓ |  |

#### Compression utilities

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.gzipSync(buf, options?)` | ✗ | ✓ | Node analog: `zlib.gzipSync` |
| `Bun.gunzipSync(buf)` | ✗ | ✓ |  |
| `Bun.deflateSync(buf, options?)` | ✗ | ✓ |  |
| `Bun.inflateSync(buf)` | ✗ | ✓ |  |
| `Bun.zstdCompress(buf, options?)` | ✗ | ✓ | Async. Node 23+ has `zlib.createZstdCompress` |
| `Bun.zstdCompressSync(buf, options?)` | ✗ | ✓ |  |
| `Bun.zstdDecompress(buf)` | ✗ | ✓ |  |
| `Bun.zstdDecompressSync(buf)` | ✗ | ✓ |  |

#### Stream consumption helpers

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.readableStreamToArrayBuffer(rs)` | ✗ | ✓ |  |
| `Bun.readableStreamToBytes(rs)` | ✗ | ✓ | → `Uint8Array` |
| `Bun.readableStreamToBlob(rs)` | ✗ | ✓ |  |
| `Bun.readableStreamToJSON(rs)` | ✗ | ✓ |  |
| `Bun.readableStreamToText(rs)` | ✗ | ✓ |  |
| `Bun.readableStreamToArray(rs)` | ✗ | ✓ | All chunks as array |
| `Bun.readableStreamToFormData(rs, boundary?)` | ✗ | ✓ |  |

#### `Bun.dns`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.dns.lookup(hostname, options?)` | ✗ | ✓ | Node analog: `dns.lookup` |
| `Bun.dns.prefetch(hostname, port)` | ✗ | ✓ | Warm Bun's DNS cache |
| `Bun.dns.getCacheStats()` | ✗ | ✓ | `{ cacheHitsCompleted, cacheHitsInflight, cacheMisses, size, errors, totalCount }` |

#### `Bun.semver`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.semver.satisfies(version, range)` | ✗ | ✓ | Node analog: `node-semver` npm pkg |
| `Bun.semver.order(a, b)` | ✗ | ✓ | Returns 0 / 1 / -1 |

#### `Bun.Glob`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `new Bun.Glob(pattern)` | ✗ | ✓ | Node 22+ has `fs.glob` (different shape) |
| `glob.scan(rootOrOptions)` | ✗ | ✓ | Async iterable |
| `glob.scanSync(rootOrOptions)` | ✗ | ✓ |  |
| `glob.match(path)` | ✗ | ✓ |  |
| scan option `cwd` | ✗ | ✓ |  |
| scan option `dot` | ✗ | ✓ |  |
| scan option `absolute` | ✗ | ✓ |  |
| scan option `followSymlinks` | ✗ | ✓ |  |
| scan option `throwErrorOnBrokenSymlink` | ✗ | ✓ |  |
| scan option `onlyFiles` | ✗ | ✓ |  |

#### `Bun.color`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.color(input, outputFormat?)` | ✗ | ✓ | Alternative to `color`/`tinycolor2` npm packages |
| format `"css"` | ✗ | ✓ |  |
| format `"ansi"` (auto-detect terminal) | ✗ | ✓ |  |
| format `"ansi-16"` / `"ansi-256"` / `"ansi-16m"` | ✗ | ✓ |  |
| format `"number"` | ✗ | ✓ |  |
| format `"rgb"` / `"rgba"` / `"hsl"` | ✗ | ✓ |  |
| format `"hex"` / `"HEX"` | ✗ | ✓ |  |
| format `"{rgb}"` / `"{rgba}"` / `"[rgb]"` / `"[rgba]"` | ✗ | ✓ |  |

#### `Bun.s3` / `Bun.S3Client`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.s3` (singleton) | ✗ | ✓ | env-driven default. Node analog: `@aws-sdk/client-s3` |
| `class Bun.S3Client` | ✗ | ✓ |  |
| `new S3Client({accessKeyId, secretAccessKey, region?, endpoint?, bucket?, sessionToken?, virtualHostedStyle?, acl?})` | ✗ | ✓ |  |
| `client.file(path, options?)` | ✗ | ✓ | Returns lazy `S3File` |
| `client.write(path, data, options?)` | ✗ | ✓ |  |
| `client.delete(path, options?)` | ✗ | ✓ |  |
| `client.unlink(path, options?)` | ✗ | ✓ | Alias |
| `client.exists(path, options?)` | ✗ | ✓ |  |
| `client.size(path, options?)` | ✗ | ✓ |  |
| `client.stat(path, options?)` | ✗ | ✓ | → `{etag, lastModified, size, type}` |
| `client.list(options?, credentials?)` | ✗ | ✓ |  |
| `client.presign(path, options?)` | ✗ | ✓ | Synchronous URL signing |
| static `S3Client.write` / `.delete` / `.unlink` / `.exists` / `.size` / `.stat` / `.list` / `.presign` | ✗ | ✓ | Same shape, take credentials inline |
| `class S3File extends Blob` | ✗ | ✓ |  |
| `s3file.text() / json() / bytes() / arrayBuffer() / stream() / slice(start, end?)` | ✗ | ✓ |  |
| `s3file.write(data, options?)` | ✗ | ✓ |  |
| `s3file.writer(options?)` | ✗ | ✓ | Streaming multipart upload |
| `s3file.exists() / unlink() / delete() / presign(options?) / stat()` | ✗ | ✓ |  |
| `s3://` protocol in `fetch()` / `Bun.file()` | ✗ | ✓ |  |
| error codes `ERR_S3_MISSING_CREDENTIALS` / `_INVALID_METHOD` / `_INVALID_PATH` / `_INVALID_ENDPOINT` / `_INVALID_SIGNATURE` / `_INVALID_SESSION_TOKEN` | ✗ | ✓ |  |

#### `Bun.SQL` / `Bun.sql`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.sql` (singleton, env-configured) | ✗ | ✓ | Node analog: `pg` / `mysql2` / `better-sqlite3` |
| `new Bun.SQL(connectionString, options?)` | ✗ | ✓ |  |
| tagged template `` sql`SELECT ...` `` | ✗ | ✓ |  |
| `sql.begin(cb)` / `sql.beginDistributed(name, cb)` / `sql.commit()` / `sql.rollback()` | ✗ | ✓ | Transactions |
| `sql.savepoint(name, cb)` | ✗ | ✓ |  |
| `sql.close({timeout?})` / `sql.end()` | ✗ | ✓ |  |
| `sql.connect()` / `sql.flush()` | ✗ | ✓ |  |
| `sql.options` | ✗ | ✓ |  |
| `sql.unsafe(query, params?)` | ✗ | ✓ |  |
| `sql.file(path, params?)` | ✗ | ✓ |  |
| `sql.json(value)` / `sql.array(value, type?)` / `sql.types.*` | ✗ | ✓ |  |
| `sql.reserve()` | ✗ | ✓ | Reserved connection from pool |
| result `.execute()` / `.cancel()` / `.simple()` / `.raw()` / `.values()` | ✗ | ✓ |  |
| protocol `postgres://` (default) | ✗ | ✓ |  |
| protocol `mysql://` | ✗ | ✓ |  |
| protocol `sqlite://` | ✗ | ✓ |  |

#### `Bun.RedisClient` / `Bun.redis`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.redis` (singleton) | ✗ | ✓ | Node analog: `ioredis` / `redis` npm |
| `new Bun.RedisClient(url?, options?)` | ✗ | ✓ |  |
| `client.connect()` / `.close()` | ✗ | ✓ |  |
| `client.send(cmd, args)` | ✗ | ✓ |  |
| client convenience methods: `.get`/`.set`/`.del`/`.exists`/`.incr`/`.decr`/`.expire`/`.ttl`/`.keys`/`.hget`/`.hset`/`.hgetall`/etc. | ✗ | ✓ |  |
| `client.subscribe(channel, handler)` / `.unsubscribe(channel)` / `.publish(channel, msg)` | ✗ | ✓ |  |

#### `Bun.Cookie` / `Bun.CookieMap`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `class Bun.Cookie` | ✗ | ✓ |  |
| `new Bun.Cookie(name, value, options?)` | ✗ | ✓ |  |
| `new Bun.Cookie(cookieString)` | ✗ | ✓ |  |
| `new Bun.Cookie(options)` | ✗ | ✓ |  |
| `cookie.name / value / domain / path / expires / secure / sameSite / partitioned / maxAge / httpOnly` | ✗ | ✓ |  |
| `cookie.isExpired()` | ✗ | ✓ |  |
| `cookie.serialize()` | ✗ | ✓ |  |
| `cookie.toString()` | ✗ | ✓ |  |
| `cookie.toJSON()` | ✗ | ✓ |  |
| static `Cookie.parse(str)` | ✗ | ✓ |  |
| static `Cookie.from(name, value, options?)` | ✗ | ✓ |  |
| `class Bun.CookieMap` | ✗ | ✓ |  |
| `new Bun.CookieMap(init?)` | ✗ | ✓ | string / record / `[name,value][]` |
| `map.get(name)` / `.has(name)` / `.set(name, value, options?)` / `.delete(name, options?)` | ✗ | ✓ |  |
| `map.toJSON()` | ✗ | ✓ |  |
| `map.toSetCookieHeaders()` | ✗ | ✓ | array of `Set-Cookie` strings |
| `map.entries() / keys() / values() / forEach()` | ✗ | ✓ |  |
| `map[Symbol.iterator]()` | ✗ | ✓ |  |
| `map.size` | ✗ | ✓ |  |

#### `Bun.CSRF`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.CSRF.generate(secret, options?)` | ✗ | ✓ |  |
| `Bun.CSRF.verify(token, secret, options?)` | ✗ | ✓ |  |

#### Shell (`$`)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `import { $ } from "bun"` tagged template | ✗ | ✓ | Bash-like; cross-platform; not Node |
| `$.cwd(path)` / `await $.cwd()` | ✗ | ✓ |  |
| `$.env(envObject\|undefined)` | ✗ | ✓ |  |
| `$.nothrow()` | ✗ | ✓ |  |
| `$.throws(bool)` | ✗ | ✓ |  |
| `$.braces(template)` | ✗ | ✓ | brace expansion |
| `$.escape(str)` | ✗ | ✓ |  |
| `ShellOutput.text(encoding?)` | ✗ | ✓ |  |
| `ShellOutput.json()` | ✗ | ✓ |  |
| `ShellOutput.blob()` | ✗ | ✓ |  |
| `ShellOutput.arrayBuffer()` | ✗ | ✓ |  |
| `ShellOutput.bytes()` | ✗ | ✓ |  |
| `ShellOutput.lines()` | ✗ | ✓ | async iterable |
| `ShellOutput.exitCode / stdout / stderr` | ✗ | ✓ |  |
| `.quiet()` / `.nothrow()` / `.cwd(p)` / `.env(o)` on command | ✗ | ✓ |  |
| builtins: cd, ls, rm, echo, pwd, bun, cat, touch, mkdir, which, mv, exit, true, false, yes, seq, dirname, basename | ✗ | ✓ |  |
| `.sh` file loader | ✗ | ✓ |  |

#### `Bun.WebView`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `new Bun.WebView(options?)` | ✗ | ✓ | Headless/headful browser. No Node analog |
| `wv.navigate(url) / .setHTML(html) / .eval(js) / .bind(name, fn) / .destroy() / .show()` | ✗ | ✓ |  |

#### `Bun.TOML` / `Bun.markdown` / `Bun.Image`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `Bun.TOML.parse(text)` | ✗ | ✓ |  |
| `Bun.TOML.stringify(value)` | ✗ | ✓ |  |
| `Bun.markdown(text, options?)` | ✗ | ✓ | Renders Markdown to HTML |
| `Bun.Image(input)` | ✗ | ✓ | Image decoding |

#### `Bun.ArrayBufferSink`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `new Bun.ArrayBufferSink()` | ✗ | ✓ |  |
| `sink.start(options?)` | ✗ | ✓ | `{ highWaterMark, asUint8Array, stream }` |
| `sink.write(chunk)` | ✗ | ✓ |  |
| `sink.flush()` | ✗ | ✓ |  |
| `sink.end()` | ✗ | ✓ | Returns final ArrayBuffer / Uint8Array |
| `sink.ref()` / `.unref()` | ✗ | ✓ |  |

---

#### `bun:test`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `describe(name, fn)` | ✗ | ✓ | Node analog: `node:test` (different shape) |
| `describe.each(table)(name, fn)` | ✗ | ✓ |  |
| `describe.skip(name, fn)` | ✗ | ✓ |  |
| `describe.skipIf(cond)(name, fn)` | ✗ | ✓ |  |
| `describe.only(name, fn)` | ✗ | ✓ |  |
| `describe.todo(name, fn?)` | ✗ | ✓ |  |
| `describe.if(cond)(name, fn)` | ✗ | ✓ |  |
| `test(name, fn, timeoutOrOptions?)` | ✗ | ✓ |  |
| `it(name, fn, ...)` | ✗ | ✓ | alias |
| `test.each(table)(name, fn)` | ✗ | ✓ |  |
| `test.skip(name, fn?)` | ✗ | ✓ |  |
| `test.skipIf(cond)(name, fn)` | ✗ | ✓ |  |
| `test.only(name, fn)` | ✗ | ✓ |  |
| `test.todo(name, fn?)` | ✗ | ✓ |  |
| `test.if(cond)(name, fn)` | ✗ | ✓ |  |
| `test.failing(name, fn)` | ✗ | ✓ |  |
| `test.concurrent(name, fn)` | ✗ | ✓ |  |
| `test.serial(name, fn)` | ✗ | ✓ |  |
| `beforeAll(fn)` | ✗ | ✓ |  |
| `beforeEach(fn)` | ✗ | ✓ |  |
| `afterAll(fn)` | ✗ | ✓ |  |
| `afterEach(fn)` | ✗ | ✓ |  |
| `setDefaultTimeout(ms)` | ✗ | ✓ |  |
| `mock(fn)` | ✗ | ✓ |  |
| `mock.module(specifier, factory)` | ✗ | ✓ |  |
| `mock.restore()` | ✗ | ✓ |  |
| `spyOn(obj, method)` | ✗ | ✓ |  |
| `jest.fn(fn?)` | ✗ | ✓ | alias |
| `jest.spyOn(obj, m)` | ✗ | ✓ |  |
| `jest.useFakeTimers()` / `useRealTimers()` | ✗ | ✓ |  |
| `jest.setSystemTime(time)` | ✗ | ✓ |  |
| `jest.advanceTimersByTime(ms)` | ✗ | ✓ |  |
| `jest.runAllTimers()` / `runOnlyPendingTimers()` | ✗ | ✓ |  |
| `jest.clearAllMocks()` / `resetAllMocks()` / `restoreAllMocks()` | ✗ | ✓ |  |
| `jest.mock(moduleName, factory?)` | ✗ | ✓ |  |
| `jest.unmock(moduleName)` | ✗ | ✓ |  |
| `expect(value)` | ✗ | ✓ |  |
| `expect.anything()` | ✗ | ✓ |  |
| `expect.any(constructor)` | ✗ | ✓ |  |
| `expect.arrayContaining(array)` | ✗ | ✓ |  |
| `expect.objectContaining(obj)` | ✗ | ✓ |  |
| `expect.stringContaining(str)` | ✗ | ✓ |  |
| `expect.stringMatching(regex)` | ✗ | ✓ |  |
| `expect.closeTo(num, digits?)` | ✗ | ✓ |  |
| `expect.assertions(n)` | ✗ | ✓ |  |
| `expect.hasAssertions()` | ✗ | ✓ |  |
| `expect.addSnapshotSerializer(serializer)` | ✗ | ✓ |  |
| `expect.extend(matchers)` | ✗ | ✓ |  |
| `expect.unreachable()` | ✗ | ✓ |  |
| matcher `.toBe(value)` | ✗ | ✓ |  |
| matcher `.toEqual(value)` | ✗ | ✓ |  |
| matcher `.toStrictEqual(value)` | ✗ | ✓ |  |
| matcher `.toBeCloseTo(num, digits?)` | ✗ | ✓ |  |
| matcher `.toBeDefined()` | ✗ | ✓ |  |
| matcher `.toBeUndefined()` | ✗ | ✓ |  |
| matcher `.toBeNull()` | ✗ | ✓ |  |
| matcher `.toBeFalsy()` | ✗ | ✓ |  |
| matcher `.toBeTruthy()` | ✗ | ✓ |  |
| matcher `.toBeNaN()` | ✗ | ✓ |  |
| matcher `.toBeFinite()` | ✗ | ✓ |  |
| matcher `.toBeOneOf(values)` | ✗ | ✓ |  |
| matcher `.toBeGreaterThan(n)` | ✗ | ✓ |  |
| matcher `.toBeGreaterThanOrEqual(n)` | ✗ | ✓ |  |
| matcher `.toBeLessThan(n)` | ✗ | ✓ |  |
| matcher `.toBeLessThanOrEqual(n)` | ✗ | ✓ |  |
| matcher `.toBeInstanceOf(Class)` | ✗ | ✓ |  |
| matcher `.toBeTypeOf("...")` | ✗ | ✓ |  |
| matcher `.toBeBoolean()` / `.toBeNumber()` / `.toBeString()` / `.toBeArray()` / `.toBeObject()` / `.toBeFunction()` / `.toBeSymbol()` / `.toBeDate()` | ✗ | ✓ | Jest-extended matchers |
| matcher `.toBeEmpty()` / `.toBeEmptyObject()` | ✗ | ✓ |  |
| matcher `.toBeNil()` / `.toBeOdd()` / `.toBeEven()` / `.toBePositive()` / `.toBeNegative()` | ✗ | ✓ |  |
| matcher `.toContain(value)` | ✗ | ✓ |  |
| matcher `.toContainEqual(value)` | ✗ | ✓ |  |
| matcher `.toContainKey(key)` / `.toContainKeys(keys)` / `.toContainValue(v)` / `.toContainValues(vs)` | ✗ | ✓ |  |
| matcher `.toContainAllKeys(keys)` / `.toContainAnyKeys(keys)` / `.toContainAllValues(vs)` / `.toContainAnyValues(vs)` | ✗ | ✓ |  |
| matcher `.toHaveLength(n)` | ✗ | ✓ |  |
| matcher `.toHaveProperty(path, value?)` | ✗ | ✓ |  |
| matcher `.toMatch(strOrRegex)` | ✗ | ✓ |  |
| matcher `.toMatchObject(obj)` | ✗ | ✓ |  |
| matcher `.toMatchSnapshot(hint?)` | ✗ | ✓ |  |
| matcher `.toMatchInlineSnapshot(snap?)` | ✗ | ✓ |  |
| matcher `.toThrow(error?)` | ✗ | ✓ |  |
| matcher `.toThrowError(error?)` | ✗ | ✓ |  |
| matcher `.toThrowErrorMatchingSnapshot()` | ✗ | ✓ |  |
| matcher `.toThrowErrorMatchingInlineSnapshot()` | ✗ | ✓ |  |
| matcher `.toHaveBeenCalled()` | ✗ | ✓ |  |
| matcher `.toHaveBeenCalledTimes(n)` | ✗ | ✓ |  |
| matcher `.toHaveBeenCalledWith(...args)` | ✗ | ✓ |  |
| matcher `.toHaveBeenLastCalledWith(...args)` | ✗ | ✓ |  |
| matcher `.toHaveBeenNthCalledWith(n, ...args)` | ✗ | ✓ |  |
| matcher `.toHaveReturned()` / `.toHaveReturnedTimes(n)` / `.toHaveReturnedWith(v)` / `.toHaveLastReturnedWith(v)` / `.toHaveNthReturnedWith(n, v)` | ✗ | ✓ |  |
| matcher `.toResolve()` | ✗ | ✓ |  |
| matcher `.toReject()` | ✗ | ✓ |  |
| matcher `.toEqualIgnoringWhitespace(str)` | ✗ | ✓ |  |
| matcher modifier `.not.<...>` | ✗ | ✓ |  |
| matcher modifier `.resolves.<...>` | ✗ | ✓ |  |
| matcher modifier `.rejects.<...>` | ✗ | ✓ |  |

#### `bun:sqlite`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `import { Database } from "bun:sqlite"` | ✗ | ✓ | Node analog: `better-sqlite3` npm / `node:sqlite` experimental |
| `new Database(filename?, options?)` | ✗ | ✓ | `{readonly, create, readwrite, safeIntegers, strict}` |
| `Database.deserialize(buf)` | ✗ | ✓ |  |
| `Database.setCustomSQLite(path)` | ✗ | ✓ | macOS only — point at non-system SQLite |
| `db.query(sql)` | ✗ | ✓ | Returns cached `Statement` |
| `db.prepare(sql)` | ✗ | ✓ | Non-cached `Statement` |
| `db.run(sql, params?)` | ✗ | ✓ | `{lastInsertRowid, changes}` |
| `db.exec(sql, params?)` | ✗ | ✓ | alias for run |
| `db.transaction(fn)` | ✗ | ✓ | Returns wrapper + `.deferred / .immediate / .exclusive` |
| `db.close(throwOnError?)` | ✗ | ✓ |  |
| `db.serialize(name?)` | ✗ | ✓ | → `Uint8Array` |
| `db.loadExtension(name, entryPoint?)` | ✗ | ✓ |  |
| `db.fileControl(cmd, value)` | ✗ | ✓ | sqlite3 `file_control` API |
| `db.filename` / `db.inTransaction` / `db.handle` | ✗ | ✓ |  |
| `constants.SQLITE_FCNTL_PERSIST_WAL` etc. | ✗ | ✓ | `import { constants } from "bun:sqlite"` |
| `Statement.all(...params)` | ✗ | ✓ |  |
| `Statement.get(...params)` | ✗ | ✓ |  |
| `Statement.run(...params)` | ✗ | ✓ | `{lastInsertRowid, changes}` |
| `Statement.values(...params)` | ✗ | ✓ |  |
| `Statement.iterate(...params)` | ✗ | ✓ | Sync iterator |
| `Statement[Symbol.iterator]()` | ✗ | ✓ |  |
| `Statement.as(Class)` | ✗ | ✓ | Maps rows to class instances (no ctor) |
| `Statement.finalize()` | ✗ | ✓ |  |
| `Statement.toString()` | ✗ | ✓ | Expanded SQL |
| `Statement.columnNames` | ✗ | ✓ |  |
| `Statement.columnTypes` | ✗ | ✓ |  |
| `Statement.declaredTypes` | ✗ | ✓ |  |
| `Statement.paramsCount` | ✗ | ✓ |  |
| `Statement.native` | ✗ | ✓ |  |
| `import db from "./x.sqlite" with { type: "sqlite" }` | ✗ | ✓ | Import attribute |

#### `bun:ffi`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `dlopen(path, symbols)` | ✗ | ✓ | Node analog: `node-api` (different shape) |
| `suffix` | ✗ | ✓ | `"dylib"` / `"so"` / `"dll"` |
| `class FFIType` | ✗ | ✓ |  |
| `FFIType.i8 / int8_t` | ✗ | ✓ |  |
| `FFIType.i16 / int16_t` | ✗ | ✓ |  |
| `FFIType.i32 / int32_t / int` | ✗ | ✓ |  |
| `FFIType.i64 / int64_t` | ✗ | ✓ |  |
| `FFIType.i64_fast` | ✗ | ✓ |  |
| `FFIType.u8 / uint8_t` | ✗ | ✓ |  |
| `FFIType.u16 / uint16_t` | ✗ | ✓ |  |
| `FFIType.u32 / uint32_t` | ✗ | ✓ |  |
| `FFIType.u64 / uint64_t` | ✗ | ✓ |  |
| `FFIType.u64_fast` | ✗ | ✓ |  |
| `FFIType.f32 / float` | ✗ | ✓ |  |
| `FFIType.f64 / double` | ✗ | ✓ |  |
| `FFIType.bool` | ✗ | ✓ |  |
| `FFIType.char` | ✗ | ✓ |  |
| `FFIType.ptr / pointer / void*` | ✗ | ✓ |  |
| `FFIType.cstring` | ✗ | ✓ | char* → JS string |
| `FFIType.buffer` | ✗ | ✓ | TypedArray/DataView arg |
| `FFIType.function / fn / callback` | ✗ | ✓ |  |
| `FFIType.napi_env` | ✗ | ✓ |  |
| `FFIType.napi_value` | ✗ | ✓ |  |
| `class CString extends String` | ✗ | ✓ |  |
| `new CString(ptr, byteOffset?, byteLength?)` | ✗ | ✓ |  |
| `cstring.ptr / .byteOffset / .byteLength` | ✗ | ✓ |  |
| `ptr(typedArray, byteOffset?)` | ✗ | ✓ | TypedArray → pointer number |
| `toArrayBuffer(ptr, byteOffset?, byteLength?, deallocator?, deallocatorCtx?)` | ✗ | ✓ |  |
| `toBuffer(ptr, byteOffset?, byteLength?, deallocator?, deallocatorCtx?)` | ✗ | ✓ |  |
| `class JSCallback` | ✗ | ✓ |  |
| `new JSCallback(fn, { returns, args, threadsafe? })` | ✗ | ✓ |  |
| `jsCallback.ptr` | ✗ | ✓ |  |
| `jsCallback.close()` | ✗ | ✓ |  |
| `class CFunction` | ✗ | ✓ |  |
| `new CFunction({ returns, args, ptr })` | ✗ | ✓ |  |
| `linkSymbols(symbols)` | ✗ | ✓ |  |
| `read.i8/i16/i32/i64/u8/u16/u32/u64/f32/f64/ptr(ptr, byteOffset)` | ✗ | ✓ |  |
| `viewSource(symbols, asString?)` | ✗ | ✓ | Generated wrapper source |
| `cc(options)` | ✗ | ✓ | Compile + dlopen C source via TinyCC |

#### `bun:jsc`

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `serialize(value, options?)` | ✗ | ✓ | Same as `structuredClone` algorithm; → ArrayBuffer |
| `deserialize(buf)` | ✗ | ✓ |  |
| `callerSourceOrigin()` | ✗ | ✓ |  |
| `jscDescribe(value)` | ✗ | ✓ |  |
| `jscDescribeArray(args)` | ✗ | ✓ |  |
| `isRope(string)` | ✗ | ✓ |  |
| `memoryUsage()` | ✗ | ✓ | `{ current, peak, currentCommit, peakCommit, pageFaults }` |
| `heapSize()` | ✗ | ✓ |  |
| `heapStats()` | ✗ | ✓ | `{ heapSize, heapCapacity, objectCount, ... }` |
| `estimateShallowMemoryUsageOf(value)` | ✗ | ✓ |  |
| `fullGC()` | ✗ | ✓ |  |
| `edenGC()` | ✗ | ✓ |  |
| `gcAndSweep()` | ✗ | ✓ |  |
| `releaseWeakRefs()` | ✗ | ✓ |  |
| `getProtectedObjects()` | ✗ | ✓ |  |
| `profile(cb, sampleInterval?, ...args)` | ✗ | ✓ |  |
| `startSamplingProfiler(dir?)` | ✗ | ✓ |  |
| `optimizeNextInvocation(fn)` | ✗ | ✓ |  |
| `numberOfDFGCompiles(fn)` | ✗ | ✓ |  |
| `totalCompileTime(fn)` | ✗ | ✓ |  |
| `reoptimizationRetryCount(fn)` | ✗ | ✓ |  |
| `noFTL(fn)` | ✗ | ✓ |  |
| `noOSRExitFuzzing(fn)` | ✗ | ✓ |  |
| `getRandomSeed()` | ✗ | ✓ |  |
| `setRandomSeed(value)` | ✗ | ✓ |  |
| `setTimeZone(tz)` | ✗ | ✓ |  |
| `drainMicrotasks()` | ✗ | ✓ |  |
| `startRemoteDebugger(host?, port?)` | ✗ | ✓ |  |
| `generateHeapSnapshotForGCDebugging()` | ✗ | ✓ |  |

#### `import.meta` (Bun-specific extensions)

| API | Node.js | Bun | Notes |
|-----|---------|-----|-------|
| `import.meta.url` | ✓ | ✓ |  |
| `import.meta.resolve(spec)` | ✓ | ✓ |  |
| `import.meta.dirname` | ✓ | ✓ | Node 20.6+ |
| `import.meta.filename` | ✓ | ✓ | Node 20.6+ |
| `import.meta.dir` | ✗ | ✓ | Bun extension — same as dirname |
| `import.meta.file` | ✗ | ✓ | Bun extension — basename only |
| `import.meta.path` | ✗ | ✓ | Bun extension — absolute path |
| `import.meta.main` | ✗ | ✓ | Bun extension — true if entrypoint |
| `import.meta.env` | ✗ | ✓ | Bun extension; mirrors `process.env` |
| `import.meta.require(spec)` | ✗ | ✓ | Bun extension; CJS-style synchronous require from ESM |
| `import.meta.hot` | ✗ | ✓ | Bun extension — HMR API (`accept`, `dispose`, `data`) |
| `import.meta.glob(pattern, options?)` | ✗ | ✓ | Bun extension — Vite-compatible glob import |
| `import.meta.glob.eager(pattern, options?)` | ✗ | ✓ |  |

---

### Notes on shape/return-value divergences

- **`setTimeout` / `setInterval`**: Node returns a `Timeout` object with `.ref()`/`.unref()`/`.refresh()`/`.hasRef()`; Bun returns a `number` (web-spec) but the value supports `.unref()`/`.ref()` when coerced.
- **`setImmediate`**: Both runtimes ship this Node-extension global; web spec has no equivalent.
- **`Bun.file`** stdin/stdout: Bun re-uses the `BunFile` shape; Node exposes `process.stdin` as a `Readable` stream and `process.stdout`/`stderr` as `Writable` streams.
- **WebCrypto SubtleCrypto**: Algorithm coverage diverges most. Node 24 added a large block of post-quantum + KMAC algorithms and `SubtleCrypto.supports()`; Bun has not reached parity on those yet. Mark `⚠` cells as expectation-dependent.
- **Bun `WebSocket` (client)**: Adds non-spec `.ping()`, `.pong()`, `.terminate()` for compat with the popular `ws` npm package.
- **Workers**: Bun adds `"open"`, `"close"` events plus a `smol` option and `preload` array; Node exposes the equivalents under `node:worker_threads` (`threadId`, `resourceLimits`, `transferList`, `MessageChannel`).
- **Bun.spawn `terminal` option (PTY)**: No Node equivalent in core; needs `node-pty` npm.
- **`bun:test`** vs `node:test`: Same goals, divergent surface. Bun is Jest-compatible; Node test is its own AVA-like API. Snapshot/inline-snapshot/Jest-extended matchers are Bun-only.
- **`bun:sqlite`** vs `node:sqlite` (experimental): API shapes differ. Bun's is closer to `better-sqlite3`.
- **`Bun.SQL`** unified driver for Postgres/MySQL/SQLite vs Node's per-database npm packages.
- **`Bun.s3`**: synchronous presigning is unique; Node ecosystem uses `@aws-sdk/client-s3` with async signing.
