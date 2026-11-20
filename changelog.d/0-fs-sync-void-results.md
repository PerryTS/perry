The synchronous fs mutators (`writeFileSync`, `appendFileSync`, `unlinkSync`,
`closeSync`, `utimesSync`, `truncateSync`, `chmodSync`, `fsyncSync`, `rmSync`
and the rest node declares `void`) return `undefined` on every call path; some
paths returned a boolean or a number. `fs.mkdirSync(path, { recursive: true })`,
`fs.mkdir` and `fs.promises.mkdir` with `recursive` return the first directory
they created (spelled as node spells it), or `undefined` when nothing was created.
