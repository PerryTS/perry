`Buffer.prototype.write(string, offset, length, encoding)` now handles its arguments the way Node's `lib/buffer.js` does, so an explicit `undefined` is the same as an omitted argument (#11291). Before this fix, `buf.write(s, off, undefined, "utf8")` threw `ERR_INVALID_ARG_TYPE: Invalid Buffer length`. bson's `encodeUTF8Into` makes exactly that call for any key or string longer than 25 characters or containing non-ASCII, so `BSON.serialize` failed, and so did mongodb compiled from source.

- Argument handling now matches Node:
  - An undefined offset writes utf8 over the whole buffer and ignores the later arguments, including the encoding.
  - `write(s, enc)` takes the encoding from the offset slot.
  - An undefined length means the remaining bytes, and a string length is the encoding.
  - A numeric length is clamped to the remaining bytes.
  - A falsy encoding (`undefined`, `null`, `""`) means utf8.
  - A non-string encoding is coerced and fails with `ERR_UNKNOWN_ENCODING` (`Unknown encoding: 5`).
- `offset` and `length` are validated with Node's own errors. A non-number is `ERR_INVALID_ARG_TYPE` (`The "offset" argument must be of type number. Received null`). A fraction, a NaN or a value outside `[0, buf.length]` is `ERR_OUT_OF_RANGE`. Previously an out-of-range offset or length was silently clamped, and a negative one was silently treated as 0.
- The change is in `crates/perry-runtime/src/object/buffer_dispatch.rs` (`buffer_write_args`). Unit tests are in `buffer_dispatch_write_args_tests.rs`. The new gap test `test_gap_11291_buffer_write_args` covers every omitted/undefined combination, each error, and the bson `encodeUTF8Into` shape.
