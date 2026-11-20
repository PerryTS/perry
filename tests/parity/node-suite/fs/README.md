# node:fs parity cases

Granular `node:fs` coverage derived from deterministic portions of Node's
`test/parallel/test-fs*` family, Deno's `tests/unit_node/_fs` suite, and Bun's
Node-compatibility fixtures under `test/js/node/fs` plus its vendored Node
`test-fs*` cases.

The cases intentionally avoid host- or timing-dependent APIs (`watch`, exact
mtimes, uid/gid ownership, complex permission failures) and focus on stable
filesystem behavior that Perry can compare byte-for-byte against Node:
imports, constants, glob, watch/watchFile object surface, sync I/O including Buffer writes and fd buffer writes, fd open/close/read/write/readv/writev/stat/chmod/truncate/fsync/fdatasync/times basics plus raw-fd readFile/writeFile, callback APIs including exists/open/close and fd read/write string+buffer/stat/chmod/truncate/fsync/times plus cp/rmdir/readlink/realpath/mkdtemp, statfs, stats,
Dirents, recursive readdir, opendir/Dir, links/symlinks, recursive copy/mkdir/rm, truncate, and basic read/write streams.
