`--target web`: a method call or a property read on `null` / `undefined`
(`n.foo()`, `n.bar`) did nothing and yielded `undefined`. It now throws
`TypeError: Cannot read properties of null (reading 'bar')`, which `catch`
sees, as in JS.
