Generator resumes on a private frame keep the state, completion kind and done
controls in registers for the length of one resume and write them back on every
exit, instead of re-reading the owner through the frame for every access. The
operation is an integer argument, the try-scope handle height is read once per
resume, a transition to a known state branches straight to that state instead of
through the dispatch switch again, and the sealed private caller supplies the
constant sent value, which removes its barrier test. The executing flag, the
sent slot and every user slot stay in the frame. No cache, side table or knob.

Cover return and throw injection mid-loop, try/finally and in-generator catch,
early return from for-of, nested frames and moving-collector stress in gap
tests, and assert in codegen tests that every exit writes the controls back.
