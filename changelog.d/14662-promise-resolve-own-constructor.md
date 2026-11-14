`Promise.resolve(p).then(..)` on a native promise with its own `constructor`
property now goes through PromiseResolve, so an abrupt `then` (for example the
SpeciesConstructor `TypeError` for a non-object `constructor`) rejects the new
promise instead of throwing synchronously and uncaught (#13556, test262
`built-ins/Promise/resolve/arg-uniq-ctor`).
