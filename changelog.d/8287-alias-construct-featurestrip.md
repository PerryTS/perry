### runtime: value-held builtin constructors survive the feature-stripped build (#8223)

`construct.rs`'s dispatch arm for builtins reached through a VALUE (alias
variable, intrinsic lookup, cross-module re-export — Map, Set, WeakMap,
WeakSet, WeakRef, EventTarget, AbortController, TextEncoder, URLSearchParams,
DisposableStack) carried a `#[cfg(feature = "global-webfetch")]` inherited
from #7008's web-platform size gating when #7779 moved the arms out. The
factories behind it are all unconditional modules; auto-optimize builds the
runtime with a minimal feature set (a bare test gets `async-runtime` alone),
so the whole arm compiled out: `new (Map-as-value)()` threw "Constructor Map
requires 'new'", an aliased `EventTarget` had no surface. The prebuilt FULL
stdlib (fast mode) masked it — the exact fast/auto-optimize divergence #8223
documents. Dormant since 08-11; first observed 08-15 only because every
gap-suite run before that was cancelled in the queue saturation.

Cfg removed (the arm is feature-free). Validated A/B in the failing
configuration: pre-fix all three witnesses diverge (two die mid-run), post-fix
all three match node byte-for-byte; `cargo check -p perry-stdlib
--no-default-features --features async-runtime` clean.
