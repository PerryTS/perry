### ConstFn method lanes call the method body directly

A completed shape whose ConstFn lane names a method's body now decides the
call. A method site whose receiver has a compile-time class candidate (an
object-literal or factory local, or `this` in a literal method) compares the
receiver word with each completed ShapeId that carries the lane and calls that
body directly. The slot value is passed only as the callee's environment and is
never checked or called through. Other receivers use the learned site. Its
ConstFn entries are decoded first and call the recorded body with exactly the
call's arguments, with no kind or info check of the slot. An inherited method
whose prototype's shape carries the lane is now a ConstFn entry: the receiver
word pins the holder and the holder word pins the body. A prototype keeps the
ConstFn lanes it still satisfies when it is marked as a prototype. A method
shorthand that doesn't use `this`, which is hoisted to a module function, gets
a lane for its function-value body.

A learned ConstFn hit whose recorded body is one of the site's compile-time
candidates (for example `this.m()` inside a prototype literal's own method,
reached by an inheriting receiver) calls that body directly. Compact lane
bodies are admitted to the early inliner, so these direct calls flatten the
way a class method's exact-receiver clone does.

The receiver guard in a method body compares the completed id before the
birth id, so a finalized literal hits on its first compare. Any store of
another body still deprecates the lane, and the site falls back to the
generic path.

Tests cover the direct-call IR, a single-compare body guard, reassignment
(another body, and the same body with other captures), accessors, deletes and
prototype writes. They run ON and OFF, under forced evacuation and in a
worker. Skipping the deprecation on a ConstFn store makes three of the four
tests fail.
