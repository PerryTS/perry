Fix `fs.write`/`fs.writeSync` (and the `fs.read`/`fs.readSync` core) reading
their offset, length and position arguments as raw doubles. An explicit
`undefined` or `null` is a NaN-boxed value, so `fs.write(fd, buf, undefined,
undefined, undefined, cb)` wrote 0 bytes; Effect's `FileSystem` handle writes
every flushed log batch that way, which left OpenCode's log file empty. The
arguments are now read as JS numbers (double or int32): an absent offset is 0,
a non-number write length is the rest of the buffer, a non-number read length
is 0 (`length | 0`), and a non-number or negative position is the current file
position, as in Node.
