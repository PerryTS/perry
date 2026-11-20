Fix class-method values replacing primitive, null, undefined, and array receivers with an internal owner marker. Calls through `call`, `apply`, `bind`, and detached method values now preserve the actual strict-mode receiver. A value-only dispatch entry preserves subnormal numeric receivers without interpreting their bits as raw object pointers.

Regression coverage checks exact receiver identity, ordinary objects, arrays, functions, proxies, rest arguments, and native output against Node 26.5.1.
