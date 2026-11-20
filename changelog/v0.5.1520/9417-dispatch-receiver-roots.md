### Fixed

- **A dynamic instance-method call no longer loses its receiver to the GC
  (#9417).** `lower_call/property_get/dynamic_dispatch.rs` lowered the receiver
  first — JS evaluation order requires the MemberExpression to be evaluated
  before the arguments — and then consumed it last, in the own-override probe,
  the class-id tower and `js_native_call_method`. Every argument expression was
  lowered in between, and an argument is arbitrary user code that can allocate.
  A bare SSA register is not a GC root, so an evacuating young-gen minor inside
  an argument left the receiver naming from-space.

  Nothing faulted at the move. `js_object_get_own_field_or_undef` failed its
  `obj_type == GC_TYPE_OBJECT` check on the recycled cell and answered
  `TAG_UNDEFINED`, so the override probe missed and the by-name dispatch ran on
  a retired address — the failure surfaced as a wrong answer several steps
  downstream, naming a property unrelated to the defect. In the Claude Code
  bundle that was `Cannot read properties of undefined (reading 'def')` on the
  request-build path, from zod's `ZodObject.extend`; unauthenticated
  `--input-format stream-json` went from 24/25 runs bad to 0/25.

  Both dispatch sites in that file — the unknown-receiver-class path and the
  known-class virtual tower — now root the receiver and every argument in one
  `RootedGroup` and re-read below the group, the same combinator
  `early_branches.rs`'s computed-key dispatch (`obj[k](…)`) has used since
  #7210. `root_reload` then re-derives each later use that a collection point
  can reach. `operand_protection` still decides how each operand is protected,
  so a provably non-pointer argument costs nothing.

  `test-files/test_gap_9417_dispatch_receiver_roots.ts` reproduces the wrong
  answer deterministically with no GC environment knobs, and
  `temp_root_coverage::dispatch_receiver` pins the emission contract under both
  root lowerings.
