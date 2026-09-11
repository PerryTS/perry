Let LLVM choose ordinary shadow-root inlining under `-Oz` instead of forcing
every small-body candidate into its callers. Profitable inlines remain allowed;
normal optimization levels, `-Os`, native-root hints, explicit attributes, and
the separately admitted pre-statepoint inline path keep their existing behavior.

The shared definition-header renderer carries the policy into textual IR and
native LLVM construction. Independent header tests and a pinned-Node native
fixture cover both transports, both root modes, Os/Oz controls, exception and
object identity, callbacks, and actual moving collections. Scoped native CI
installs the repository's exact Node oracle.
