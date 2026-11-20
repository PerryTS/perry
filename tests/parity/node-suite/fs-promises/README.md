# node:fs/promises parity cases

Granular coverage for the Promise-based filesystem module. Kept separate from
`node-suite/fs` because the import surface, async return values, FileHandle API,
and error behavior are distinct from callback/sync `node:fs`.

Current coverage includes deterministic import-shape parity plus functional
Promise operations for read/write/append including Buffer data, directory mutation, recursive readdir, stats/lstat, chmod,
copy/recursive copy, links/symlinks/readlink, truncate, mkdtemp, rm/rmdir/unlink,
rename, access, statfs, glob/watch import surface, opendir/Dir, utimes/lutimes, and a FileHandle subset (`open`, `readFile`,
`writeFile`, `appendFile`, `read`, `write`, `readv`, `writev`, `stat`, `chmod`, `utimes`, `truncate`, `sync`, `close`, `readLines`,
`readableWebStream`, `pull`, `pullSync`, `writer`). FileHandle stream behavior is covered for createReadStream/createWriteStream,
readable Web Streams, and no-transform stream-iter source/writer paths; broader readline integration and transform pipelines remain future work.
