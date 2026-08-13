### Bug fixes

- Code generation now requires runtime-derived evidence before treating local bindings as non-pointer or native scalar values, preserving JavaScript semantics for erased annotations and keeping BigInt and object values rooted across collection points.
