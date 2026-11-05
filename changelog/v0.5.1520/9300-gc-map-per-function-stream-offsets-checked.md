The compact GC map's varint stream is chained: reaching function `i`'s records
means decoding functions `0..i` first. That is why the runtime decodes the whole
section — 2,078,970 records for claude-code — to answer the 74 record lookups a
`--help` run actually makes. Giving a decoder a per-function starting offset is
what ends that, and this change is the half of it that carries all the risk.

`encode_stream` now records, for each function, the stream position at which its
records begin, and `verify_roundtrip` checks that value against the position the
sequential decode reaches. **Nothing emits these offsets and nothing reads them.**
The emitted assembly is byte-identical and the format is still v4.

That is the point. The offsets are the one new datum a per-function decoder would
have to trust, and trusting a wrong one means reading another function's live set
— the collector scanning the wrong words, which is the failure this file exists to
prevent. So the check lands first, against behaviour it cannot change, where it
must never fire. It runs on every compile over every function of the binary being
produced, which for claude-code is 72,669 functions and 2,078,970 records of real
data rather than a test fixture's handful.

The value is also not computed twice and compared. `function_offsets[i]` is
`bytes.len()` at the moment the encoder begins function `i`: it *is* the position,
by construction, recorded on one line that nothing else recomputes. What
`verify_roundtrip` adds is proof that the decoder's idea of where function `i`
starts agrees with the encoder's — which is the only way the two can disagree.

`roundtrip_check_catches_a_wrong_per_function_stream_offset` plants each way an
offset can be wrong (off by one in either direction, off by four, and one offset
short of one per function) and asserts each is rejected, in the same spirit as the
existing corrupted-stream test: a verifier that only ever agrees with itself is the
gate that runs while its subject never did.
