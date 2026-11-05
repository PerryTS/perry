Per-unit program facts no longer claim more than the whole program in three
cases. A unit that hands `Object.prototype` to unknown code now writes only the
keys stored into ⊤ onto the builtin prototypes (not every key); a number-key
read of an intrinsic prototype sees only number-key writes; and an
`Object.setPrototypeOf` on an unknown target in one unit reaches the other
units, as it does in the whole program.
