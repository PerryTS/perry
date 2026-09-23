fix(stream): a readable fed by live `push()`/`write()` output now ends only at
EOF (#11045). A chunk emitted to a flowing reader stayed in the retained
buffer (only pipe destinations consumed it), so the next resume/drain
microtask replayed it and then emitted `'end'` because the buffer ran dry —
the following `write()` threw `ERR_STREAM_WRITE_AFTER_END`. This broke
nodemailer's `sendMail`, which writes MIME headers into a PassThrough before
piping the body in with `{ end: false }`. Live emits now always consume the
buffered front (the pipe-only marker is gone), and the drain ends a
live-push stream only after `push(null)`/a finished writable side; snapshot
sources (`Readable.from`, duplex-from-source) still end when drained.
Coverage: `test-files/test_gap_11045_live_push_ends_at_eof.ts`.
