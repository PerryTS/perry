`fs.statSync(path, { throwIfNoEntry: false })` and `fs.lstatSync` with the
same option now return `undefined` when the path (or a parent, ENOTDIR) does
not exist, as node does; they used to throw ENOENT.
