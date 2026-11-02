### Fixed

- **`stream.setEncoding("utf8")` decodes like Node's `string_decoder` (#9490).**
  Feeding bytes 0..255 through `process.stdin.setEncoding("utf8")` produced
  **158 code units with zero U+FFFD** and raw `U+0080..U+00FF` passing
  through, where Node yields **256 units with 128 replacements**. The encoded
  path handed the raw bytes to `js_string_from_bytes`, which validates
  nothing: it memcpy's them into the string payload and counts UTF-16 units
  with a WTF-8-shaped walk, so continuation bytes were swallowed into the
  preceding lead byte and 98 units simply vanished. `Buffer.toString("utf8")`
  was already correct; only the stream path was wrong.

  The second half of the contract is carry-over. Both the generic `Readable`
  and stdin decoded each chunk independently, so a code point split across a
  chunk boundary became replacement characters — which #9489's chunking change
  makes far more likely, since chunk edges now fall wherever a read lands.

  The fix reuses machinery that already existed rather than adding a second
  copy: the incremental UTF-8 core written for `node:string_decoder`
  (`utf8_check_incomplete` / `write_utf8` / `end_utf8`) moves down from
  `perry-stdlib` into `perry-runtime` as `utf8_stream_decoder::Utf8StreamDecoder`,
  where the stream paths can reach it. `node:string_decoder` now embeds that
  struct, so its `lastNeed` / `lastTotal` / `lastChar` properties and the
  stream decoders cannot drift apart.

  Wired into: `process.stdin`'s `'data'` delivery (both the runtime pump and
  perry-stdlib's readline pump), `process.stdin.read()` (pull mode shares the
  same state), and the generic `Readable`, whose per-stream remainder is held
  in a hidden field exactly like the existing base64 remainder. A sequence
  left incomplete at EOF flushes as one U+FFFD in its own final `'data'`
  event, ahead of `'end'`, and a chunk absorbed whole into the held partial
  emits no event at all — both matching Node.

  Both pumps also decoded the chunk **once per registered listener**. That was
  merely wasteful with a stateless conversion; with carry-over it would have
  fed the same bytes through the decoder N times and handed the second
  listener a continuation of the first one's leftovers. The decode is now
  hoisted above the listener loop, with the resulting value rooted across the
  calls.

  User-visible consequence in claude-code: transcripts of binary stdin
  contained raw `0x80..0xFF` bytes — neither valid UTF-8 nor parseable JSON —
  so `--resume` could not read the session back.

  Fixture `test-files/test_gap_9490_stream_set_encoding_utf8.ts` covers all 256
  byte values, a 4-byte emoji split at every boundary (1/3, 2/2, 3/1), lone
  continuation bytes, 2- and 3-byte overlong encodings, a UTF-8-encoded
  surrogate, a truncated sequence at `end`, and both `'data'`-event strings
  and `readable`/`read()` pulls — asserting exact code-unit sequences and the
  per-event breakdown.
