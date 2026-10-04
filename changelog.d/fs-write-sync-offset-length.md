Fixed `fs.writeSync(fd, buffer, offset, length)` and
`fs.writeSync(fd, buffer, offset)`: they were read as the string form, with
the third argument taken as a file position, so each write landed at that
position and wrote the whole buffer whatever the length. They now write
`length` bytes (or the rest) from `offset` at the current file position, as
in Node, through a named import and through the module object.
