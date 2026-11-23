Reading `.size` of a `Map`/`Set`-typed value that holds `undefined` or `null` throws `TypeError: Cannot read properties of undefined (reading 'size')` (or `of null`), as Node does.
