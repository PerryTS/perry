### Testing

- Run the cross-module streamed `Response` regression as an app-only dynamic
  library against separately loaded runtime and stdlib providers on Linux and
  macOS. The native host preserves the returned Promise across collection and
  requires exact headers, cookies, two-chunk body, EOF, subclass, and rejected
  stream output in both normal and forced/verified GC modes.
- Require positive moving-GC liveness for the forced provider run, and retain
  and export the fixture's exact Headers, Response, and Streams provider
  surface so duplicated or hidden native registries fail at load time.
