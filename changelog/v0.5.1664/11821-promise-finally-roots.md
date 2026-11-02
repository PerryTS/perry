`Promise.prototype.finally` keeps the settled value, the chained promise and
its continuation closures rooted across `onFinally()` (user code) and the
allocations that follow it. A collection there left them at their pre-move
addresses, so `await p.finally(cleanup)` could resume with a dangling value
— a package manager failed at random reading the result of
`resolveTree(...).finally(close)`.
