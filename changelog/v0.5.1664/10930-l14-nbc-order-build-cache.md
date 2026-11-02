Registered `PERRY_L14_NBC_ORDER` as a build-cache input.

#10929 added the knob and keyed it into the **object** cache
(`commands/compile/object_cache.rs`) but not the **build** cache, so
`codegen_env_vars_are_build_cache_inputs` — #6394's rule, that every codegen
switch must key the build cache or carry a written exclusion — failed:

```
these codegen env vars key neither the build cache nor an exclusion
(#6394's rule): ["PERRY_L14_NBC_ORDER"].
```

It belongs in `BUILD_CACHE_ENV_VARS`, not the exclusions: on, an accumulator
written `h = h + o.a` is admitted and the `+` routes to `INLINE_FADD`; off, the
shape inputs are empty and it stays guarded. The two settings emit different
code, so an object built under one must not be served to a build of the other.
