`Buffer.prototype.<method>` values were stubs that returned `undefined`, so
`Buffer.prototype.utf8Slice.call(bytes, start, end)` (used by database drivers
to decode a plain `Uint8Array`) produced nothing. The prototype methods now run
the same implementation as a direct call on the receiver they are invoked with,
whether through `.call`, `.apply`, `bind` or an extracted reference, and a
non-buffer receiver throws a `TypeError`.
