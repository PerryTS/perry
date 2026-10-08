Buffer B5 closes byte-layout debt: the layout gate now requires zero findings outside `buffer::store` and `perry-abi`, with no baseline or update mode. Its executable self-test independently plants header-size assumptions, literal pointer offsets, length/capacity writes, old raw byte-helper calls, emitted literal offsets, and allocation followed by rebranding. CI and `run_lint_gates.sh` both run this gate.

`BufferHeader` is opaque outside `buffer::store`; `TypedArrayHeader` aliases the same private-field type. Store owns allocation, extent reads, and the collector's traced link slot. Remaining stream consumers use scoped bytes. Existing readers and test fixtures use store accessors without changing stored lengths, capacities, offsets, or layout.

Every byte cell is allocated with its final brand, including resizable/foreign ArrayBuffers, Uint8Array views, slice/clone results, secret keys, and WebCrypto keys. The byte rebrand functions and their external stamping entry points are deleted. CryptoKey metadata registration requires a cell already born as CryptoKey and never changes its brand. Asymmetric string-key metadata remains unchanged. This removes brand-after-birth transitions and introduces no caches, side tables, latches, or name checks.

Validation and instructions/RSS A/B results are recorded in the B5 report. Versions are unchanged.
