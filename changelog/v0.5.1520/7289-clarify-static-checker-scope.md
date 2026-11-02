`gc-rooting-invariant.md` said the static checker was "structurally blind to this
class" and, eight lines later, "the only instrument that sees this class before
it crashes". Both sentences were about different classes and read as a
contradiction.

The checker's scope is now stated as emitted-LLVM rooting hazards, with its three
known blind spots named and attributed: runtime tables/interning caches (#7231),
unrooted locals in runtime Rust (#7249), and anything its exact-emitted-symbol
sets fail to name (#7284, where property GETs were unread because the set carried
a symbol codegen never emits).
