### Make production Next.js lazy path modules deadlock-free

Production webpack modules under `.next/server` now use a provider-owned
registry for once-only initialization when first required by canonical path.
Executable and app-only dylib entry points both emit lazy initializer
registrations. Concurrent callers wait without holding loader locks, CommonJS
cycles can observe partial exports, real `undefined` exports remain
distinguishable from misses, and initialization failures are cached and replayed
without retry.
