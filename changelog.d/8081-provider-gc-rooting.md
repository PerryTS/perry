### Fixed

- Preserve native GC roots when Perry runs behind separately loaded runtime and
  stdlib providers and generated application code lives in an app-only dynamic
  library (#8075). The runtime previously indexed only the process executable's
  compact stack map, then cached that incomplete view permanently. Provider
  hosts could therefore complete a full collection at a clean host boundary
  and corrupt values used by the next imported-handler invocation.

- Rebuild the stack-map index at module initialization and discover compact GC
  maps in every loaded Mach-O or ELF image. Generation-ordered publication
  prevents an older concurrent loader snapshot from replacing a newer index,
  while root scanning never performs loader I/O. Linux fails closed rather
  than publishing an incomplete index when a loaded ELF image cannot be read,
  and section addresses are checked against the loader's mapped segments
  before use.

- Add a provider-host integration gate for Linux and macOS. It builds separate
  runtime and stdlib providers plus an app-only two-module dylib, validates
  32,768 JSON/Buffer responses from serial and concurrently queued callers,
  forces at least ten full collections, covers retained and temporary Buffers,
  requires reclaimed bytes with a flat latter-half live set, verifies the app
  map survives macOS dead stripping, and isolates provider build artifacts.
