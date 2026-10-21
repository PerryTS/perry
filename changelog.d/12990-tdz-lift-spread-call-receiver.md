A spread call on a module-level `let`/`const` in its temporal dead zone no
longer loses its receiver. The TDZ check is now lifted out of a spread call's
member callee, as it already was for a plain call, so `arr.push(...xs)` lowers
as `(check, arr.push(...xs))` rather than `(check, arr.push)(...xs)`. The
latter is an unbound call, which left codegen's member-aware spread arms
unmatched and reached the array mutators with `this === undefined`, throwing
`TypeError: Cannot convert undefined or null to object` for a receiver that
exists. Visible as `arr.push(x)` working while `arr.push(...xs)` threw on the
same array, in a module whose binding is read across an import cycle.
