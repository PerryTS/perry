# File System

Perry implements Node.js file system APIs for reading, writing, and managing files.

## Reading Files

```typescript
{{#include ../../examples/stdlib/fs/snippets.ts:read-text}}
```

### Binary File Reading

```typescript
{{#include ../../examples/stdlib/fs/snippets.ts:read-binary}}
```

`readFileBuffer` reads files as binary data (uses `fs::read()` internally, not `read_to_string()`).

## Writing Files

```typescript
{{#include ../../examples/stdlib/fs/snippets.ts:write-text}}
```

## File Streams

`fs.createReadStream()` returns a `stream.Readable`, with buffered `read()`,
`readable` and `data` events, listener management, and async iteration. Its file
reads run on deferred native turns so listeners can be attached before delivery.

Without `start`, a supplied descriptor is read from its current offset. `start`
and inclusive `end` select a byte range. Set `autoClose: false` to keep a
caller-owned descriptor open at EOF, including when the stream is consumed by
`fs.promises.writeFile()`. A partially read stream passes all remaining buffered
bytes and subsequent file reads to `writeFile()` without rewinding the descriptor.
The stream emits `end` once before any automatic `close`; explicit `destroy()` or
`close()` still closes its descriptor. Deferred `writeFile` operations await the
iterator's real completion and yield to native read callbacks between chunks.
A caller-supplied destination descriptor remains open, while a path destination
is owned and closed by the write operation. Callback and promise forms share
these consumption and ownership rules.

## File Information

```typescript
{{#include ../../examples/stdlib/fs/snippets.ts:stat}}
```

## Directory Operations

```typescript
{{#include ../../examples/stdlib/fs/snippets.ts:dirs}}
```

For recursive removal Perry exposes `rmRecursive` (a thin wrapper around
`std::fs::remove_dir_all`). Wired via
[#193](https://github.com/PerryTS/perry/issues/193) through
`js_fs_rm_recursive` in the LLVM backend.

```typescript,no-test
import { rmRecursive } from "fs";
rmRecursive("output"); // Recursive remove; returns 1 on success, 0 on failure.
```

## Path Utilities

```typescript
{{#include ../../examples/stdlib/fs/snippets.ts:path-utils}}
```

For `import.meta.url` → filesystem path conversion, use `fileURLToPath` from
the `url` module:

```text
import { fileURLToPath } from "url";
import { dirname } from "path";

const dir = dirname(fileURLToPath(import.meta.url));
```

## Threading

`fs` numeric file descriptors and `fs.promises.FileHandle` objects are
thread-affine across `perry/thread`. Passing a numeric fd into `spawn` or
`parallelMap` copies only the number; the receiving thread has its own fd
registry, so operations fail with `EBADF`. Passing a `FileHandle` produces a
detached handle with `fd === -1`.

Pass file paths across thread boundaries and reopen files inside the worker.

`perry/thread` deep-copies function-local captures. In descriptor-isolation
tests, create the descriptor and worker closure inside an ordinary function
so the test reaches that capture path instead of the compiler's module-global
heap guard. The FS parity fixtures use this structure without changing their
`EBADF` checks or expected output: this is fixture-structure alignment, not a
runtime or compiler descriptor fix.

## Next Steps

- [HTTP & Networking](http.md)
- [Overview](overview.md) — All stdlib modules
