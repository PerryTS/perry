---
category: Runtime
title: Complete Bun and Node C-ABI FFI support
---

Perry now supports typed scalar C-ABI calls through `bun:ffi`, including stack
arguments, pinned pointers, zero-copy native memory views, scalar reads, and
rooted same-thread or threadsafe callbacks. A Node 26-compatible `node:ffi`
adapter lets OpenTUI/Yoga and other native wrappers load their upstream shared
libraries without source changes, and the real `bun-pty` shell roundtrip is
covered end to end.
