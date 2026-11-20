**A generic class specialization now answers with its generic's constructor, and sees its generic's prototype patches.**
`new Gen<number>()` is monomorphized into a separate class (`Gen$num`) with its
own class id; TypeScript erases type arguments, so at runtime there is exactly
one `Gen`. After `instanceof` (#7575), `constructor.name` (#7632) and the two
prototype-object registries (#7762), two id-keyed holes remained. The instance
`.constructor` arm synthesized the class ref straight from the instance's class
id, so `a.constructor !== Gen` and `a.constructor !== b.constructor` — and since
#7632 gave both the display name `Gen`, they printed identically while comparing
unequal. Separately, and untouched by #7762's aliasing of the prototype
*objects*, the prototype-method chain walk started at the specialization's id
and followed only parent edges, so `Gen.prototype.tag = "G"` was invisible from a
specialized instance even though `Object.getPrototypeOf(a) === Gen.prototype`
reported true — the two edges disagreed about the same object. Both now take the
same generic-origin edge, with the generic tried *before* the parent because it
is an alias rather than an ancestor. Method dispatch is deliberately not aliased:
it runs off the per-class-id vtable, so each specialization keeps its own
monomorphized bodies. Covered by
`test-files/test_gap_generic_specialization_constructor_identity_7757.ts`, which
also pins the negative direction — distinct generics stay distinct, a specialized
subclass reports the subclass rather than its base, and `instanceof` still
discriminates. (#7757)
