`a[0]`, `a[i]`, `a[-1]`, `a["0"]` and `a[0] = v` on `null` / `undefined` behind an Array annotation
(`function f(a: number[])` given `null`) throw node's TypeError, in sloppy code too; they used to answer `undefined`
(or `NaN`) and drop the store. The test sits only in the element read's and the typed fallbacks' failure arms.
