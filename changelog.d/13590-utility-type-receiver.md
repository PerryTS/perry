A receiver typed with a TypeScript utility type such as `Pick<Svc, "join">`,
`Omit<Svc, "sep">` or `Readonly<Svc>` was lowered as an array, so
`p.join(root, "x")` folded into `Array.prototype.join` and dropped every
argument after the first. Only `Array` and `ReadonlyArray` generics are now
treated as array receivers; any other generic type dispatches the method on
the receiver itself.
