# Retained function source

Permanent compiled function infos can name independent 4 KiB raw-DEFLATE
frames in the retained-source section. `FN_SOURCE_BLOCKS`, together with
`FN_HAS_SOURCE`, describes this immutable representation. Each frame has two
little-endian `u32` lengths (compressed, decoded), followed by its payload.
Frames are adjacent; the next header is at `frame + 8 + compressed_len`.

The info directly owns the first frame's signed displacement from the info,
the offset within that decoded frame, and the exact source byte length. When
there is no versioned clone, these occupy the existing idle versioned fields:
`versioned_captures` is the source length and the mask's storage holds an
`i32` displacement followed by a `u32` decoded offset. Common infos remain
64 bytes. A versioned clone instead licenses a 16-byte source tail containing
displacement, decoded offset, source length and padding. Name and constructor
tails follow that authoritative layout. Raw legacy info ranges remain readable.

A source read decodes into call-local scratch storage, copies the requested
intersection, then follows adjacent headers if the range spans frames. The
last frame produces only the prefix through the requested range end; its
unneeded suffix is not inflated. Fully traversed frames still validate stream
completion and lengths. Frames are trusted compiler-owned image data. No
decoded block, decoder state or runtime lookup index survives the call. Nested
ranges share frames. Frame boundaries may split Unicode sequences; byte slicing
and assembly happen before the original range becomes a string. Nothing
normalizes, replaces or truncates the original source bytes.

A function whose own source is at most 512 bytes (`RETAINED_SOURCE_RAW_MAX_BYTES`)
is not framed: its info names a raw copy in the ordinary retained-source blob,
the legacy layout without `FN_SOURCE_BLOCKS`. This is a fixed size rule on the
function itself, not a hot/cold split and not a parent-size rule; the bytes also
stay inside any longer parent's frames. It needs no runtime table or cache.

Classes, raw method/accessor bodies, runtime source overrides and unloadable
images keep their existing raw registration and ownership contracts. In
particular, module initialization does not eagerly decode retained source.

## Reflection census

Set `PERRY_SOURCE_READ_COUNT=1` when running a program to report these counters
on stderr at natural exit or explicit `process.exit()`:

- `function_to_string`: ordinary closure source requests, including implicit
  string coercion;
- `class_source`: class source requests, including default coercion and error
  formatting;
- `retained_info`: requests actually served by a source-bearing function info;
- `framed`: the part of `retained_info` that decoded DEFLATE frames; the rest read raw bytes (functions whose own source is at most 512 bytes);
- `class_error`: the subset of class requests used by readonly-static errors.

Class closures use the class counter rather than counting twice. Counters
overlap where an ordinary function request is served by its info. The census
is off by default, owns no source or GC pointers, and does not affect emitted
code or object-cache identity. If no source helper runs, no census line is
registered and all counts are zero. The environment setting is read on the
first source request and is fixed for that process's census.
