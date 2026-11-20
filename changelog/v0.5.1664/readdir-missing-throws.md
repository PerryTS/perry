`fs.readdirSync`, `fs.promises.readdir` and `fs.readdir` on a missing
directory or a file now fail with Node's `scandir` error (`ENOENT`,
`ENOTDIR`, ...) instead of returning an empty array; the callback form passes
the error to the callback while argument errors still throw synchronously.
Next.js's `recursiveReadDir` branches on `err.code`, so the empty array made
the standalone server believe a `static` directory existed.
