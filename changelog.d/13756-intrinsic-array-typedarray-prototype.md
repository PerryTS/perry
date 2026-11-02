`Object.getPrototypeOf` of an array or a typed array answers the realm's own
`Array.prototype` / `<Kind>Array.prototype`, not the `prototype` of whatever
`globalThis.Array` / `globalThis.Uint8Array` currently holds, so after
`globalThis.Array = function F() {}`, `[] instanceof Array` is `false`, as in
node.
