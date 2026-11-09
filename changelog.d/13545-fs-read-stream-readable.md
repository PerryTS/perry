`fs.createReadStream` returned a bespoke stream object. It now returns a
`stream.Readable` that reads the file descriptor sequentially, and `Readable`
emits `readable` for data still buffered when EOF arrives.

Preserve caller-owned descriptors with `autoClose: false` when
`fs.promises.writeFile()` consumes a read stream, and emit `end` before `close`.

Consume the remaining buffered bytes as well as subsequent reads when
`fs.promises.writeFile()` receives a partially read file stream.

Resume deferred iterable writes through queued promise reactions, retaining
buffered and future chunks until real completion without blocking native read
callbacks or closing caller-owned destination descriptors.
