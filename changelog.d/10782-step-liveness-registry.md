Registered the new "Pre-warm auto-optimized runtime" step in
`scripts/compiler_output_step_liveness.py`'s `COMPILER_SUBJECTS`.

That gate refuses to let the `compiler-output-regression` job's post-build step
inventory drift silently, and it caught the addition immediately — which is the
gate working, not a nuisance. Registering the step deliberately is the intended
response.

It belongs in `COMPILER_SUBJECTS` rather than `UNIT_TEST_SUBJECTS` because it
depends on the build, and it already carries the matching guard
(`!cancelled() && steps.compiler_output_build.outcome == 'success'`) so a
sibling failure cannot hide it.

Listing it matters beyond satisfying the check: the pre-warm carries its own
liveness assertions and can fail, so it is a subject. Were it removed or
silently disabled, every gate step below would quietly inherit the 300 s
timeout the pre-warm exists to prevent.
