### Fixed

- **`Error.prototype.stack` now carries real, named frames.** Every stack a
  compiled program printed was one line — `    at <anonymous>` — where node
  prints the call chain. `#9432`/`#9410` gave `.stack` its *existence*; nothing
  ever looked at the native stack, so its *content* was a placeholder. That is
  what made `claude doctor`'s raw-mode report and commander's parse-error
  render useless (120 bytes of stderr against node's 14,573), and it is why
  every divergence investigation that touched a compiled app had to reach for
  gdb and a symbolized build.

  Two halves, and the split between them is the design:

  **Capture** is a frame-pointer chain walk on every `new Error` — two loads
  per frame, no allocation, no symbolication. Codegen already tags generated
  functions `"frame-pointer"="non-leaf"`, the property the collector's own
  `fp_chain` walker relies on, so `[fp] = caller fp` / `[fp+8] = return
  address` holds for JS frames. The captured addresses ride in a new
  `ErrorHeader.frames` slot (a `StringHeader`, so it needs no new `GC_TYPE_*`
  and no new rewrite-descriptor arm — one added `visit(...)` line in the
  `GcRewriteDescriptorKind::Error` trace arm covers it).

  **Resolution** happens on the first `.stack` read and reuses the registry
  codegen already fills: `js_register_function_name` records
  `(compiled address, JS display name)` once per function at module init
  (72,713 entries for the claude-code bundle) so `fn.name` and `[Function: f]`
  work. That table is keyed by exact function start; a return address points
  into the middle of a function, so the resolver snapshots it into an
  address-sorted vector once and answers containment with a binary search.
  Codegen now registers the same name against the function BODY symbol
  (`perry_fn_<prefix>__<name>`) as well as the wrapper — a direct call between
  two compiled functions targets the body, so the wrapper address a closure
  value carries is not what a return address points into. Both keys map to the
  same name and `fn.name` still reads the wrapper key, so nothing that
  consulted the registry before sees a different answer.

  Building `.stack` eagerly is what the fix REMOVES: `alloc_error` used to
  decode its own message from UTF-8 and allocate two `String`s per
  construction to produce a line almost no program ever reads. Constructing a
  million errors without reading `.stack` is now cheaper than before, not more
  expensive, and the symbolication — the part that costs — happens only for
  errors whose `.stack` is actually read, then memoises into the `stack` slot.

  **Frames are named but not positioned.** A `file:line:col` needs a
  per-return-address line table, an O(instructions) artifact against this
  one's O(functions); a resolved frame renders as `    at <name> (<anonymous>)`
  — V8's own spelling for a frame whose script position is unknown, which is
  also the `name (location)` shape the stack-parsing libraries in real bundles
  read a name out of. Frames the
  resolver cannot attribute to a registered JS function — the runtime's own,
  between `new Error` and the throwing code — are elided rather than printed
  as bare addresses, the way node elides its internals; a capture in which
  nothing resolves falls back to the pre-fix single `<anonymous>` line, so no
  program loses what it had. Windows and any target without a guaranteed
  frame-pointer chain keep the old behavior rather than guess at a frame
  shape the ABI does not promise.

  **Two limits worth knowing.** Inlining removes frames: `a() → b() → c()`
  where all three are small folds into one function, so the trace names the
  frame that survives rather than all three. V8 keeps inlined frames because it
  retains inlining metadata for deoptimization; an ahead-of-time compiler has
  no such record, and the frames that matter in real traces — the ones across
  `try`/`catch`, callbacks and module boundaries — are exactly the ones the
  inliner does not fold. And a frame is only as good as the registry's
  coverage: an address inside a function nothing registered resolves to
  whichever registered function precedes it, so this change also registers
  class constructors, static methods and accessors, which previously had no
  name of their own and were the ones a neighbour's name leaked into (measured:
  a `new Widget()` frame came out labelled `main`).

  Registering a name is gated on `LlModule::has_function`. `method_names` is
  a DISPATCH registry, not an emission record — it carries keys this module
  never defines a body for, and emitting a registration against one makes
  module init reference an undefined global. The claude-code bundle found
  exactly one, a getter (`UT7.__get_get_extensionName`) out of ~46k functions,
  and failed to compile; nothing smaller than that bundle reproduced it.

- **x86_64 builds now keep frame pointers.** The capture above walks the
  `rbp` / `x29` chain, which is only a chain if every frame between
  `new Error` and the throwing JS function maintains one. Generated code always
  did; the runtime is Rust, and on `x86_64-unknown-linux-gnu` rustc leaves
  `rbp` as a general-purpose callee-saved register — measured, `rbp` inside
  `alloc_error` held `0x1`, so the walk had no root and every stack on that
  target fell back to `at <anonymous>`. `.cargo/config.toml` now adds
  `-C force-frame-pointers=yes` for x86_64 only; the AArch64 platform ABI
  reserves `x29`, which is why the collector's own `fp_chain` walker was
  AArch64-only and why this knob is not needed there.
