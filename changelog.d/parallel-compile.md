Compile large programs on all cores. The Claude Code bundle (13 MB) spent about
1,500 of its ~2,100 seconds of `perry compile` on one core before LLVM started,
and the LLVM units then ran eight at a time behind a producer that froze them
one by one. The serial time was not inherent work: six passes were
superlinear in the size of the module. Each is now linear, with byte-identical
output:

- `root_reload` answers its per-root-load questions from sorted position lists
  instead of rescanning the function's instructions for every root load.
- Class keys globals resolve their class through one sanitized-name index per
  pass instead of re-sanitizing every class name per global.
- `module_const_fold` no longer re-walks every nested statement list's
  expressions once per enclosing list.
- A per-module `ClassHierarchy` (subclasses per class, declaring classes per
  method name) answers the method-dispatch and class-field subclass questions
  that walked every class in the module at every call site and field access.
- Callee-binding resolution reads the capture and module-global maps directly
  instead of collecting module-wide id sets per function.
- The codegen-unit layout renders each function once: the native backend
  freezes its worker payload from the text the reference scan already
  produced, and the whole-function text passes (precise-root lowering, leaf
  annotation, landing-pad retype) run on the unit workers.

`PERRY_CODEGEN_UNIT_JOBS` is now the one knob for both unit backends. Its
default is every available core, bounded by available memory (one worker per
2 GiB); Windows keeps two. Units are slotted by index, so the object is
identical for any worker count.
