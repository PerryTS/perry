Local sampling diagnostic of the bounded-preflight worker, with the same small
record fixture and default GC. Worker and sampler exited successfully; the
command and worker hash are recorded. Not quiet-window CPU/RSS evidence.

The small record uses six fields including a primitive tags array. The sample
confirms it reaches shape-template emission before the modified generic
primitive preflight: 677 of 2180 main-thread samples enter the template emitter
through stringify_object_inner. The template builder itself has 180 top-of-stack
samples, and malloc/free contribute 136. The per-call template is rebuilt and
cleared at each top-level stringify call. This weakens the earlier speculation
that the added generic primitive checks caused this row's CPU regression.

A more substantial next optimization is direct output for small records with
primitive-array fields, avoiding the per-call prefix Strings/Vec/Box as well as
the growable output copy. It must use bounded stack plans, reject callbacks,
array descriptors/holes/named properties and non-inline or complex layouts,
root the original input before any allocating prototype probe and final output
allocation, then re-read keys and child arrays after GC. Existing small scalar
object/string fast paths should continue to return before the larger plan.
This is an investigation direction, not an implemented or measured change.
