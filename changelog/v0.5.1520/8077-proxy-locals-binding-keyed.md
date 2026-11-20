### Fixed

- **Proxy lowering now follows the resolved binding instead of an identifier's
  spelling (#7775).** A proxy local in one function no longer causes same-named
  plain objects, parameters, loop bindings, or destructured values elsewhere
  in the module to use proxy operations and silently read as `undefined`.
  Proxies declared in class methods, arrow bodies, `Proxy.revocable`
  destructuring, and forward-referenced module bindings continue to take the
  proxy path.
