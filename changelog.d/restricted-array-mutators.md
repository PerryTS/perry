`pop` / `shift` / `unshift` / `splice` / `reverse` on a frozen, sealed or non-extensible Array, or one whose `length`
is read-only, run the specification's steps and throw at the first step that fails, as node does: a sealed `pop`
throws before it shrinks the Array, a sparse `splice` on a non-extensible Array is performed instead of refused.
