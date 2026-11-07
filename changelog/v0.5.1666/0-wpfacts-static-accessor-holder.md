Whole-program facts: a `static get` / `static set` accessor belongs to the class
object, as in the compiled program, not to the class prototype. Reading `C.x`
(or `D.x` on a subclass) runs the static getter; the facts looked for it on
instances, so they claimed it never runs, and the run-time claim check mode
reported it on gap programs. An instance read under the same name no longer
counts as running it.
