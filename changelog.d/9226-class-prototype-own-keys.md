Class prototypes now expose one coherent, spec-ordered own-key surface.
`Object.getOwnPropertyNames` includes class accessors without leaking Perry's
internal `@@iterator` dispatch alias, `Object.getOwnPropertySymbols` includes
the corresponding `Symbol.iterator` (and other computed Symbol members), and
`Reflect.ownKeys` returns their union: integer-index strings first, remaining
strings in property-creation order, then Symbols in property-creation order.

Class member registration now retains source order across the separate method,
getter, setter, and Symbol registries. Static fields retain first-install order,
and deleting then recreating a class key moves it to the end as required.
Symbol-keyed class members also agree across enumeration,
`hasOwnProperty`, and `getOwnPropertyDescriptor`.
