Fixed `fs.Stats` date fields reading `undefined`. `fs.statSync(f).mtime`, the
`fs.stat`/`lstat`/`fstat` callback's `stats.mtime` (and `atime`, `ctime`,
`birthtime`, bigint Stats included) returned `undefined` instead of a `Date`,
so the `send` package (static files in Astro's `@astrojs/node` adapter) threw
on `stat.mtime.toUTCString()` at the first static-file request. A regression
from c2d0318d6 ("class getter reads resolve through the class prototype"):
class accessors now resolve through the class's decl prototype, which is only
built for a class with a registered name, and the runtime-internal Stats
classes registered their Date accessors without one. They now register
`Stats` / `BigIntStats`, Node's constructor names.

The Date aliases also carry an integer time value, as in Node:
`Math.round(mtimeMs)` for Stats and the whole bigint `mtimeMs` for
BigIntStats. Perry kept the fractional milliseconds, so
`st.mtime.getTime() === Math.round(st.mtimeMs)` was false.

Also fixed `fs.promises.<method>()` called on the default `node:fs` import
resolving to `undefined` for every method except `readFile`, `writeFile`,
`appendFile`, `mkdir` and `rmdir` (a pre-existing gap: `fs.promises.stat`,
`lstat`, `readdir`, ... never ran). The codegen arm now falls back to the
generic call path, as the sync `fs.<method>()` arm already does.

Tests: `test-parity/node-suite/fs/stats/date-fields-access-forms.ts` (also
pins `constructor.name` and that a user class named `Stats` keeps its own
accessors) and `test-parity/node-suite/fs/imports/default-import-promises-methods.ts`
(`readdir`, `access`, `stat`, `lstat`, `copyFile`, `rename`, `truncate`,
`unlink`, `realpath`, `mkdtemp`, `rm`, and an `ENOENT` rejection). The
existing `fs/stats/date-fields.ts` and `fs-promises/stats/date-fields.ts` pass
again.
