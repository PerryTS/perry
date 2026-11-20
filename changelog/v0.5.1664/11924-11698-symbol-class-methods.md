Symbol-keyed class methods now live as real properties in the declared
prototype's shape. As a result, deleting `C.prototype[Symbol.iterator]`
actually removes the method, and `super[Symbol.iterator]()` starts its lookup
at the parent prototype instead of recursively selecting the derived override
(#11698).
