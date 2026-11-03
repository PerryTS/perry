A class that extends nothing no longer emits a super-construct when an
imported class shares its name. `requires_live` consulted
`imported_class_ctors` by bare class name before establishing the class was
derived, and a type-only import cycle registers an entry under the name of a
class the module itself declares. The collision reported live heritage for a
class with no parent, so the emitted super-construct found an undefined parent
and threw `TypeError: Super constructor is not a constructor` from a class
whose file contains no `extends` and no `super`. Both name-keyed lookups are
now guarded by the same derived test the heritage walk already applies.
