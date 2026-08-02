- Fix TanStack Start package subpath resolution, JSX/runtime binding selection,
  closure-contained dynamic imports, and hoisted `TextEncoder`/`TextDecoder`
  inference.
- Preserve pull-driven `ReadableStream` bodies through `Response` construction
  and cloning, reject invalid `GET`/`HEAD` stream bodies before consuming them,
  and expose the fetch/abort reflection surface expected by response wrappers.
- Resolve nested `?script-string` assets relative to their importer and preserve
  their source exactly when generating hydration bootstrap modules.
