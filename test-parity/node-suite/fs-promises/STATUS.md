# node:fs/promises parity status

The promise suite is tracked separately from `node:fs` because the import surface,
async return values, FileHandle model, and rejection behavior need dedicated
coverage. See `../fs/STATUS.md` for the combined fs/fs-promises coverage count,
reviewed upstream sources, and the follow-up gap list.
