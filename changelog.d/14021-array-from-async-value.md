`Array.fromAsync` is now an own function value on the Array constructor. Held references
and `.call` with a non-constructor receiver reach the async implementation, including
mapping and `thisArg`; the function exposes the standard `length` of `1` and name
`fromAsync`.
