### Fixed

- Apply declared GC witness settings during both compilation and execution of
  the moving loop-poll arm, so seeded and protected witnesses select the matching
  instrumented runtime. Validate the settings as literal GC assignments, keep
  the compiled group separate from the safepoint control, and record the actual
  compile and run settings. Exercise routing failure controls in the witness
  workflow. Preserve the other arms, witness workloads, triage and movement
  requirements.
