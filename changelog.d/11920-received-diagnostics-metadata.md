**Node-compatible received-value brands and constructor metadata.**

Shared argument errors now preserve a value's intrinsic brand and observable
constructor name before falling back to `an instance of Object`: Buffer,
TypedArray and DataView views keep their real identity, inherited prototype
constructor data, accessors and throw behavior are honored, and lone UTF-16
surrogate code units survive name interpolation. Native negative-depth fallback
classifies the whole recorded prototype chain — a replacement that inherits an
intrinsic prototype, or one ending in a null-born prototype, keeps its native
label and payload — while inspecting link authority only, so diagnosis never
performs an extra user-visible `constructor` Get, accessor call or Proxy trap.

The specialized JSON string emitter now rejects registry handle-band pointers
with the canonical heap-address plausibility guard instead of a raw address
floor, so no handle value is dereferenced as a string header.
