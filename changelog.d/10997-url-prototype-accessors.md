Fixed URL component reflection (#10823).

URL instances now keep their twelve component values in internal numbered slots
and inherit enumerable, configurable accessors from `URL.prototype`. This makes
`Object.keys(url)` and `Object.getOwnPropertyNames(url)` empty while preserving
component reads, setters, and `Object.getPrototypeOf(url) === URL.prototype`.
The first ordinary or `Object.defineProperty` expando reserves the internal
slots before appending a user key, so it cannot overwrite `href` or the
`searchParams` owner link. The `host` accessor now also updates hostname and
an optional valid port.

Verified with a compiled TypeScript regression covering descriptors, borrowed
getters, setters, both expando paths, and URLSearchParams behavior on Linux.
