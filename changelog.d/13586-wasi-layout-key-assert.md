Fix the wasm32 (WASI) runtime build: the compile-time check of the
`PerryLayoutKey.off` offset used the 64-bit value (14) on every target, but
with 32-bit pointers the field sits at byte 10, so `perry-runtime` failed to
compile for wasm32-wasip1/wasip2 and the release build-wasi leg went red. The
offset and the key size are now stated per pointer width (14/16 on LP64,
10/12 on ILP32), matching the descriptor constants beside them, and a codegen
test checks the emitted `%PerryLayoutKey` LLVM type has the same offset and
size under both data layouts. Native targets are unchanged.

The release wasip1 step now admits `perry_host.discard_pages` in its import
check (`--allow-perry-host`, as the test workflow already does): the one
wasip1 archive has carried the opt-in page-discard member since it landed,
so the check rejected the archive it was meant to ship.
