### Fixed

- Counted loops over a `class X extends Array` instance read element 0 as the object's `meta` word (a bare heap pointer seen as `1.8e-311`): the loop guard admitted such a receiver as kind 1 — "the receiver IS the ArrayHeader" — while handing back the elements store's address, and the generated loop derives its base from the receiver. Elements-backed receivers are now admitted as their own kind, with the payload address published in the descriptor and refreshed by every revalidation, so those loops stay on the fast path and read the right memory.
