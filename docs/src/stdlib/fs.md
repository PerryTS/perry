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

## Next Steps

- [HTTP & Networking](http.md)
- [Overview](overview.md) — All stdlib modules
