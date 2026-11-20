A class's standalone constructor (the body `super(...)` and `new` of a class
value run) is compiled under its constructor's own function id instead of
id 0. Its `this`, definite-init proofs and allocation hotness were read
from whichever function had HIR id 0; now a store `this.k = v` into a
definite birth field of a prefix-stable class is the slot store the inlined
`new C(...)` already emits. A synthesized default constructor gets an id
nothing is keyed by.
