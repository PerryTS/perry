### Fix compile environment identity in both caches (#12582)

The per-module object cache and completed-build cache now use one sorted
`COMPILE_ENV_VARS` list, with lossless OS-value encoding and a distinction
between an unset and an empty environment variable. This removes the separate
handwritten object-cache fields that omitted `PERRY_SEGVIEW` and other lowering
gates. Existing cache formats are invalidated.

The inventory includes HIR, transform, native and optional backends, compile
configuration, toolchain/link inputs and runtime-feature selection. A source
audit automatically follows the compiler's workspace dependencies and requires
every literal `PERRY_*` environment read to be keyed or explicitly excluded
with a reason. It checks a minimum read count and recognizes `var`, `var_os`,
`env!`, `option_env!`, and the HIR environment helper. List-driven tests check
that every input changes object identity, including empty values; segment-view
and contract-mode integration tests cover cold/warm cache transitions.
