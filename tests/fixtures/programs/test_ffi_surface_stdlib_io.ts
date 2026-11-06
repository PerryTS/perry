// Stdlib IO, network, stream, and framework FFI surface inventory.
//
// This fixture is intentionally executable by the normal parity runner,
// but its main purpose is to keep TS-side coverage accounting attached
// to related public FFI shims. Move @covers entries from this
// inventory into behavioral tests as each area gets deeper compatibility
// coverage.
//
// Inventory entries: 0 unique FFI names, 0 declarations.

const testFfiSurfaceStdlibIoVersion = 1;
if (testFfiSurfaceStdlibIoVersion !== 1) {
  throw new Error("unexpected coverage inventory version");
}
console.log("test_ffi_surface_stdlib_io: ok");

/*
@covers
*/
