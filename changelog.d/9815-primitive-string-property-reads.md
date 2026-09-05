Fix primitive string property reads with computed keys. Named properties now
consult `String.prototype` and preserve the original method value, so reflective
read-then-call code and method identity checks work. Inherited accessors receive
the primitive string as `this`; object and symbol keys follow `ToPropertyKey`.
Character indices and `length` keep precedence over prototype properties, and
boxed strings retain their own-property lookup before their custom prototype.

Cover typed and untyped receivers, short strings, borrowed methods, inherited
accessors, symbol keys, key coercion, and prototype mutation in runtime unit
tests and a Node parity fixture. Direct method-call lowering is unchanged.
