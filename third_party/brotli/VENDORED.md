# Brotli 9.0.0 library snapshot

Source: the official cached crates.io `brotli-9.0.0.crate` published by
[dropbox/rust-brotli](https://github.com/dropbox/rust-brotli).
Archive SHA-256: `f8b851b75c23ca7873623d612fe49bd1989aeb03d08fb9432187eb253d3d4c6b`.
Both upstream licenses are retained: `LICENSE.BSD-3-Clause` and `LICENSE.MIT`.

The import contains the complete `src/` library closure (excluding `src/bin/`),
`Cargo.toml`, `Cargo.toml.orig`, `README.md`, and both licenses: 63 original
files, 233,974 pristine lines. Most lines are upstream generated lookup tables.
No registry cache was modified. The workspace patch replaces only Brotli9's
source; Brotli8, decoder6.0.1 and every locked dependency version are unchanged.

## Authored changes relative to the archive

- `Cargo.toml`: remove the excluded binary/example targets; add the focused
  `custom_prefix` integration-test target. Original metadata remains available
  in the unchanged `Cargo.toml.orig`.
- `src/enc/encode.rs`: capture the effective dictionary size after existing
  window truncation, and permanently expire the supplementary prefix metadata
  using the unwrapped encoder position.
- `src/enc/backward_references/hash_to_binary_tree.rs`: retain/clone/compare that
  small metadata record alongside H10; tree storage and ordering are unchanged.
- `src/enc/backward_references/hq.rs`: search the unindexed prefix tail alongside
  normal H10 matches; merge non-dominated, length-ordered matches with nearest
  distance ties, without changing quality10/11 parsing or tree lookahead.
- New `src/enc/backward_references/custom_prefix.rs`: bounded search of at most
  127 positions, limited by available history/input, dictionary break, and
  physical byte slices. No padded dictionary or fabricated error is used.
- New `tests/custom_prefix.rs`: public-codec roundtrip/quality/stream controls,
  stock wire literals, and the private search's bounds/expiry invariants.

All other imported files remain byte-identical to the verified archive.
The five upstream files above the repository's Rust file-size cap retain their
original layout under exact-path exceptions. `encode.rs` is locally patched;
it is **not** claimed to be byte-identical. New owned files remain below the cap.

## Known upstream control limitation

The dictionary `[b'a';256]` with input `[b'a';400]` at quality10/11 emits an
11-byte stream rejected by the decoder in **both pristine and patched Brotli9**;
quality9 succeeds. This pre-existing HQ issue is not repaired here. Its exact
stock/candidate control evidence was retained during verification; the repeated
prefix regression uses `b"abcXYZ".repeat(30)`, whose pristine HQ roundtrip is
valid, rather than hiding that separate failure with an ignored test.
