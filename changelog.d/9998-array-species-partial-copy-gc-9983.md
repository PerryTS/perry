**`Array.prototype.slice` and `splice` keep a custom species result's pointer
layout valid when an element getter throws** (#9983). A custom
`Symbol.species` constructor can return an existing plain array. The exotic
copy path wrote its slots directly and rebuilt the GC layout only after the
whole loop returned, so a later throwing getter skipped the rebuild and left
an earlier heap pointer permanently absent from slot enumeration. A later
collection could then sweep that child while the array still named it.

The receiver and species result now stay rooted across constructors and
getters. Custom-species elements use `CreateDataPropertyOrThrow`, which updates
the slot layout immediately and also rejects frozen or non-extensible results;
only a fresh default-species array uses the raw dense copy. The regression
forces evacuation after both a throwing `slice` and a throwing `splice`,
requires `PERRY_GC_VERIFY_MARK` to report `OK` with no unenumerated slots, and
checks that frozen custom results throw before `splice` mutates its source.
