`--target web`: `instanceof` a built-in constructor (`e instanceof SyntaxError`,
`a instanceof Array`, `x instanceof Error`) was always `false`, because the
check only walked user-class chains; and a user-class instance was not an
instance of `Object`. A host object is now checked against the host
constructor of that name, and a user-class instance is an `Object`.
