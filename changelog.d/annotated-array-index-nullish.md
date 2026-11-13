`a[0]` and `a[0] = v` on `null` / `undefined` behind an Array annotation (`function f(a: number[])` given `null`)
throw node's TypeError, in sloppy code too; they used to answer `undefined` and drop the store.
