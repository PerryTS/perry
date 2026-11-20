### Fixed

- **A string's bytes could be read after the string moved.** Two runtime
  helpers derived a raw pointer into a `StringHeader`'s payload, then allocated,
  then copied *from that pointer*. An evacuating minor inside the allocation
  relocates the string, so the copy read retired from-space —
  `js_buffer_from_string` (`Buffer.from(str)`) and `js_text_encoder_encode_llvm`
  (`TextEncoder.encode`).

  Not observable from output: evacuation copies rather than zeroes, so the stale
  address still held the correct bytes and both produced right answers.
  `PERRY_GC_PROTECT_FROMSPACE=1` unmaps retired from-space and turns the latent
  read into a fault — which is how they were found (#7341).

  Fixed with a no-move window rather than a root: the borrow is a raw slice
  handed to a callee that reads it at an arbitrary point, so rooting the header
  would leave the slice stale anyway. Each window covers one bounded
  allocate-and-copy.

  Closes **13 of the 54** quarantine catches across the gap suite.
