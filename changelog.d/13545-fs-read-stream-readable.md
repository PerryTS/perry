`fs.createReadStream` returned a bespoke stream object. It now returns a
`stream.Readable` that reads the file descriptor sequentially, and `Readable`
emits `readable` for data still buffered when EOF arrives.
