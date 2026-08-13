### Make production Next.js lazy path modules deadlock-free

Production webpack modules under `.next/server` now initialize exactly once
when first required by canonical path, including from app-only dylibs backed by
shared runtime providers. Concurrent callers wait without holding loader locks,
CommonJS cycles can observe partial exports, real `undefined` exports remain
distinguishable from misses, and initialization failures are cached and replayed
without retry.
