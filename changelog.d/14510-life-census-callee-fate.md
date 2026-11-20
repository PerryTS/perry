The compile-time allocation census (`PERRY_LIFE_CENSUS`, compiler-internal and
off by default) now says what receives each heap birth that is not bound to a
local: the compiled callee with that parameter's fate, an unresolved target
with its reason, a builtin, an object literal field, a constructor's fate, or
what the visible callers do with a returned birth; and the verdict of the birth
that holds it. Field stores are proven by one predicate shared by the lowering
and the census. Generated code does not change.
