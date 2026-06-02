# DeepWiki Response

Repository: `nodejs/node`
Question: After fs parity work lands, which Node fs behaviors remain platform-specific, non-deterministic, unsupported, or part of broader non-fs stdlib validation work?
Search: https://deepwiki.com/search/after-fs-parity-work-lands-whi_7cdf8fdf-e13a-42c0-a009-4cd1a24e9e59

**Question**: After fs parity work lands, which Node fs behaviors remain platform-specific, non-deterministic, unsupported, or part of broader non-fs stdlib validation work?

Even after the `fs` parity work, several Node.js `fs` behaviors remain platform-specific, non-deterministic, or unsupported, and some aspects are part of broader standard library validation.

### Platform-Specific Behaviors
*   **File Permissions (`fs.chmod`, `fs.chown`)**: On Windows, only write permissions can be changed, and the distinction between group, owner, or others' permissions is not implemented. [1](#ref-q1-1)  Similarly, `fs.chown()` and `fsPromises.chown()` are also affected by platform differences. [2](#ref-q1-2)  [3](#ref-q1-3)
*   **Temporary Directory Creation (`fsPromises.mkdtemp`)**: The behavior of `fsPromises.mkdtemp()` can vary across platforms, especially concerning trailing 'X' characters in the prefix, as some BSD platforms might return more than six random characters or replace trailing 'X's. [4](#ref-q1-4)
*   **File Naming Conventions**: Windows has reserved characters (`< > : " / \ | ? *`) for filenames, and using a colon in a filename under NTFS will open a file system stream. [5](#ref-q1-5)
*   **Symbolic Links (`fs.symlinkSync`)**: Windows does not handle symbolic links (`symlink`) well, leading to tests being skipped on this platform. [6](#ref-q1-6)
*   **File Watching (`fs.watch`)**: The `fs.watch` API is highly inconsistent across platforms and relies on underlying operating system features like `inotify(7)` on Linux, `kqueue(2)` on BSD/macOS, `FSEvents` for macOS directories, `event ports` on SunOS, and `ReadDirectoryChangesW` on Windows. [7](#ref-q1-7)  This can lead to unreliability or unavailability on network file systems or with virtualization software. [8](#ref-q1-8)
    *   On Windows, no events are emitted if the watched directory is moved or renamed, and an `EPERM` error occurs if it's deleted. [9](#ref-q1-9)
    *   On Linux and macOS, `fs.watch()` watches inodes, so if a file is deleted and recreated, the watch continues on the original inode, missing events for the new one. [10](#ref-q1-10)
    *   AIX files retain the same inode, resulting in two notifications for saving and closing a watched file. [11](#ref-q1-11)
    *   The `filename` argument in the `fs.watch` callback is only supported on Linux, macOS, Windows, and AIX, and is not always guaranteed to be provided even on these platforms. [12](#ref-q1-12)

### Non-Deterministic Behaviors
*   **File Copying (`fs.copyFile`, `fsPromises.copyFile`)**: Node.js makes no guarantees about the atomicity of copy operations. If an error occurs after the destination file is opened for writing, an attempt is made to remove the destination, but the state before the error is not guaranteed. [13](#ref-q1-13)  [14](#ref-q1-14)  The synchronous version `fs.copyFileSync()` also has this non-atomic behavior. [15](#ref-q1-15)
*   **File Writing with File Descriptors (`fs.writeFile`)**: When `fs.writeFile()` is used with a file descriptor, it retries writing until all data is written or an error occurs. However, the file is not replaced, and the data is not necessarily written to the beginning, meaning original data might remain before or after the new data. [16](#ref-q1-16)

### Unsupported Behaviors
*   **File Watching on IBM i**: The `fs.watch` feature is not supported on IBM i systems. [17](#ref-q1-17)
*   **`fs.constants` for File Modes**: Constants like `S_ISVTX`, `S_ISGID`, or `S_ISUID` are not exposed in `fs.constants` because using raw numbers larger than `0o777` for file modes may result in platform-specific behaviors that are not consistently supported. [18](#ref-q1-18)
*   **Copying Specific File Types (`fs.cp`)**: The `fs.cp()` API explicitly throws errors for unsupported copy operations, such as copying a directory to a non-directory (`ERR_FS_CP_DIR_TO_NON_DIR`), a non-directory to a directory (`ERR_FS_CP_NON_DIR_TO_DIR`), named pipes (`ERR_FS_CP_FIFO_PIPE`), or sockets (`ERR_FS_CP_SOCKET`). [19](#ref-q1-19)

### Broader Non-FS Stdlib Validation Work
*   **`fs.access()` for Pre-checks**: It is explicitly stated that `fs.access()` should not be used to check for file accessibility before calling `fs.open()`, `fs.readFile()`, or `fs.writeFile()`. This is because it introduces a race condition, and instead, the user should directly attempt the operation and handle any errors. [20](#ref-q1-20)  This is a validation guideline for correct usage within the standard library rather than a specific `fs` behavior.

## Notes
The prompt refers to "fs parity work," which is a general term for efforts to standardize `fs` behavior across platforms. The provided context does not contain specific details about this "parity work" itself, but rather highlights existing platform-specific differences and behaviors within the `fs` module. The information above is derived from the current state of the `fs` documentation and related code, indicating what remains platform-dependent or non-deterministic.

Wiki pages you might want to explore:
- [Core APIs (nodejs/node)](/wiki/nodejs/node#3)

View this search on DeepWiki: https://deepwiki.com/search/after-fs-parity-work-lands-whi_7cdf8fdf-e13a-42c0-a009-4cd1a24e9e59

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/fs.md:2366-2369`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2366-L2369)

```markdown
Caveats: on Windows only the write permission can be changed, and the
distinction among the permissions of group, owner, or others is not
implemented.
```

<a id="ref-q1-2"></a>
### [2] `doc/api/fs.md:2370-2403`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2370-L2403)

```markdown
### `fs.chown(path, uid, gid, callback)`

<!-- YAML
added: v0.1.97
changes:
  - version: v18.0.0
    pr-url: https://github.com/nodejs/node/pull/41678
    description: Passing an invalid callback to the `callback` argument
                 now throws `ERR_INVALID_ARG_TYPE` instead of
                 `ERR_INVALID_CALLBACK`.
  - version: v10.0.0
    pr-url: https://github.com/nodejs/node/pull/12562
    description: The `callback` parameter is no longer optional. Not passing
                 it will throw a `TypeError` at runtime.
  - version: v7.6.0
    pr-url: https://github.com/nodejs/node/pull/10739
    description: The `path` parameter can be a WHATWG `URL` object using `file:`
                 protocol.
  - version: v7.0.0
    pr-url: https://github.com/nodejs/node/pull/7897
    description: The `callback` parameter is no longer optional. Not passing
                 it will emit a deprecation warning with id DEP0013.
-->

* `path` {string|Buffer|URL}
* `uid` {integer}
* `gid` {integer}
* `callback` {Function}
  * `err` {Error}

Asynchronously changes owner and group of a file. No arguments other than a
possible exception are given to the completion callback.

See the POSIX chown(2) documentation for more detail.
```

<a id="ref-q1-3"></a>
### [3] `doc/api/fs.md:962-974`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L962-L974)

```markdown
### `fsPromises.chown(path, uid, gid)`

<!-- YAML
added: v10.0.0
-->

* `path` {string|Buffer|URL}
* `uid` {integer}
* `gid` {integer}
* Returns: {Promise} Fulfills with `undefined` upon success.

Changes the ownership of a file.
```

<a id="ref-q1-4"></a>
### [4] `doc/api/fs.md:1298-1303`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1298-L1303)

```markdown
Creates a unique temporary directory. A unique directory name is generated by
appending six random characters to the end of the provided `prefix`. Due to
platform inconsistencies, avoid trailing `X` characters in `prefix`. Some
platforms, notably the BSDs, can return more than six random characters, and
replace trailing `X` characters in `prefix` with random characters.
```

<a id="ref-q1-5"></a>
### [5] `doc/api/fs.md:1378-1380`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1378-L1380)

```markdown
Some characters (`< > : " / \ | ? *`) are reserved under Windows as documented
by [Naming Files, Paths, and Namespaces][]. Under NTFS, if the filename contains
a colon, Node.js will open a file system stream, as described by
```

<a id="ref-q1-6"></a>
### [6] `benchmark/fs/bench-symlinkSync.js:8-11`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/benchmark/fs/bench-symlinkSync.js#L8-L11)

```javascript
if (process.platform === 'win32') {
  console.log('Skipping: Windows does not play well with `symlink`');
  process.exit(0);
}
```

<a id="ref-q1-7"></a>
### [7] `doc/api/fs.md:4843-4868`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4843-L4868)

```markdown

The `fs.watch` API is not 100% consistent across platforms, and is
unavailable in some situations.

On Windows, no events will be emitted if the watched directory is moved or
renamed. An `EPERM` error is reported when the watched directory is deleted.

The `fs.watch` API does not provide any protection with respect
to malicious actions on the file system. For example, on Windows it is
implemented by monitoring changes in a directory versus specific files. This
allows substitution of a file and fs reporting changes on the new file
with the same filename.

##### Availability

<!--type=misc-->

This feature depends on the underlying operating system providing a way
to be notified of file system changes.

* On Linux systems, this uses [`inotify(7)`][].
* On BSD systems, this uses [`kqueue(2)`][].
* On macOS, this uses [`kqueue(2)`][] for files and [`FSEvents`][] for
  directories.
* On SunOS systems (including Solaris and SmartOS), this uses [`event ports`][].
* On Windows systems, this feature depends on [`ReadDirectoryChangesW`][].
```

<a id="ref-q1-8"></a>
### [8] `doc/api/fs.md:4872-4877`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4872-L4877)

```markdown
If the underlying functionality is not available for some reason, then
`fs.watch()` will not be able to function and may throw an exception.
For example, watching files or directories can be unreliable, and in some
cases impossible, on network file systems (NFS, SMB, etc) or host file systems
when using virtualization software such as Vagrant or Docker.
```

<a id="ref-q1-9"></a>
### [9] `doc/api/fs.md:4847-4849`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4847-L4849)

```markdown
On Windows, no events will be emitted if the watched directory is moved or
renamed. An `EPERM` error is reported when the watched directory is deleted.
```

<a id="ref-q1-10"></a>
### [10] `doc/api/fs.md:4885-4888`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4885-L4888)

```markdown
On Linux and macOS systems, `fs.watch()` resolves the path to an [inode][] and
watches the inode. If the watched path is deleted and recreated, it is assigned
a new inode. The watch will emit an event for the delete but will continue
watching the _original_ inode. Events for the new inode will not be emitted.
```

<a id="ref-q1-11"></a>
### [11] `doc/api/fs.md:4891-4892`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4891-L4892)

```markdown
AIX files retain the same inode for the lifetime of a file. Saving and closing a
watched file on AIX will result in two notifications (one for adding new
```

<a id="ref-q1-12"></a>
### [12] `doc/api/fs.md:4899-4902`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4899-L4902)

```markdown
Providing `filename` argument in the callback is only supported on Linux,
macOS, Windows, and AIX. Even on supported platforms, `filename` is not always
guaranteed to be provided. Therefore, don't assume that `filename` argument is
always provided in the callback, and have some fallback logic if it is `null`.
```

<a id="ref-q1-13"></a>
### [13] `doc/api/fs.md:1006-1008`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L1006-L1008)

```markdown
No guarantees are made about the atomicity of the copy operation. If an
error occurs after the destination file has been opened for writing, an attempt
will be made to remove the destination.
```

<a id="ref-q1-14"></a>
### [14] `doc/api/fs.md:2466-2469`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2466-L2469)

```markdown
callback function. Node.js makes no guarantees about the atomicity of the copy
operation. If an error occurs after the destination file has been opened for
writing, Node.js will attempt to remove the destination.
```

<a id="ref-q1-15"></a>
### [15] `doc/api/fs.md:5537-5539`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L5537-L5539)

```markdown
already exists. Returns `undefined`. Node.js makes no guarantees about the
atomicity of the copy operation. If an error occurs after the destination file
has been opened for writing, Node.js will attempt to remove the destination.
```

<a id="ref-q1-16"></a>
### [16] `doc/api/fs.md:5297-5306`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L5297-L5306)

```markdown
The difference from directly calling `fs.write()` is that under some unusual
conditions, `fs.write()` might write only part of the buffer and need to be
retried to write the remaining data, whereas `fs.writeFile()` retries until
the data is entirely written (or an error occurs).

The implications of this are a common source of confusion. In
the file descriptor case, the file is not replaced! The data is not necessarily
written to the beginning of the file, and the file's original data may remain
before and/or after the newly written data.
```

<a id="ref-q1-17"></a>
### [17] `doc/api/fs.md:4870`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L4870)

```markdown
* On IBM i systems, this feature is not supported.
```

<a id="ref-q1-18"></a>
### [18] `doc/api/fs.md:2361-2364`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2361-L2364)

```markdown
When using raw numbers where file modes are expected, any value larger than
`0o777` may result in platform-specific behaviors that are not supported to work
consistently. Therefore constants like `S_ISVTX`, `S_ISGID`, or `S_ISUID` are
not exposed in `fs.constants`.
```

<a id="ref-q1-19"></a>
### [19] `doc/api/errors.md:1335-1397`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/errors.md#L1335-L1397)

```markdown
<a id="ERR_FS_CP_DIR_TO_NON_DIR"></a>

### `ERR_FS_CP_DIR_TO_NON_DIR`

<!-- YAML
added: v16.7.0
-->

An attempt was made to copy a directory to a non-directory (file, symlink,
etc.) using [`fs.cp()`][].

<a id="ERR_FS_CP_EEXIST"></a>

### `ERR_FS_CP_EEXIST`

<!-- YAML
added: v16.7.0
-->

An attempt was made to copy over a file that already existed with
[`fs.cp()`][], with the `force` and `errorOnExist` set to `true`.

<a id="ERR_FS_CP_EINVAL"></a>

### `ERR_FS_CP_EINVAL`

<!-- YAML
added: v16.7.0
-->

When using [`fs.cp()`][], `src` or `dest` pointed to an invalid path.

<a id="ERR_FS_CP_FIFO_PIPE"></a>

### `ERR_FS_CP_FIFO_PIPE`

<!-- YAML
added: v16.7.0
-->

An attempt was made to copy a named pipe with [`fs.cp()`][].

<a id="ERR_FS_CP_NON_DIR_TO_DIR"></a>

### `ERR_FS_CP_NON_DIR_TO_DIR`

<!-- YAML
added: v16.7.0
-->

An attempt was made to copy a non-directory (file, symlink, etc.) to a directory
using [`fs.cp()`][].

<a id="ERR_FS_CP_SOCKET"></a>

### `ERR_FS_CP_SOCKET`

<!-- YAML
added: v16.7.0
-->

An attempt was made to copy to a socket with [`fs.cp()`][].
```

<a id="ref-q1-20"></a>
### [20] `doc/api/fs.md:2062-2066`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/fs.md#L2062-L2066)

```markdown
Do not use `fs.access()` to check for the accessibility of a file before calling
`fs.open()`, `fs.readFile()`, or `fs.writeFile()`. Doing
so introduces a race condition, since other processes may change the file's
state between the two calls. Instead, user code should open/read/write the
file directly and handle the error raised if the file is not accessible.
```
