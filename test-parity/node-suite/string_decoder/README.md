# node:string_decoder parity suite

Granular Node compatibility cases for `node:string_decoder`, curated from the
broad `test-files/test_parity_string_decoder.ts` smoke test plus Node/Deno
StringDecoder behavior around incremental multibyte decoding.

Cases are intentionally small and deterministic so each failure maps to one API
family: imports, constructor/encoding normalization, UTF-8 chunking, UTF-16LE
surrogates, base64/hex/latin1/ascii encodings, `.end()` flush behavior, state
properties, and accepted input views.

Compatibility target: Node 22+ / current LTS behavior. Do not add legacy-only
upstream cases unless they still describe Node 22/24 semantics.
