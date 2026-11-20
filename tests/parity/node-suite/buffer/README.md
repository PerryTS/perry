# node:buffer granular parity suite

Focused Node.js parity cases for Perry's `node:buffer` compatibility layer.

Cases are curated from Node core `test-buffer-*` coverage and Deno's `tests/unit_node/buffer_test.ts`, then converted into small deterministic TypeScript programs so failures identify one API family at a time.


Additional coverage added while reviewing upstream Node/Deno cases includes `base64url` decoding, bad hex truncation, numeric slice coercion, and Buffer iteration.

Further gap-closing coverage adds encoding-aware `byteLength`, `Buffer.of`, and Buffer constants exports.

Numeric byte wrapping now includes negative array/of values (`-1` -> `ff`).

ArrayBuffer source coverage includes `Buffer.from(arrayBuffer, byteOffset, length?)` range selection.

Review pass adds: large-buffer `allocUnsafe`/`allocUnsafeSlow` length parity, signed 64-bit endian round-trip, base64 implicit-padding decode, and an astral-char regression for `byteLength('ascii'|'latin1'|'binary')` (runtime now returns UTF-16 code units to match Node).
