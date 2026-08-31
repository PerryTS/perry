### fix(runtime): preserve custom array prototypes across growth

Growing an array's dense element storage now transfers its recorded
`[[Prototype]]` metadata to the replacement allocation. Moving GC now
rekeys and traces the same metadata for arrays as well. An array retargeted
with `Object.setPrototypeOf`, `Reflect.setPrototypeOf`, or `__proto__`
therefore keeps that exact prototype across collection and after an indexed
write or array mutator reallocates its backing storage.

In particular, an explicit null prototype no longer silently regains
`Array.prototype` when the array grows. Fixes #9304.
